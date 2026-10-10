use std::sync::Arc;
use std::time::Duration;

use chatsvc::{EventKind, MessageEvent, Pins, Realtime, RealtimeEvent, StatusKind, TypingEvent};
use chrono::{DateTime, Utc};
use graph::Graph;
use session::{Session, SessionConfig, Transport};
use store::Store;
use teams_core::{ChatsvcFolderSource, CoreEvent, SyncEngine};
use tokio::sync::mpsc;

use crate::call::CallLauncher;

pub type Engine = SyncEngine<Graph>;

const CONNECT_RETRY: Duration = Duration::from_secs(8);
const REFRESH_INTERVAL: Duration = Duration::from_secs(120);
const REFRESH_SETTLE: Duration = Duration::from_secs(2);
const PINS_INTERVAL: Duration = Duration::from_secs(90);
const REALTIME_RETRY: Duration = Duration::from_secs(15);
const PRESENCE_RESUBSCRIBE: Duration = Duration::from_secs(30 * 60);
const MAX_ERROR_CHARS: usize = 120;
const IMAGE_DIRECTORY: &str = "images";
const IMAGE_CACHE_MAX_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Online,
    NoBrowser,
    NoAppTab,
    LoginRequired,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveState {
    Off,
    Connecting,
    Live,
    Reconnecting,
    MessageLoss,
    Failed,
}

pub enum BackendEvent {
    Connection(ConnectionState),
    Engine(Arc<Engine>),
    Calls(Arc<CallLauncher>),
    Core(CoreEvent),
    Live(LiveState),
    Typing(TypingEvent),
    Favorites(Vec<String>),
    Synced(DateTime<Utc>),
}

type EventSender = mpsc::UnboundedSender<BackendEvent>;

pub fn classify_session_error(error: &session::Error) -> ConnectionState {
    match error {
        session::Error::NoBrowser { .. } => ConnectionState::NoBrowser,
        session::Error::NoAppTab => ConnectionState::NoAppTab,
        session::Error::LoginRequired(_) | session::Error::NoFreshToken { .. } => {
            ConnectionState::LoginRequired
        }
        other => ConnectionState::Failed(shorten(&other.to_string())),
    }
}

pub fn classify_core_error(error: &teams_core::Error) -> ConnectionState {
    match error {
        teams_core::Error::Graph(graph::Error::Session(inner)) => classify_session_error(inner),
        other => ConnectionState::Failed(shorten(&other.to_string())),
    }
}

fn image_directory() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(IMAGE_DIRECTORY))
}

fn shorten(text: &str) -> String {
    text.chars().take(MAX_ERROR_CHARS).collect()
}

pub fn live_state_for(kind: StatusKind) -> LiveState {
    match kind {
        StatusKind::Connected => LiveState::Live,
        StatusKind::Disconnected => LiveState::Reconnecting,
        StatusKind::MessageLoss => LiveState::MessageLoss,
        StatusKind::Error => LiveState::Failed,
    }
}

pub fn start(
    store: Arc<Store>,
    transport: Arc<dyn Transport>,
) -> mpsc::UnboundedReceiver<BackendEvent> {
    let (sender, receiver) = mpsc::unbounded_channel();
    crate::runtime::handle().spawn(supervise(store, transport, sender));
    receiver
}

async fn supervise(store: Arc<Store>, transport: Arc<dyn Transport>, events: EventSender) {
    let _ = events.send(BackendEvent::Connection(ConnectionState::Connecting));
    let session = loop {
        match Session::with_transport(transport.clone(), SessionConfig::default()).await {
            Ok(session) => break session,
            Err(error) => {
                let _ = events.send(BackendEvent::Connection(classify_session_error(&error)));
                tokio::time::sleep(CONNECT_RETRY).await;
            }
        }
    };
    let build = || {
        SyncEngine::new(Graph::new(session.clone()), store.clone())
            .with_folder_source(Arc::new(ChatsvcFolderSource::new(&session)))
    };
    let engine = image_directory()
        .and_then(|directory| {
            build()
                .with_image_dir(&directory, IMAGE_CACHE_MAX_BYTES)
                .ok()
        })
        .unwrap_or_else(build);
    let engine = Arc::new(engine);
    let _ = events.send(BackendEvent::Engine(engine.clone()));
    let _ = events.send(BackendEvent::Calls(Arc::new(CallLauncher::new(transport))));
    tokio::spawn(forward_core_events(engine.clone(), events.clone()));
    let (nudge, nudges) = mpsc::unbounded_channel();
    tokio::spawn(realtime_loop(
        session.clone(),
        engine.clone(),
        events.clone(),
        nudge,
    ));
    tokio::spawn(pins_loop(session, events.clone()));
    tokio::spawn(presence_resubscribe_loop(engine.clone()));
    refresh_loop(engine, events, nudges).await;
}

async fn forward_core_events(engine: Arc<Engine>, events: EventSender) {
    let mut receiver = engine.subscribe();
    loop {
        let event = match receiver.recv().await {
            Ok(event) => event,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => CoreEvent::SidebarChanged,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        };
        if events.send(BackendEvent::Core(event)).is_err() {
            return;
        }
    }
}

async fn refresh_loop(
    engine: Arc<Engine>,
    events: EventSender,
    mut nudges: mpsc::UnboundedReceiver<()>,
) {
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            nudge = nudges.recv() => {
                if nudge.is_none() {
                    return;
                }
                tokio::time::sleep(REFRESH_SETTLE).await;
                while nudges.try_recv().is_ok() {}
            }
        }
        match engine.refresh_sidebar().await {
            Ok(_) => {
                let _ = engine.refresh_folders().await;
                let _ = events.send(BackendEvent::Connection(ConnectionState::Online));
                let _ = events.send(BackendEvent::Synced(Utc::now()));
            }
            Err(error) => {
                let _ = events.send(BackendEvent::Connection(classify_core_error(&error)));
                tokio::time::sleep(CONNECT_RETRY).await;
            }
        }
    }
}

async fn realtime_loop(
    session: Session,
    engine: Arc<Engine>,
    events: EventSender,
    nudge: mpsc::UnboundedSender<()>,
) {
    loop {
        let _ = events.send(BackendEvent::Live(LiveState::Connecting));
        match Realtime::start(&session).await {
            Ok(mut realtime) => {
                while let Some(event) = realtime.recv().await {
                    match event {
                        RealtimeEvent::Message(message) => {
                            let _ = events.send(BackendEvent::Live(LiveState::Live));
                            let receipt_engine = engine.clone();
                            let receipt_event = message.clone();
                            engine.handle_pin_event(&message);
                            tokio::spawn(async move {
                                receipt_engine.handle_receipt_event(&receipt_event).await
                            });
                            tokio::spawn(handle_message_event(
                                engine.clone(),
                                nudge.clone(),
                                message,
                            ));
                        }
                        RealtimeEvent::Typing(typing) => {
                            let _ = events.send(BackendEvent::Typing(typing));
                        }
                        RealtimeEvent::Status(status) => {
                            let _ = events.send(BackendEvent::Live(live_state_for(status.kind)));
                            let receipt_engine = engine.clone();
                            let receipt_status = status.clone();
                            tokio::spawn(async move {
                                receipt_engine.handle_receipt_status(&receipt_status).await
                            });
                        }
                        RealtimeEvent::Presence(updates) => {
                            engine.apply_presence(&updates);
                            if engine.concerns_me(&updates) {
                                let engine = engine.clone();
                                tokio::spawn(async move { engine.refresh_own_status().await });
                            }
                        }
                        RealtimeEvent::Endpoint(endpoint) => {
                            let engine = engine.clone();
                            tokio::spawn(async move { engine.presence_endpoint(endpoint).await });
                        }
                    }
                }
                let _ = events.send(BackendEvent::Live(LiveState::Reconnecting));
            }
            Err(_) => {
                let _ = events.send(BackendEvent::Live(LiveState::Failed));
            }
        }
        tokio::time::sleep(REALTIME_RETRY).await;
    }
}

async fn presence_resubscribe_loop(engine: Arc<Engine>) {
    let mut interval = tokio::time::interval(PRESENCE_RESUBSCRIBE);
    interval.tick().await;
    loop {
        interval.tick().await;
        let _ = engine.resubscribe_presence().await;
    }
}

pub fn refresh_plan(event: &MessageEvent) -> Option<RefreshPlan> {
    let conversation_id = event.conversation_id.clone()?;
    match event.kind {
        EventKind::MessageUpdate => Some(match event.message_id.clone() {
            Some(message_id) => RefreshPlan::Message {
                conversation_id,
                message_id,
            },
            None => RefreshPlan::Newer { conversation_id },
        }),
        EventKind::NewMessage => Some(RefreshPlan::Newer { conversation_id }),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshPlan {
    Newer {
        conversation_id: String,
    },
    Message {
        conversation_id: String,
        message_id: String,
    },
}

async fn handle_message_event(
    engine: Arc<Engine>,
    nudge: mpsc::UnboundedSender<()>,
    event: MessageEvent,
) {
    let Some(plan) = refresh_plan(&event) else {
        return;
    };
    let outcome = match &plan {
        RefreshPlan::Newer { conversation_id } => {
            engine.fetch_newer(conversation_id).await.map(|_| ())
        }
        RefreshPlan::Message {
            conversation_id,
            message_id,
        } => engine
            .refresh_message(conversation_id, message_id)
            .await
            .map(|_| ()),
    };
    let unknown = matches!(outcome, Err(teams_core::Error::UnknownConversation(_)));
    if unknown || event.kind == EventKind::NewMessage {
        let _ = nudge.send(());
    }
}

async fn pins_loop(session: Session, events: EventSender) {
    let pins = Pins::new(&session);
    let mut chat_ids: Vec<String> = Vec::new();
    let mut channel_ids: Vec<String> = Vec::new();
    let mut interval = tokio::time::interval(PINS_INTERVAL);
    loop {
        interval.tick().await;
        if let Ok(pinned) = pins.pinned_chats().await {
            chat_ids = pinned.chat_ids;
        }
        if let Ok(pinned) = pins.pinned_channels().await {
            channel_ids = pinned.channel_ids;
        }
        let combined = chat_ids.iter().chain(channel_ids.iter()).cloned().collect();
        if events.send(BackendEvent::Favorites(combined)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: EventKind, conversation: Option<&str>, message: Option<&str>) -> MessageEvent {
        MessageEvent {
            resource_type: "x".into(),
            kind,
            conversation_id: conversation.map(str::to_owned),
            message_id: message.map(str::to_owned),
            received_at: Utc::now(),
        }
    }

    #[test]
    fn new_message_fetches_newer() {
        assert_eq!(
            refresh_plan(&event(EventKind::NewMessage, Some("c"), Some("m"))),
            Some(RefreshPlan::Newer {
                conversation_id: "c".into()
            })
        );
    }

    #[test]
    fn update_refreshes_one_message() {
        assert_eq!(
            refresh_plan(&event(EventKind::MessageUpdate, Some("c"), Some("m"))),
            Some(RefreshPlan::Message {
                conversation_id: "c".into(),
                message_id: "m".into()
            })
        );
        assert_eq!(
            refresh_plan(&event(EventKind::MessageUpdate, Some("c"), None)),
            Some(RefreshPlan::Newer {
                conversation_id: "c".into()
            })
        );
    }

    #[test]
    fn typing_and_missing_conversation_are_ignored() {
        assert_eq!(
            refresh_plan(&event(EventKind::Typing, Some("c"), None)),
            None
        );
        assert_eq!(
            refresh_plan(&event(EventKind::NewMessage, None, None)),
            None
        );
    }

    #[test]
    fn session_errors_map_to_states() {
        assert_eq!(
            classify_session_error(&session::Error::NoAppTab),
            ConnectionState::NoAppTab
        );
        assert_eq!(
            classify_session_error(&session::Error::LoginRequired("x".into())),
            ConnectionState::LoginRequired
        );
        assert_eq!(live_state_for(StatusKind::Connected), LiveState::Live);
    }
}
