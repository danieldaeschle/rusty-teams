//! One WebDriver BiDi session for the whole app. Firefox allows a single session per browser and
//! keeps it after the socket closes, so every caller shares this connection.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use crate::error::BrowserError;
use crate::targets::{self, Tab};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_BUFFER: usize = 1024;
/// Subscribed once per session; tab channels filter by context.
const EVENTS: [&str; 2] = ["script.message", "browsingContext.load"];

struct Call {
    method: String,
    params: Value,
    reply: oneshot::Sender<Result<Value, BrowserError>>,
}

/// A BiDi event as `{method, params}`.
pub type Event = Value;

#[derive(Clone)]
pub struct Bidi {
    port: u16,
    connection: Arc<Mutex<Option<Connection>>>,
}

struct Connection {
    commands: mpsc::UnboundedSender<Call>,
    /// Kept only to hand out receivers. The pump owns the sender, so receivers close with the socket.
    events: broadcast::Receiver<Event>,
}

/// A top-level browsing context, the BiDi word for a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub id: String,
    pub url: String,
}

impl Bidi {
    pub fn new(port: u16) -> Bidi {
        Bidi {
            port,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    /// Events of every tab on the current connection, from the moment of the call. The stream
    /// ends when the connection does, for example when the browser restarts.
    pub async fn events(&self) -> Result<broadcast::Receiver<Event>, BrowserError> {
        self.connected(|connection| connection.events.resubscribe())
            .await
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, BrowserError> {
        let commands = self
            .connected(|connection| connection.commands.clone())
            .await?;
        send(&commands, method, params).await
    }

    /// Opens the session if there is none. [`BrowserError::SessionTaken`] when a session another
    /// process left behind blocks it: only a browser restart frees that.
    pub async fn connect(&self) -> Result<(), BrowserError> {
        self.connected(|_| ()).await
    }

    async fn connected<T>(&self, read: impl FnOnce(&Connection) -> T) -> Result<T, BrowserError> {
        let mut connection = self.connection.lock().await;
        if let Some(open) = connection
            .as_ref()
            .filter(|open| !open.commands.is_closed())
        {
            return Ok(read(open));
        }
        *connection = None;
        let url = format!("ws://127.0.0.1:{}/session", self.port);
        let (stream, _) =
            tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(&url))
                .await
                .map_err(|_| BrowserError::NoBrowser(format!("{url}: connect timed out")))?
                .map_err(|error| BrowserError::NoBrowser(format!("{url}: {error}")))?;
        let (commands, receiver) = mpsc::unbounded_channel();
        let (events_sender, events) = broadcast::channel(EVENT_BUFFER);
        tokio::spawn(pump(stream, receiver, events_sender));
        let opened = tokio::time::timeout(
            CONNECT_TIMEOUT,
            send(&commands, "session.new", json!({"capabilities": {}})),
        )
        .await
        .map_err(|_| BrowserError::Cdp("session.new timed out".into()))?;
        match opened {
            Err(BrowserError::Cdp(text)) if text.contains("session not created") => {
                return Err(BrowserError::SessionTaken(self.port));
            }
            outcome => outcome?,
        };
        send(&commands, "session.subscribe", json!({"events": EVENTS})).await?;
        let open = Connection { commands, events };
        let value = read(&open);
        *connection = Some(open);
        Ok(value)
    }

    pub async fn contexts(&self) -> Result<Vec<Context>, BrowserError> {
        let tree = self
            .call("browsingContext.getTree", json!({"maxDepth": 0}))
            .await?;
        Ok(contexts_of(&tree))
    }

    pub async fn tabs(&self) -> Result<Vec<Tab>, BrowserError> {
        Ok(self
            .contexts()
            .await?
            .into_iter()
            .map(|context| targets::tab_from_url(context.id, &context.url))
            .collect())
    }

    /// Evaluates `expression` in the tab and answers its value as JSON.
    pub async fn evaluate(&self, context: &str, expression: &str) -> Result<Value, BrowserError> {
        let params = json!({
            "expression": expression,
            "target": {"context": context},
            "awaitPromise": true,
            "resultOwnership": "none",
        });
        let result = self.call("script.evaluate", params).await?;
        evaluation_value(&result).map_err(BrowserError::Cdp)
    }

    pub async fn navigate(&self, context: &str, url: &str) -> Result<(), BrowserError> {
        let params = json!({"context": context, "url": url, "wait": "none"});
        self.call("browsingContext.navigate", params)
            .await
            .map(drop)
    }

    pub async fn open_tab(&self, url: &str) -> Result<(), BrowserError> {
        let created = self
            .call("browsingContext.create", json!({"type": "tab"}))
            .await?;
        let context = created["context"].as_str().unwrap_or_default().to_owned();
        self.navigate(&context, url).await
    }

    pub async fn close_tab(&self, context: &str) -> Result<(), BrowserError> {
        self.call("browsingContext.close", json!({"context": context}))
            .await
            .map(drop)
    }
}

async fn send(
    commands: &mpsc::UnboundedSender<Call>,
    method: &str,
    params: Value,
) -> Result<Value, BrowserError> {
    let (reply, answer) = oneshot::channel();
    let closed = || BrowserError::Cdp(format!("{method}: BiDi connection closed"));
    commands
        .send(Call {
            method: method.to_owned(),
            params,
            reply,
        })
        .map_err(|_| closed())?;
    answer.await.map_err(|_| closed())?
}

async fn pump<S>(
    mut stream: S,
    mut commands: mpsc::UnboundedReceiver<Call>,
    events: broadcast::Sender<Event>,
) where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures_util::Sink<Message>
        + Unpin,
{
    let mut pending: HashMap<u64, (String, oneshot::Sender<Result<Value, BrowserError>>)> =
        HashMap::new();
    let mut next_id: u64 = 1;
    loop {
        tokio::select! {
            call = commands.recv() => {
                let Some(call) = call else { break };
                let id = next_id;
                next_id += 1;
                let frame = json!({"id": id, "method": call.method, "params": call.params});
                if stream.send(Message::text(frame.to_string())).await.is_err() {
                    let _ = call.reply.send(Err(BrowserError::Cdp("BiDi send failed".into())));
                    break;
                }
                pending.insert(id, (call.method, call.reply));
            }
            message = stream.next() => {
                let Some(Ok(message)) = message else { break };
                let Message::Text(text) = message else {
                    if matches!(message, Message::Close(_)) {
                        break;
                    }
                    continue;
                };
                let Ok(frame) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                if let Some(id) = frame.get("id").and_then(Value::as_u64) {
                    if let Some((method, reply)) = pending.remove(&id) {
                        let _ = reply.send(answer_of(&method, &frame));
                    }
                } else if frame["type"] == "event" {
                    let _ = events.send(json!({"method": frame["method"], "params": frame["params"]}));
                }
            }
        }
    }
    for (method, reply) in pending.into_values() {
        let _ = reply.send(Err(BrowserError::Cdp(format!(
            "{method}: BiDi connection closed"
        ))));
    }
}

fn answer_of(method: &str, frame: &Value) -> Result<Value, BrowserError> {
    if frame["type"] == "error" {
        let error = frame["error"].as_str().unwrap_or("error");
        let message = frame["message"].as_str().unwrap_or_default();
        return Err(BrowserError::Cdp(format!("{method}: {error}: {message}")));
    }
    Ok(frame.get("result").cloned().unwrap_or(Value::Null))
}

pub(crate) fn contexts_of(tree: &Value) -> Vec<Context> {
    tree["contexts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|context| {
            Some(Context {
                id: context["context"].as_str()?.to_owned(),
                url: context["url"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// The value of a `script.evaluate` result, or the exception text.
pub fn evaluation_value(result: &Value) -> Result<Value, String> {
    if result["type"] == "exception" {
        let text = result
            .pointer("/exceptionDetails/text")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(text.to_owned());
    }
    Ok(remote_value_to_json(&result["result"]))
}

/// Turns a BiDi remote value into the JSON CDP's `returnByValue` would give. Values JSON cannot
/// hold (NaN, functions, nodes) become null.
pub fn remote_value_to_json(remote: &Value) -> Value {
    let value = &remote["value"];
    match remote["type"].as_str().unwrap_or_default() {
        "string" | "boolean" => value.clone(),
        "number" if value.is_number() => value.clone(),
        "number" if value == "-0" => json!(0),
        "bigint" | "date" => value.clone(),
        "array" | "set" => Value::Array(
            value
                .as_array()
                .into_iter()
                .flatten()
                .map(remote_value_to_json)
                .collect(),
        ),
        "object" | "map" => Value::Object(
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|pair| {
                    let key = match &pair[0] {
                        Value::String(key) => key.clone(),
                        other => remote_value_to_json(other).as_str()?.to_owned(),
                    };
                    Some((key, remote_value_to_json(&pair[1])))
                })
                .collect::<Map<String, Value>>(),
        ),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_values_become_plain_json() {
        let remote = json!({"type": "object", "value": [
            ["a", {"type": "array", "value": [
                {"type": "number", "value": 1},
                {"type": "number", "value": "NaN"},
                {"type": "undefined"},
                {"type": "string", "value": "x"},
            ]}],
            ["b", {"type": "object", "value": [["c", {"type": "boolean", "value": true}]]}],
            ["d", {"type": "null"}],
            ["e", {"type": "function"}],
        ]});
        assert_eq!(
            remote_value_to_json(&remote),
            json!({"a": [1, null, null, "x"], "b": {"c": true}, "d": null, "e": null})
        );
    }

    #[test]
    fn evaluation_answers_value_or_exception_text() {
        let success = json!({"type": "success", "result": {"type": "number", "value": 2}});
        assert_eq!(evaluation_value(&success), Ok(json!(2)));
        let exception = json!({"type": "exception", "exceptionDetails": {"text": "Error: boom"}});
        assert_eq!(evaluation_value(&exception), Err("Error: boom".to_owned()));
    }

    #[test]
    fn reads_top_level_contexts() {
        let tree = json!({"contexts": [
            {"context": "a", "url": "about:blank", "children": []},
            {"context": "b", "url": "https://teams.cloud.microsoft/", "children": []},
        ]});
        assert_eq!(
            contexts_of(&tree),
            [
                Context {
                    id: "a".into(),
                    url: "about:blank".into()
                },
                Context {
                    id: "b".into(),
                    url: "https://teams.cloud.microsoft/".into()
                },
            ]
        );
    }
}
