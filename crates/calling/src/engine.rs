use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chatsvc::{CallbackReplier, InstanceNames, Realtime, RealtimeConfig, RealtimeEvent, TrouterCallback, TrouterEndpoint};
use session::Session;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::call::{CallOptions, CallSpec, IncomingCall, Plan};
use crate::control::{CallHandle, call_channel};
use crate::end::EndKind;
use crate::error::{Error, Result};
use crate::push::{CallNotification, Caller, PushEvent, decode_push};
use crate::relay::DEFAULT_RELAY_HOST;
use crate::signaling::{AttachRequest, Participant, SelfIdentity, Signaling, TenantRouting, fetch_self};
use crate::timeline::Timeline;
use crate::trouter_events::{CallEvent, CallbackLinks, callback_call_id, classify, decode_body};

const ENDPOINT_WAIT: Duration = Duration::from_secs(30);
const PRESENCE_SUFFIX: &str = "unifiedPresenceService";
const SEEN_WINDOW: Duration = Duration::from_secs(120);
const RING_ID_START: u64 = 1;
pub const TRACE_ENV: &str = "CALLING_TRACE";

pub fn calling_instance_names() -> InstanceNames {
    InstanceNames {
        global: "__callingTrouter".into(),
        binding: "__callingRealtime".into(),
        endpoint_storage_key: "__callingEpid".into(),
    }
}

pub fn calling_realtime_config(ringable: bool, instance: InstanceNames) -> RealtimeConfig {
    RealtimeConfig {
        instance,
        forward_callbacks: true,
        ringable,
        ..RealtimeConfig::default()
    }
}

pub(crate) fn callback_base(trouter_uri: &str) -> String {
    let trimmed = trouter_uri.strip_suffix(PRESENCE_SUFFIX).unwrap_or(trouter_uri).trim_end_matches('/');
    format!("{trimmed}/")
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub ringable: bool,
    pub routing: TenantRouting,
    pub relay_host: String,
    pub trace: bool,
    /// Page globals of the Trouter client; a second engine in the same tab needs its own.
    pub instance: InstanceNames,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            ringable: true,
            routing: TenantRouting::default(),
            relay_host: DEFAULT_RELAY_HOST.to_owned(),
            trace: std::env::var_os(TRACE_ENV).is_some(),
            instance: calling_instance_names(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncomingRing {
    pub ring_id: u64,
    pub caller: Caller,
    pub thread_id: Option<String>,
    pub video: bool,
    pub is_group: bool,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    Incoming(IncomingRing),
    RingEnded {
        ring_id: u64,
        kind: EndKind,
        answered_by: Option<String>,
    },
}

enum RingCommand {
    Accept(oneshot::Sender<Option<CallHandle>>),
    Decline,
    Drop,
}

pub(crate) struct Inner {
    pub(crate) session: Session,
    pub(crate) poll_session: Session,
    pub(crate) config: EngineConfig,
    pub(crate) identity: SelfIdentity,
    pub(crate) replier: CallbackReplier,
    pub(crate) gate: Arc<tokio::sync::Mutex<()>>,
    endpoint: Mutex<TrouterEndpoint>,
    routes: Mutex<HashMap<String, mpsc::UnboundedSender<TrouterCallback>>>,
    rings: Mutex<HashMap<u64, mpsc::UnboundedSender<RingCommand>>>,
    seen: Mutex<HashMap<(String, String), Instant>>,
    next_ring_id: AtomicU64,
    runtime: tokio::runtime::Handle,
    events: mpsc::UnboundedSender<EngineEvent>,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Inner {
    pub(crate) fn endpoint(&self) -> TrouterEndpoint {
        self.endpoint.lock().expect("endpoint lock").clone()
    }

    pub(crate) fn register_route(&self, call_id: &str) -> mpsc::UnboundedReceiver<TrouterCallback> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.routes.lock().expect("routes lock").insert(call_id.to_owned(), sender);
        receiver
    }

    pub(crate) fn unregister_route(&self, call_id: &str) {
        self.routes.lock().expect("routes lock").remove(call_id);
    }

    pub(crate) fn participant(&self, participant_id: &str) -> Participant {
        Participant {
            mri: format!("8:orgid:{}", self.identity.object_id),
            display_name: self.identity.display_name.clone(),
            endpoint_id: self.endpoint().endpoint_id,
            participant_id: participant_id.to_owned(),
            language_id: "en-gb".into(),
        }
    }

    pub(crate) fn callback_links(&self) -> CallbackLinks {
        CallbackLinks::new(&callback_base(&self.endpoint().trouter_uri), &uuid::Uuid::new_v4().to_string())
    }

    pub(crate) fn signaling(&self, timeline: Timeline) -> Signaling {
        Signaling::new(self.session.clone(), self.poll_session.clone(), self.config.routing.clone(), timeline)
    }

    pub(crate) fn call_options(&self) -> CallOptions {
        CallOptions {
            relay_host: self.config.relay_host.clone(),
            routing: self.config.routing.clone(),
            trace: self.config.trace,
            ..CallOptions::default()
        }
    }

    fn route(&self, callback: TrouterCallback) {
        let Some(call_id) = callback_call_id(&callback.path) else {
            self.replier.reply(callback.request_id, 200, "");
            return;
        };
        let sender = self.routes.lock().expect("routes lock").get(&call_id).cloned();
        match sender {
            Some(sender) => {
                if let Err(rejected) = sender.send(callback) {
                    self.replier.reply(rejected.0.request_id, 200, "");
                }
            }
            None => self.replier.reply(callback.request_id, 200, ""),
        }
    }

    fn already_seen(&self, notification: &CallNotification) -> bool {
        let mut seen = self.seen.lock().expect("seen lock");
        let now = Instant::now();
        seen.retain(|_, at| now.duration_since(*at) < SEEN_WINDOW);
        seen.insert(notification.dedupe_key(), now).is_some()
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }
}

#[derive(Clone)]
pub struct CallEngine {
    pub(crate) inner: Arc<Inner>,
}

impl CallEngine {
    pub async fn start(
        session: Session,
        poll_session: Session,
        config: EngineConfig,
    ) -> Result<(CallEngine, mpsc::UnboundedReceiver<EngineEvent>)> {
        let mut realtime = Realtime::start_with(&session, calling_realtime_config(config.ringable, config.instance.clone())).await?;
        let callbacks = realtime
            .take_callbacks()
            .ok_or_else(|| Error::Callback("callback stream already taken".into()))?;
        let replier = realtime.callback_replier();
        let endpoint = timeout(ENDPOINT_WAIT, async {
            while let Some(event) = realtime.recv().await {
                if let RealtimeEvent::Endpoint(endpoint) = event {
                    return Some(endpoint);
                }
            }
            None
        })
        .await
        .ok()
        .flatten();
        let Some(endpoint) = endpoint else {
            realtime.stop().await;
            return Err(Error::Callback("Trouter socket never announced its endpoint".into()));
        };
        let identity = match fetch_self(&session).await {
            Ok(identity) => identity,
            Err(error) => {
                realtime.stop().await;
                return Err(error);
            }
        };
        let (events, event_receiver) = mpsc::unbounded_channel();
        let (stop, stop_receiver) = oneshot::channel();
        let inner = Arc::new(Inner {
            session,
            poll_session,
            config,
            identity,
            replier,
            gate: Arc::new(tokio::sync::Mutex::new(())),
            endpoint: Mutex::new(endpoint),
            routes: Mutex::default(),
            rings: Mutex::default(),
            seen: Mutex::default(),
            next_ring_id: AtomicU64::new(RING_ID_START),
            runtime: tokio::runtime::Handle::current(),
            events,
            stop: Mutex::new(Some(stop)),
            tasks: Mutex::default(),
        });
        let engine = CallEngine { inner: inner.clone() };
        let pump = tokio::spawn(pump_realtime(realtime, inner.clone(), stop_receiver));
        let router = tokio::spawn(route_callbacks(callbacks, engine.clone()));
        inner.tasks.lock().expect("tasks lock").extend([pump, router]);
        Ok((engine, event_receiver))
    }

    pub fn session(&self) -> Session {
        self.inner.session.clone()
    }

    pub fn start_call(&self, spec: CallSpec) -> CallHandle {
        let (handle, control) = call_channel();
        let engine = self.clone();
        self.inner.runtime.spawn(async move {
            let options = engine.inner.call_options();
            let _ = engine.run(Plan::Outgoing(spec), options, control).await;
        });
        handle
    }

    pub async fn accept_ring(&self, ring_id: u64) -> Option<CallHandle> {
        let commands = self.inner.rings.lock().expect("rings lock").get(&ring_id).cloned()?;
        let (answer, handle) = oneshot::channel();
        commands.send(RingCommand::Accept(answer)).ok()?;
        handle.await.ok().flatten()
    }

    /// Only for a click on Decline: it ends the call for all of the user's devices.
    pub fn decline_ring(&self, ring_id: u64) {
        self.send_ring_command(ring_id, RingCommand::Decline);
    }

    /// Stops ringing locally without telling Teams; the call keeps ringing elsewhere.
    pub fn drop_ring(&self, ring_id: u64) {
        self.send_ring_command(ring_id, RingCommand::Drop);
    }

    fn send_ring_command(&self, ring_id: u64, command: RingCommand) {
        let commands = self.inner.rings.lock().expect("rings lock").get(&ring_id).cloned();
        if let Some(commands) = commands {
            let _ = commands.send(command);
        }
    }

    pub async fn stop(&self) {
        let stop = self.inner.stop.lock().expect("stop lock").take();
        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        let tasks: Vec<JoinHandle<()>> = std::mem::take(&mut *self.inner.tasks.lock().expect("tasks lock"));
        for task in tasks {
            let _ = task.await;
        }
    }
}

async fn pump_realtime(mut realtime: Realtime, inner: Arc<Inner>, mut stop: oneshot::Receiver<()>) {
    let mut last_status = None;
    loop {
        tokio::select! {
            event = realtime.recv() => match event {
                Some(RealtimeEvent::Endpoint(endpoint)) => *inner.endpoint.lock().expect("endpoint lock") = endpoint,
                Some(RealtimeEvent::Status(status)) => {
                    if last_status.as_ref() != Some(&status) {
                        eprintln!("calls: trouter {:?} {}", status.kind, status.detail);
                        last_status = Some(status);
                    }
                }
                Some(_) => {}
                None => return,
            },
            _ = &mut stop => {
                realtime.stop().await;
                return;
            }
        }
    }
}

async fn route_callbacks(mut callbacks: mpsc::UnboundedReceiver<TrouterCallback>, engine: CallEngine) {
    while let Some(callback) = callbacks.recv().await {
        if callback.path.is_empty() {
            engine.handle_push(callback);
        } else {
            engine.inner.route(callback);
        }
    }
}

impl CallEngine {
    fn handle_push(&self, callback: TrouterCallback) {
        self.inner.replier.reply(callback.request_id, 200, "");
        if !self.inner.config.ringable {
            return;
        }
        let Ok(PushEvent::IncomingCall(notification)) = decode_push(&callback.body) else {
            return;
        };
        let notification = *notification;
        if self.inner.already_seen(&notification) {
            return;
        }
        let ring_id = self.inner.next_ring_id.fetch_add(1, Ordering::SeqCst);
        let (commands, receiver) = mpsc::unbounded_channel();
        self.inner.rings.lock().expect("rings lock").insert(ring_id, commands);
        tokio::spawn(ring_task(self.clone(), notification, ring_id, receiver));
    }
}

async fn ring_task(
    engine: CallEngine,
    notification: CallNotification,
    ring_id: u64,
    mut commands: mpsc::UnboundedReceiver<RingCommand>,
) {
    let inner = engine.inner.clone();
    let links = inner.callback_links();
    let route_id = links.call_id().to_owned();
    let mut callbacks = inner.register_route(&route_id);
    let timeline = Timeline::new(inner.config.trace);
    let signaling = inner.signaling(timeline.clone());
    let participant_id = if notification.participant_id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        notification.participant_id.clone()
    };
    let participant = inner.participant(&participant_id);
    let cleanup = |inner: &Inner| {
        inner.unregister_route(&route_id);
        inner.rings.lock().expect("rings lock").remove(&ring_id);
    };
    let Some(attach_url) = notification.links.get("attach") else {
        cleanup(&inner);
        return;
    };
    let request = AttachRequest {
        from: &participant,
        callbacks: &links,
        controller: notification.conversation_controller.as_deref(),
        needs_media: notification.offer_sdp.is_none(),
    };
    let mut attached = match signaling.attach(attach_url, &request).await {
        Ok(attached) => attached,
        Err(error) => {
            timeline.record("attach failed", error.to_string());
            cleanup(&inner);
            return;
        }
    };
    if attached.offer_sdp.is_none() {
        attached.offer_sdp = notification.offer_sdp.clone();
    }
    if let Some(progress) = attached.links.get("progress")
        && let Err(error) = signaling.send_ringing(progress, &participant).await
    {
        timeline.record("ringing progress failed", error.to_string());
    }
    inner.emit(EngineEvent::Incoming(IncomingRing {
        ring_id,
        caller: notification.caller.clone(),
        thread_id: notification.thread_id.clone(),
        video: notification.video,
        is_group: notification.is_multi_party,
        subject: notification.subject.clone(),
    }));
    loop {
        tokio::select! {
            callback = callbacks.recv() => {
                let Some(callback) = callback else { break };
                inner.replier.reply(callback.request_id, 200, "");
                let event = decode_body(&callback).and_then(|body| classify(&callback.path, body));
                if let Ok((_, CallEvent::End(end))) = event {
                    inner.emit(EngineEvent::RingEnded {
                        ring_id,
                        kind: end.kind(),
                        answered_by: end.accepted_elsewhere_by,
                    });
                    break;
                }
            }
            command = commands.recv() => match command {
                Some(RingCommand::Accept(answer)) => {
                    let (handle, control) = call_channel();
                    let incoming = IncomingCall {
                        notification,
                        attached,
                        links,
                        participant,
                        callbacks,
                    };
                    let _ = answer.send(Some(handle));
                    inner.rings.lock().expect("rings lock").remove(&ring_id);
                    let options = inner.call_options();
                    tokio::spawn(async move {
                        let _ = engine.run(Plan::Incoming(Box::new(incoming)), options, control).await;
                    });
                    return;
                }
                Some(RingCommand::Decline) => {
                    if let Some(call_leg) = attached.links.get("callLeg")
                        && let Err(error) = signaling.decline(call_leg).await
                    {
                        timeline.record("decline failed", error.to_string());
                    }
                    break;
                }
                Some(RingCommand::Drop) | None => break,
            }
        }
    }
    cleanup(&inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_base_comes_from_the_presence_uri() {
        assert_eq!(
            callback_base("https://pub-ent-euwe-02-f.trouter.teams.microsoft.com:3443/v4/f/abc//unifiedPresenceService"),
            "https://pub-ent-euwe-02-f.trouter.teams.microsoft.com:3443/v4/f/abc/"
        );
        assert_eq!(callback_base("https://h/v4/f/abc/unifiedPresenceService"), "https://h/v4/f/abc/");
    }

    #[test]
    fn only_the_ringable_engine_registers_for_calls() {
        assert!(calling_realtime_config(true, calling_instance_names()).ringable);
        assert!(!calling_realtime_config(false, calling_instance_names()).ringable);
        assert!(calling_realtime_config(true, calling_instance_names()).forward_callbacks);
    }
}
