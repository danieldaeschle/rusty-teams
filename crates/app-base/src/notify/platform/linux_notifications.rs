//! Desktop notifications on Linux through `org.freedesktop.Notifications`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;
use std::time::Duration;

use futures_util::StreamExt;
use zbus::Connection;
use zbus::zvariant::{OwnedValue, Value};

use crate::APP_NAME;
use crate::linux_desktop::DESKTOP_ID;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";

static MISSING_DAEMON_LOGGED: AtomicBool = AtomicBool::new(false);

/// RGBA image attached as the freedesktop `image-data` hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One notification the session bus should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusNotification {
    pub logical_key: String,
    pub replace_key: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub timeout_ms: i32,
    pub urgency: u8,
    pub category: String,
    pub image: Option<NotificationImage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationEvent {
    /// Default activation or a named action (not inline-reply text).
    Action {
        logical_key: String,
        action_id: String,
    },
    /// Inline reply text from the shell.
    Reply {
        logical_key: String,
        text: String,
    },
    /// User or shell closed the notification (reason 2 = dismissed by user).
    Closed {
        logical_key: String,
        reason: u32,
    },
}

enum Command {
    Sync {
        visible: Vec<BusNotification>,
        close: Vec<String>,
    },
    Shutdown,
}

struct Published {
    dbus_id: u32,
    logical_key: String,
    fingerprint: u64,
}

/// Linux desktop notification client. Poll from the UI tick like the tray.
pub struct DesktopNotifications {
    commands: Sender<Command>,
    events: Mutex<Receiver<NotificationEvent>>,
}

impl DesktopNotifications {
    pub fn start() -> Option<Self> {
        let (command_tx, command_rx) = channel();
        let (event_tx, event_rx) = channel();
        let (ready_tx, ready_rx) = channel();
        thread::Builder::new()
            .name("linux-notifications".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                runtime.block_on(worker(command_rx, event_tx, ready_tx));
            })
            .ok()?;
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Some(Self {
                commands: command_tx,
                events: Mutex::new(event_rx),
            }),
            Ok(Err(error)) => {
                log_missing_once(&error);
                None
            }
            Err(_) => {
                log_missing_once("notification worker did not start");
                None
            }
        }
    }

    pub fn sync(&self, visible: Vec<BusNotification>, close: Vec<String>) {
        let _ = self.commands.send(Command::Sync { visible, close });
    }

    pub fn poll(&self) -> Vec<NotificationEvent> {
        let Ok(receiver) = self.events.lock() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        loop {
            match receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        events
    }
}

impl Drop for DesktopNotifications {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

fn log_missing_once(detail: &str) {
    if MISSING_DAEMON_LOGGED
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        log::warn!("desktop notifications unavailable: {detail}");
    }
}

fn fingerprint(note: &BusNotification) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    note.logical_key.hash(&mut hasher);
    note.replace_key.hash(&mut hasher);
    note.summary.hash(&mut hasher);
    note.body.hash(&mut hasher);
    note.actions.hash(&mut hasher);
    note.timeout_ms.hash(&mut hasher);
    note.urgency.hash(&mut hasher);
    note.category.hash(&mut hasher);
    note.image
        .as_ref()
        .map(|image| (image.width, image.height, image.rgba.len()))
        .hash(&mut hasher);
    hasher.finish()
}

async fn worker(
    commands: Receiver<Command>,
    events: Sender<NotificationEvent>,
    ready: Sender<Result<(), String>>,
) {
    let connection = match Connection::session().await {
        Ok(connection) => connection,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let caps = match get_capabilities(&connection).await {
        Ok(caps) => caps,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));

    let mut by_replace: HashMap<String, Published> = HashMap::new();
    let mut by_dbus: HashMap<u32, String> = HashMap::new();
    let mut supports_inline_reply = caps.iter().any(|cap| cap == "inline-reply");

    let mut action_stream = match receive_signal(&connection, "ActionInvoked").await {
        Ok(stream) => stream,
        Err(error) => {
            log::warn!("notifications ActionInvoked stream failed: {error}");
            return;
        }
    };
    let mut replied_stream = receive_signal(&connection, "NotificationReplied")
        .await
        .ok();
    let mut closed_stream = receive_signal(&connection, "NotificationClosed")
        .await
        .ok();

    loop {
        tokio::select! {
            biased;
            message = action_stream.next() => {
                let Some(message) = message else { break };
                if let Ok(message) = message
                    && let Some(event) = parse_action_invoked(&message, &by_dbus)
                {
                    let _ = events.send(event);
                }
            }
            message = async {
                match replied_stream.as_mut() {
                    Some(stream) => stream.next().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(Ok(message)) = message
                    && let Some(event) = parse_replied(&message, &by_dbus)
                {
                    let _ = events.send(event);
                }
            }
            message = async {
                match closed_stream.as_mut() {
                    Some(stream) => stream.next().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(Ok(message)) = message
                    && let Some(event) = parse_closed(&message, &by_dbus)
                {
                    let _ = events.send(event);
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(40)) => {
                match commands.try_recv() {
                    Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                    Ok(Command::Sync { visible, close }) => {
                        if let Ok(caps) = get_capabilities(&connection).await {
                            supports_inline_reply = caps.iter().any(|cap| cap == "inline-reply");
                        }
                        apply_sync(
                            &connection,
                            &mut by_replace,
                            &mut by_dbus,
                            visible,
                            close,
                            supports_inline_reply,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

async fn apply_sync(
    connection: &Connection,
    by_replace: &mut HashMap<String, Published>,
    by_dbus: &mut HashMap<u32, String>,
    visible: Vec<BusNotification>,
    close: Vec<String>,
    supports_inline_reply: bool,
) {
    for logical in close {
        if let Some((replace_key, dbus_id)) = by_replace
            .iter()
            .find(|(_, published)| published.logical_key == logical)
            .map(|(key, published)| (key.clone(), published.dbus_id))
        {
            let _ = close_notification(connection, dbus_id).await;
            by_dbus.remove(&dbus_id);
            by_replace.remove(&replace_key);
        }
    }
    let visible_keys: std::collections::HashSet<_> =
        visible.iter().map(|note| note.replace_key.clone()).collect();
    by_replace.retain(|key, published| {
        if visible_keys.contains(key) {
            true
        } else {
            by_dbus.remove(&published.dbus_id);
            false
        }
    });
    for note in visible {
        let print = fingerprint(&note);
        if let Some(existing) = by_replace.get(&note.replace_key)
            && existing.fingerprint == print
            && existing.logical_key == note.logical_key
        {
            continue;
        }
        let replaces = by_replace
            .get(&note.replace_key)
            .map(|published| published.dbus_id)
            .unwrap_or(0);
        match notify(connection, &note, replaces, supports_inline_reply).await {
            Ok(dbus_id) => {
                if let Some(previous) = by_replace.insert(
                    note.replace_key.clone(),
                    Published {
                        dbus_id,
                        logical_key: note.logical_key.clone(),
                        fingerprint: print,
                    },
                ) {
                    by_dbus.remove(&previous.dbus_id);
                }
                by_dbus.insert(dbus_id, note.logical_key);
            }
            Err(error) => log::warn!("Notify failed: {error}"),
        }
    }
}

async fn get_capabilities(connection: &Connection) -> Result<Vec<String>, String> {
    let reply = connection
        .call_method(
            Some(SERVICE),
            PATH,
            Some(INTERFACE),
            "GetCapabilities",
            &(),
        )
        .await
        .map_err(|error| error.to_string())?;
    reply.body().deserialize().map_err(|error| error.to_string())
}

async fn close_notification(connection: &Connection, id: u32) -> Result<(), String> {
    connection
        .call_method(
            Some(SERVICE),
            PATH,
            Some(INTERFACE),
            "CloseNotification",
            &id,
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn notify(
    connection: &Connection,
    note: &BusNotification,
    replaces_id: u32,
    supports_inline_reply: bool,
) -> Result<u32, String> {
    let mut actions: Vec<String> = Vec::new();
    actions.push("default".into());
    actions.push(String::new());
    for (id, label) in &note.actions {
        let id = if id == "inline-reply" && !supports_inline_reply {
            // Shell has no inline reply field; treat as open-chat.
            "reply-open"
        } else {
            id.as_str()
        };
        actions.push(id.to_owned());
        actions.push(label.clone());
    }
    let action_refs: Vec<&str> = actions.iter().map(String::as_str).collect();

    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("urgency", Value::U8(note.urgency));
    hints.insert("category", Value::Str(note.category.as_str().into()));
    hints.insert("desktop-entry", Value::Str(DESKTOP_ID.into()));
    hints.insert("suppress-sound", Value::Bool(true));

    if let Some(image) = &note.image {
        let rowstride = image.width * 4;
        let value = Value::from((
            image.width as i32,
            image.height as i32,
            rowstride as i32,
            true,
            8_i32,
            4_i32,
            image.rgba.clone(),
        ));
        hints.insert("image-data", value);
    }

    let reply = connection
        .call_method(
            Some(SERVICE),
            PATH,
            Some(INTERFACE),
            "Notify",
            &(
                APP_NAME,
                replaces_id,
                "",
                note.summary.as_str(),
                note.body.as_str(),
                action_refs,
                hints,
                note.timeout_ms,
            ),
        )
        .await
        .map_err(|error| error.to_string())?;
    reply.body().deserialize().map_err(|error| error.to_string())
}

async fn receive_signal(
    connection: &Connection,
    member: &str,
) -> Result<zbus::MessageStream, String> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(INTERFACE)
        .map_err(|error| error.to_string())?
        .member(member)
        .map_err(|error| error.to_string())?
        .path(PATH)
        .map_err(|error| error.to_string())?
        .build();
    zbus::MessageStream::for_match_rule(rule, connection, Some(32))
        .await
        .map_err(|error| error.to_string())
}

fn parse_action_invoked(
    message: &zbus::Message,
    by_dbus: &HashMap<u32, String>,
) -> Option<NotificationEvent> {
    let (id, action_key): (u32, String) = message.body().deserialize().ok()?;
    let logical_key = by_dbus.get(&id)?.clone();
    if action_key == "inline-reply" {
        return None;
    }
    Some(NotificationEvent::Action {
        logical_key,
        action_id: if action_key.is_empty() {
            "default".into()
        } else {
            action_key
        },
    })
}

fn parse_replied(
    message: &zbus::Message,
    by_dbus: &HashMap<u32, String>,
) -> Option<NotificationEvent> {
    let (id, text): (u32, String) = message.body().deserialize().ok()?;
    let logical_key = by_dbus.get(&id)?.clone();
    Some(NotificationEvent::Reply { logical_key, text })
}

fn parse_closed(
    message: &zbus::Message,
    by_dbus: &HashMap<u32, String>,
) -> Option<NotificationEvent> {
    let (id, reason): (u32, u32) = message.body().deserialize().ok()?;
    let logical_key = by_dbus.get(&id)?.clone();
    Some(NotificationEvent::Closed {
        logical_key,
        reason,
    })
}

fn inhibit_runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            // Multi-thread so `block_on` is safe from the UI thread and tests.
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
}

/// Reads the optional `Inhibited` property (Plasma / modern GNOME). Missing → false.
pub fn notifications_inhibited() -> bool {
    let Some(runtime) = inhibit_runtime() else {
        return false;
    };
    runtime.block_on(async {
        let Ok(connection) = Connection::session().await else {
            return false;
        };
        let reply = connection
            .call_method(
                Some(SERVICE),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(INTERFACE, "Inhibited"),
            )
            .await;
        let Ok(reply) = reply else {
            return false;
        };
        let Ok(value) = reply.body().deserialize::<OwnedValue>() else {
            return false;
        };
        bool::try_from(value).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_changes_with_body() {
        let mut note = BusNotification {
            logical_key: "message:1".into(),
            replace_key: "conversation:a".into(),
            summary: "Priya".into(),
            body: "one".into(),
            actions: vec![("mark-read".into(), "Mark as read".into())],
            timeout_ms: 6000,
            urgency: 1,
            category: "im.received".into(),
            image: None,
        };
        let first = fingerprint(&note);
        note.body = "two".into();
        assert_ne!(first, fingerprint(&note));
    }

    #[test]
    fn missing_inhibited_property_is_not_quiet() {
        let _ = notifications_inhibited();
    }

    #[test]
    fn log_missing_once_is_idempotent() {
        MISSING_DAEMON_LOGGED.store(false, Ordering::Relaxed);
        log_missing_once("test");
        log_missing_once("test again");
        assert!(MISSING_DAEMON_LOGGED.load(Ordering::Relaxed));
    }
}
