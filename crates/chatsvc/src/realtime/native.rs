use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde_json::{Map, Value, json};
use session::{App, Method, Request, Session};
use tokio::net::TcpStream;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::time::{MissedTickBehavior, interval, sleep, sleep_until, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use super::RealtimeConfig;
use super::callback::{CallbackReply, decode_callback};
use super::event::{RealtimeEvent, StatusEvent, StatusKind, decode_payload};
use super::host::ic3_scope;
use super::runner::CallbackPipes;

const REGISTRAR: &str = "https://teams.cloud.microsoft/registrar/prod/V2/registrations";
const UI_VERSION: &str = "1415/26091712213";
const CLIENT_VERSION: &str = "2026.36.01.1";
const PING_EVERY: Duration = Duration::from_secs(30);
const SILENCE_LIMIT: Duration = Duration::from_secs(100);
const REREGISTER_AFTER: Duration = Duration::from_secs(40 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const ACTIVITY_DEBOUNCE: Duration = Duration::from_secs(1);
const ACTIVITY_ACK_TIMEOUT: Duration = Duration::from_secs(10);
const CALLBACK_REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_STEP_TIMEOUT: Duration = Duration::from_secs(10);
const CALLBACK_PREFIX: &str = "callAgent/";
const MAX_PRESENCE_ENTRIES: usize = 1000;
const MAX_PRESENCE_FIELD: usize = 128;
const MAX_SENDER_NAME: usize = 256;
const MAX_RESOURCE_TYPE: usize = 64;
const MAX_LOSS_DETAIL: usize = 200;
const TOKEN_BODY: &str = "return token;";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// One Trouter frame: `type:id:endpoint:data`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Frame<'a> {
    pub kind: char,
    pub data: &'a str,
}

pub(super) fn parse_frame(text: &str) -> Option<Frame<'_>> {
    let mut parts = text.splitn(4, ':');
    let kind = parts.next()?;
    let (Some(_id), Some(_endpoint)) = (parts.next(), parts.next()) else { return None };
    let mut chars = kind.chars();
    let kind = chars.next().filter(|kind| kind.is_ascii_digit() && chars.next().is_none())?;
    Some(Frame { kind, data: parts.next().unwrap_or("") })
}

pub(super) fn socket_url(host: &str, endpoint_id: &str, correlation: &str, now_ms: u128) -> String {
    let client_info = json!({"cv": CLIENT_VERSION, "ua": "TeamsCDL", "hr": "", "v": UI_VERSION}).to_string();
    format!(
        "wss://{host}/v4/c?tc={}&timeout=40&epid={endpoint_id}&ccid=&cor_id={correlation}&con_num={now_ms}_0",
        percent_encode(&client_info)
    )
}

const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

fn percent_encode(text: &str) -> String {
    utf8_percent_encode(text, URI_COMPONENT).to_string()
}

pub(super) fn registration_body(ringable: bool, endpoint_id: &str, surl: &str) -> Value {
    let (app_id, template_key) = if ringable { ("SkypeSpacesWeb", "SkypeSpacesWeb_2.6") } else { ("TeamsCDLWebWorker", "TeamsCDLWebWorker_2.6") };
    let mut description = json!({
        "appId": app_id, "aesKey": "", "languageId": "en-US", "platform": "chrome",
        "templateKey": template_key, "platformUIVersion": UI_VERSION,
    });
    if ringable {
        description["productContext"] = json!("");
    }
    json!({
        "clientDescription": description,
        "registrationId": endpoint_id, "nodeId": "",
        "transports": {"TROUTER": [{"context": "", "path": surl, "ttl": 3600}]},
    })
}

fn header_value(headers: &Value, name: &str) -> Option<String> {
    let headers = headers.as_object()?;
    headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

/// Headers a callback reply echoes back to Trouter.
pub(super) fn echoed_headers(headers: &Value) -> Value {
    let mut echoed = Map::new();
    for name in ["X-Microsoft-Skype-Chain-ID", "trouter-request"] {
        if let Some(value) = header_value(headers, name) {
            echoed.insert(name.to_owned(), Value::String(value));
        }
    }
    if let Some(correlation) = header_value(headers, "MS-CV") {
        echoed.insert("MS-CV".to_owned(), Value::String(format!("{correlation}.0")));
    }
    Value::Object(echoed)
}

fn request_path(url: &str) -> String {
    let Some(rest) = url.strip_prefix("/v4/f/") else { return url.to_owned() };
    match rest.split_once('/') {
        Some((_, path)) => path.to_owned(),
        None => url.to_owned(),
    }
}

fn conversation_of(resource: &Value) -> String {
    let link = ["conversationLink", "to", "id"]
        .iter()
        .find_map(|key| resource.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()))
        .unwrap_or("");
    let raw = match link.find("conversations/") {
        Some(start) => {
            let after = &link[start + "conversations/".len()..];
            after.split([';', '/', '?']).next().unwrap_or("")
        }
        None => link.split(';').next().unwrap_or(""),
    };
    percent_decode(raw).unwrap_or_else(|| raw.to_owned())
}

fn percent_decode(text: &str) -> Option<String> {
    percent_decode_str(text).decode_utf8().ok().map(|decoded| decoded.into_owned())
}

fn classify(resource_type: &str, message_type: &str) -> &'static str {
    match resource_type {
        "NewMessage" => {
            if message_type.starts_with("Control/Typing") || message_type.starts_with("Control/ClearTyping") {
                "typing"
            } else if message_type == "ThreadActivity/MemberConsumptionHorizonUpdate" {
                "read_receipt"
            } else if message_type == "ThreadActivity/PinnedItemsUpdate" {
                "pins_changed"
            } else if message_type.starts_with("ThreadActivity/") {
                "thread_activity"
            } else if message_type.starts_with("Control/") {
                "control"
            } else {
                "new_message"
            }
        }
        "MessageUpdate" => "message_update",
        "ThreadUpdate" => "thread_update",
        _ => "other",
    }
}

fn cut(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn typing_sender(from: &str) -> Option<String> {
    let lower = from.to_ascii_lowercase();
    let start = lower.rfind("/contacts/8:orgid:")? + "/contacts/8:orgid:".len();
    let id = from[start..].trim_end_matches('/');
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|character| character.is_ascii_hexdigit() || character == '-')).then(|| id.to_owned())
}

/// The same message the page worker forwards for a Trouter notification.
pub(super) fn notification_payload(path: &str, body: Option<&Value>, received_at_ms: i64) -> Option<Value> {
    if path == "unifiedPresenceService" {
        let entries: Vec<Value> = body
            .and_then(|body| body.get("presence"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(MAX_PRESENCE_ENTRIES)
            .filter_map(|entry| {
                let text = |pointer: &str| entry.pointer(pointer).and_then(Value::as_str).map(|value| cut(value, MAX_PRESENCE_FIELD));
                let mri = text("/mri")?;
                let availability = text("/presence/availability")?;
                Some(json!({
                    "mri": mri, "availability": availability, "activity": text("/presence/activity"),
                    "outOfOffice": entry.pointer("/presence/calendarData/isOutOfOffice").and_then(Value::as_bool).unwrap_or(false),
                }))
            })
            .collect();
        return (!entries.is_empty()).then(|| json!({"channel": "presence", "entries": entries}));
    }
    let resource = body.and_then(|body| body.get("resource")).filter(|resource| !resource.is_null());
    let resource_type = body
        .and_then(|body| body.get("resourceType"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(if path.is_empty() { "unknown" } else { path });
    let resource_type = cut(resource_type, MAX_RESOURCE_TYPE);
    let message_type = resource.and_then(|resource| resource.get("messagetype")).and_then(Value::as_str).unwrap_or("");
    let event_kind = if path == "messaging" { classify(&resource_type, message_type) } else { "other" };
    let message_like = matches!(event_kind, "new_message" | "message_update");
    let mut payload = json!({
        "channel": "event", "resourceType": resource_type, "eventKind": event_kind, "receivedAt": received_at_ms,
        "conversationId": resource.map(conversation_of).filter(|id| !id.is_empty()),
        "messageId": resource.filter(|_| message_like).and_then(|resource| resource.get("id")).map(|id| match id {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }),
    });
    if event_kind == "typing"
        && let Some(resource) = resource
    {
        payload["typing"] = json!(if message_type.starts_with("Control/ClearTyping") { "clear" } else { "start" });
        payload["senderId"] = json!(resource.get("from").and_then(Value::as_str).and_then(typing_sender));
        payload["senderName"] = json!(resource.get("imdisplayname").and_then(Value::as_str).map(|name| cut(name, MAX_SENDER_NAME)));
    }
    Some(payload)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as i64)
}

fn status_payload(kind: &str, detail: &str) -> String {
    json!({"channel": "status", "kind": kind, "detail": detail}).to_string()
}

struct PendingReply {
    headers: Value,
    deadline: tokio::time::Instant,
}

enum Ended {
    Stopped(Option<oneshot::Sender<()>>),
    Lost { was_ready: bool },
}

/// Trouter over a websocket of its own; only the token lookup and the registrar calls run in the app tab, which may stay parked.
pub(super) struct NativeTrouter {
    pub session: Session,
    pub config: RealtimeConfig,
    pub host: String,
    pub output: mpsc::UnboundedSender<RealtimeEvent>,
    pub callbacks: CallbackPipes,
    pub activity: watch::Receiver<bool>,
    pub resend: Arc<Notify>,
    pub endpoint_id: String,
    force_refresh: bool,
    retry: u32,
    ack_counter: u64,
    surl: String,
    registered_at: Option<Instant>,
}

struct Connection {
    socket: Socket,
    token: String,
    ready: bool,
    last_frame: Instant,
    sent_activity: Option<bool>,
    activity_ack_id: u64,
    activity_due: Option<tokio::time::Instant>,
    activity_resend_due: Option<tokio::time::Instant>,
    pending: HashMap<i64, PendingReply>,
}

fn far_future() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(365 * 24 * 3600)
}

impl NativeTrouter {
    pub(super) fn new(
        session: Session,
        config: RealtimeConfig,
        host: String,
        output: mpsc::UnboundedSender<RealtimeEvent>,
        callbacks: CallbackPipes,
        activity: watch::Receiver<bool>,
        resend: Arc<Notify>,
    ) -> Self {
        NativeTrouter {
            session,
            config,
            host,
            output,
            callbacks,
            activity,
            resend,
            endpoint_id: uuid::Uuid::new_v4().to_string(),
            force_refresh: false,
            retry: 0,
            ack_counter: 0,
            surl: String::new(),
            registered_at: None,
        }
    }

    pub(super) async fn token(&mut self) -> session::Result<String> {
        let value = self.session.run_with_token(App::Teams, &ic3_scope(), TOKEN_BODY, &json!({}), self.force_refresh).await?;
        self.force_refresh = false;
        value.as_str().map(str::to_owned).ok_or_else(|| session::Error::Cdp("token lookup gave no text".into()))
    }

    pub(super) async fn run(mut self, mut stop: oneshot::Receiver<oneshot::Sender<()>>) {
        loop {
            let connected = tokio::select! {
                connected = self.connect() => connected,
                done = &mut stop => Err(done.ok()),
            };
            let ended = match connected {
                Ok(connection) => self.drive(connection, &mut stop).await,
                Err(Some(done)) => Ended::Stopped(Some(done)),
                Err(None) => Ended::Lost { was_ready: false },
            };
            match ended {
                Ended::Stopped(done) => {
                    self.unregister().await;
                    if let Some(done) = done {
                        let _ = done.send(());
                    }
                    return;
                }
                Ended::Lost { was_ready } => {
                    if was_ready {
                        self.forward(&status_payload("disconnected", "closed"));
                    }
                    self.retry += 1;
                    let delay = Duration::from_secs(1u64 << self.retry.min(5)).min(MAX_BACKOFF);
                    tokio::select! {
                        _ = sleep(delay) => {}
                        done = &mut stop => {
                            self.unregister().await;
                            if let Ok(done) = done {
                                let _ = done.send(());
                            }
                            return;
                        }
                        _ = self.output.closed() => return,
                    }
                }
            }
        }
    }

    async fn connect(&mut self) -> Result<Connection, Option<oneshot::Sender<()>>> {
        let token = match self.token().await {
            Ok(token) => token,
            Err(error) => {
                self.forward(&status_payload("disconnected", &format!("token_failed: {}", short(&error.to_string()))));
                return Err(None);
            }
        };
        session::ensure_crypto_provider();
        let url = socket_url(&self.host, &self.endpoint_id, &uuid::Uuid::new_v4().to_string(), now_ms() as u128);
        let Ok(Ok((socket, _))) = timeout(CONNECT_TIMEOUT, connect_async(url.as_str())).await else {
            self.forward(&status_payload("disconnected", "connect_failed"));
            return Err(None);
        };
        Ok(Connection {
            socket,
            token,
            ready: false,
            last_frame: Instant::now(),
            sent_activity: None,
            activity_ack_id: 0,
            activity_due: None,
            activity_resend_due: None,
            pending: HashMap::new(),
        })
    }

    async fn drive(&mut self, mut connection: Connection, stop: &mut oneshot::Receiver<oneshot::Sender<()>>) -> Ended {
        let mut ping = interval(PING_EVERY);
        ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ping.tick().await;
        let connect_deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
        loop {
            let next_reply = connection.pending.values().map(|pending| pending.deadline).min().unwrap_or_else(far_future);
            tokio::select! {
                frame = connection.socket.next() => {
                    let Some(Ok(message)) = frame else { return Ended::Lost { was_ready: connection.ready } };
                    let text = match message {
                        Message::Text(text) => text.to_string(),
                        Message::Close(_) => return Ended::Lost { was_ready: connection.ready },
                        _ => continue,
                    };
                    connection.last_frame = Instant::now();
                    if !self.on_frame(&mut connection, &text).await {
                        let _ = connection.socket.close(None).await;
                        return Ended::Lost { was_ready: connection.ready };
                    }
                }
                _ = sleep_until(connect_deadline), if !connection.ready => {
                    let _ = connection.socket.close(None).await;
                    return Ended::Lost { was_ready: false };
                }
                _ = ping.tick() => {
                    if connection.last_frame.elapsed() > SILENCE_LIMIT {
                        let _ = connection.socket.close(None).await;
                        return Ended::Lost { was_ready: connection.ready };
                    }
                    self.send_event(&mut connection, "ping", json!([])).await;
                    if connection.ready && self.registered_at.is_some_and(|at| at.elapsed() > REREGISTER_AFTER) {
                        let surl = self.surl.clone();
                        if let Err(detail) = self.register(&surl).await {
                            self.forward(&status_payload("error", &detail));
                        }
                    }
                }
                changed = self.activity.changed() => {
                    if changed.is_ok() {
                        let wanted = *self.activity.borrow_and_update();
                        connection.activity_due = (connection.ready && connection.sent_activity != Some(wanted)).then(|| tokio::time::Instant::now() + ACTIVITY_DEBOUNCE);
                    }
                }
                _ = sleep_until(connection.activity_due.unwrap_or_else(far_future)), if connection.activity_due.is_some() => {
                    self.transmit_activity(&mut connection, false).await;
                }
                _ = sleep_until(connection.activity_resend_due.unwrap_or_else(far_future)), if connection.activity_resend_due.is_some() => {
                    self.transmit_activity(&mut connection, true).await;
                }
                _ = self.resend.notified() => self.transmit_activity(&mut connection, false).await,
                Some(reply) = self.callbacks.replies.recv() => self.answer(&mut connection, reply).await,
                _ = sleep_until(next_reply), if !connection.pending.is_empty() => {
                    let now = tokio::time::Instant::now();
                    let due: Vec<i64> = connection.pending.iter().filter(|(_, pending)| pending.deadline <= now).map(|(id, _)| *id).collect();
                    for request_id in due {
                        self.answer(&mut connection, CallbackReply { request_id, status: 200, body: String::new() }).await;
                    }
                }
                done = &mut *stop => {
                    let _ = connection.socket.close(None).await;
                    return Ended::Stopped(done.ok());
                }
                _ = self.output.closed() => {
                    let _ = connection.socket.close(None).await;
                    return Ended::Stopped(None);
                }
            }
        }
    }

    /// Answers whether the connection should stay open.
    async fn on_frame(&mut self, connection: &mut Connection, text: &str) -> bool {
        let Some(frame) = parse_frame(text) else { return true };
        match frame.kind {
            '1' => {
                let authenticate = json!({"name": "user.authenticate", "args": [{"headers": {
                    "X-Ms-Test-User": "False", "Authorization": format!("Bearer {}", connection.token), "X-MS-Migration": "True",
                }}]});
                send(connection, format!("5:::{authenticate}")).await;
            }
            '2' => send(connection, "2::".to_owned()).await,
            '5' => {
                let Ok(event) = serde_json::from_str::<Value>(frame.data) else { return true };
                match event.get("name").and_then(Value::as_str) {
                    Some("trouter.connected") => {
                        let surl = event.pointer("/args/0/surl").and_then(Value::as_str).unwrap_or_default().to_owned();
                        if let Err(detail) = self.register(&surl).await {
                            self.forward(&status_payload("error", &detail));
                            return false;
                        }
                        connection.ready = true;
                        self.retry = 0;
                        self.transmit_activity(connection, false).await;
                        let endpoint = json!({"channel": "endpoint", "endpointId": self.endpoint_id, "trouterUri": format!("{}/unifiedPresenceService", self.surl)});
                        self.forward(&endpoint.to_string());
                        self.forward(&status_payload("connected", ""));
                    }
                    Some("trouter.message_loss") => {
                        let args = event.get("args").cloned().unwrap_or(Value::Null);
                        let dropped: Vec<String> = args
                            .pointer("/0/droppedIndicators")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .map(|indicator| indicator.get("tag").map(|tag| tag.as_str().map_or_else(|| tag.to_string(), str::to_owned)).unwrap_or_default())
                            .collect();
                        self.send_event(connection, "trouter.processed_message_loss", args).await;
                        self.forward(&status_payload("message_loss", &cut(&dropped.join(","), MAX_LOSS_DETAIL)));
                    }
                    _ => {}
                }
            }
            '6' => {
                if frame.data.split('+').next().and_then(|id| id.parse::<u64>().ok()) == Some(connection.activity_ack_id) {
                    connection.activity_resend_due = None;
                }
            }
            '3' => {
                let Ok(request) = serde_json::from_str::<Value>(frame.data) else { return true };
                let Some(request_id) = request.get("id").and_then(Value::as_i64) else { return true };
                let path = request_path(request.get("url").and_then(Value::as_str).unwrap_or(""));
                let raw_body = match request.get("body") {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Null) | None => "null".to_owned(),
                    Some(other) => other.to_string(),
                };
                let headers = request.get("headers").cloned().unwrap_or(Value::Null);
                if self.config.forward_callbacks && path.starts_with(CALLBACK_PREFIX) {
                    connection.pending.insert(request_id, PendingReply { headers: echoed_headers(&headers), deadline: tokio::time::Instant::now() + CALLBACK_REPLY_TIMEOUT });
                    let callback = json!({"channel": "callback", "requestId": request_id, "path": path,
                        "contentEncoding": header_value(&headers, "X-Microsoft-Skype-Content-Encoding"), "body": raw_body});
                    self.forward(&callback.to_string());
                    return true;
                }
                send(connection, format!("3:::{}", json!({"id": request_id, "status": 200, "headers": {}, "body": ""}))).await;
                let body = serde_json::from_str::<Value>(&raw_body).ok();
                if self.config.forward_callbacks && body.as_ref().is_some_and(|body| body.get("evt").is_some_and(Value::is_number)) {
                    let callback = json!({"channel": "callback", "requestId": request_id, "path": "", "contentEncoding": null, "body": raw_body});
                    self.forward(&callback.to_string());
                    return true;
                }
                if let Some(payload) = notification_payload(&path, body.as_ref(), now_ms()) {
                    self.forward(&payload.to_string());
                }
            }
            _ => {}
        }
        true
    }

    async fn answer(&mut self, connection: &mut Connection, reply: CallbackReply) {
        let Some(pending) = connection.pending.remove(&reply.request_id) else { return };
        let frame = json!({"id": reply.request_id, "status": reply.status, "headers": pending.headers, "body": reply.body});
        send(connection, format!("3:::{frame}")).await;
    }

    async fn send_event(&mut self, connection: &mut Connection, name: &str, args: Value) {
        self.ack_counter += 1;
        send(connection, format!("5:{}+::{}", self.ack_counter, json!({"name": name, "args": args}))).await;
    }

    async fn transmit_activity(&mut self, connection: &mut Connection, resend: bool) {
        connection.activity_due = None;
        connection.activity_resend_due = None;
        if !connection.ready {
            return;
        }
        let active = *self.activity.borrow();
        let label = if active { "active" } else { "inactive" };
        self.send_event(connection, "user.activity", json!([{"state": label, "cv": uuid::Uuid::new_v4().to_string()}])).await;
        connection.sent_activity = Some(active);
        connection.activity_ack_id = self.ack_counter;
        if !resend {
            connection.activity_resend_due = Some(tokio::time::Instant::now() + ACTIVITY_ACK_TIMEOUT);
        }
    }

    async fn register(&mut self, surl: &str) -> Result<(), String> {
        if !surl.is_empty() {
            self.surl = surl.to_owned();
        }
        let mut request = Request::with_body(Method::Post, REGISTRAR, registration_body(self.config.ringable, &self.endpoint_id, &self.surl));
        request.headers.extend([("X-MS-Migration".to_owned(), "True".to_owned()), ("Accept".to_owned(), "application/json, text/javascript".to_owned())]);
        match self.session.send(request, &ic3_scope()).await {
            Ok(_) => {
                self.registered_at = Some(Instant::now());
                Ok(())
            }
            Err(session::Error::Api { status, .. }) => {
                if matches!(status, 401 | 403) {
                    self.force_refresh = true;
                }
                Err(format!("registrar_http_{status}"))
            }
            Err(_) => Err("registrar_failed".to_owned()),
        }
    }

    async fn unregister(&self) {
        if self.registered_at.is_none() {
            return;
        }
        let mut request = Request::get(format!("{REGISTRAR}/{}", self.endpoint_id));
        request.method = Method::Delete;
        request.headers.push(("X-MS-Migration".to_owned(), "True".to_owned()));
        let _ = timeout(STOP_STEP_TIMEOUT, self.session.send(request, &ic3_scope())).await;
    }

    fn forward(&mut self, payload: &str) {
        if let Some(callback) = decode_callback(payload) {
            let _ = self.callbacks.output.send(callback);
            return;
        }
        if let Ok(event) = decode_payload(payload) {
            if let RealtimeEvent::Status(StatusEvent { kind: StatusKind::Error, detail }) = &event
                && matches!(detail.as_str(), "registrar_http_401" | "registrar_http_403")
            {
                self.force_refresh = true;
            }
            let _ = self.output.send(event);
        }
    }
}

async fn send(connection: &mut Connection, text: String) {
    let _ = connection.socket.send(Message::Text(text.into())).await;
}

fn short(text: &str) -> String {
    text.chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_split_into_type_and_data() {
        assert_eq!(parse_frame("1::"), Some(Frame { kind: '1', data: "" }));
        assert_eq!(parse_frame("5:3+::{\"a\":1}"), Some(Frame { kind: '5', data: "{\"a\":1}" }));
        assert_eq!(parse_frame("3:::{\"x\":\"a:b\"}"), Some(Frame { kind: '3', data: "{\"x\":\"a:b\"}" }));
        assert_eq!(parse_frame("garbage"), None);
        assert_eq!(parse_frame("12::"), None);
    }

    #[test]
    fn the_socket_url_carries_the_endpoint_and_encoded_client_info() {
        let url = socket_url("go-eu.trouter.teams.microsoft.com", "ep-1", "cor-1", 42);
        assert!(url.starts_with("wss://go-eu.trouter.teams.microsoft.com/v4/c?tc=%7B%22cv%22"), "{url}");
        assert!(url.ends_with("&timeout=40&epid=ep-1&ccid=&cor_id=cor-1&con_num=42_0"), "{url}");
    }

    #[test]
    fn ringable_registration_uses_the_calling_app() {
        let calling = registration_body(true, "ep", "https://surl/");
        assert_eq!(calling["clientDescription"]["appId"], "SkypeSpacesWeb");
        assert_eq!(calling["clientDescription"]["productContext"], "");
        let worker = registration_body(false, "ep", "https://surl/");
        assert_eq!(worker["clientDescription"]["templateKey"], "TeamsCDLWebWorker_2.6");
        assert!(worker["clientDescription"].get("productContext").is_none());
        assert_eq!(worker["transports"]["TROUTER"][0]["path"], "https://surl/");
    }

    #[test]
    fn callback_replies_echo_chain_request_and_correlation() {
        let echoed = echoed_headers(&json!({"x-microsoft-skype-chain-id": "c", "Trouter-Request": "r", "ms-cv": "abc", "Other": "o"}));
        assert_eq!(echoed, json!({"X-Microsoft-Skype-Chain-ID": "c", "trouter-request": "r", "MS-CV": "abc.0"}));
    }

    #[test]
    fn request_paths_drop_the_forwarding_prefix() {
        assert_eq!(request_path("/v4/f/abc123/messaging"), "messaging");
        assert_eq!(request_path("/v4/f/abc/callAgent/x/y"), "callAgent/x/y");
        assert_eq!(request_path("messaging"), "messaging");
    }

    #[test]
    fn a_message_notification_becomes_the_page_event() {
        let body = json!({"resourceType": "NewMessage", "resource": {"messagetype": "RichText/Html", "id": "177",
            "conversationLink": "https://emea.ng.msg.teams.microsoft.com/v1/users/ME/conversations/19%3Aabc%40thread.v2"}});
        let payload = notification_payload("messaging", Some(&body), 5).unwrap();
        assert_eq!(payload["eventKind"], "new_message");
        assert_eq!(payload["conversationId"], "19:abc@thread.v2");
        assert_eq!(payload["messageId"], "177");
        assert!(matches!(decode_payload(&payload.to_string()), Ok(RealtimeEvent::Message(_))));
    }

    #[test]
    fn typing_names_the_sender() {
        let body = json!({"resourceType": "NewMessage", "resource": {"messagetype": "Control/Typing", "imdisplayname": "Ada",
            "from": "https://x/v1/users/ME/contacts/8:orgid:0a1b2c3d-0000-0000-0000-000000000000", "conversationLink": "conversations/19:t@thread.v2"}});
        let payload = notification_payload("messaging", Some(&body), 5).unwrap();
        assert_eq!(payload["eventKind"], "typing");
        assert_eq!(payload["typing"], "start");
        assert_eq!(payload["senderId"], "0a1b2c3d-0000-0000-0000-000000000000");
        assert_eq!(payload["senderName"], "Ada");
        assert!(payload["messageId"].is_null());
    }

    #[test]
    fn presence_keeps_entries_with_an_availability() {
        let body = json!({"presence": [{"mri": "8:orgid:a", "presence": {"availability": "Available", "activity": "Available"}}, {"mri": "8:orgid:b"}]});
        let payload = notification_payload("unifiedPresenceService", Some(&body), 5).unwrap();
        assert_eq!(payload["entries"].as_array().unwrap().len(), 1);
        assert!(notification_payload("unifiedPresenceService", Some(&json!({"presence": []})), 5).is_none());
    }
}
