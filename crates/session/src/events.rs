use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::{SinkExt, Stream, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::error::{Error, Result};

const EVALUATE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub struct TabEvent {
    pub method: String,
    pub params: Value,
}

impl TabEvent {
    pub fn binding_payload(&self, binding_name: &str) -> Option<&str> {
        if self.method != "Runtime.bindingCalled" || self.params.get("name")?.as_str()? != binding_name {
            return None;
        }
        self.params.get("payload")?.as_str()
    }
}

pub struct TabCommand {
    pub method: String,
    pub params: Value,
    reply: oneshot::Sender<Result<Value>>,
}

impl TabCommand {
    pub fn respond(self, outcome: Result<Value>) {
        let _ = self.reply.send(outcome);
    }
}

/// Both ends of a tab connection, for a transport that serves the commands itself.
pub struct TabChannel {
    pub control: TabControl,
    pub events: Option<TabEvents>,
    pub commands: mpsc::UnboundedReceiver<TabCommand>,
    pub event_sink: Option<mpsc::UnboundedSender<TabEvent>>,
}

impl TabChannel {
    pub fn new(with_events: bool) -> Self {
        let (commands_sender, commands) = mpsc::unbounded_channel();
        let (event_sink, events) = if with_events {
            let (sender, receiver) = mpsc::unbounded_channel();
            (Some(sender), Some(TabEvents { receiver }))
        } else {
            (None, None)
        };
        TabChannel {
            control: TabControl {
                commands: commands_sender,
            },
            events,
            commands,
            event_sink,
        }
    }
}

#[derive(Clone)]
pub struct TabControl {
    commands: mpsc::UnboundedSender<TabCommand>,
}

pub struct TabEvents {
    receiver: mpsc::UnboundedReceiver<TabEvent>,
}

impl TabEvents {
    pub async fn recv(&mut self) -> Option<TabEvent> {
        self.receiver.recv().await
    }
}

impl Stream for TabEvents {
    type Item = TabEvent;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<TabEvent>> {
        self.receiver.poll_recv(context)
    }
}

impl TabControl {
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let (reply, answer) = oneshot::channel();
        let command = TabCommand {
            method: method.to_owned(),
            params,
            reply,
        };
        self.commands
            .send(command)
            .map_err(|_| Error::Cdp("tab connection closed".into()))?;
        answer.await.map_err(|_| Error::Cdp("tab connection closed".into()))?
    }

    pub async fn enable_events(&self) -> Result<()> {
        self.call("Runtime.enable", json!({})).await?;
        self.call("Page.enable", json!({})).await?;
        Ok(())
    }

    pub async fn add_binding(&self, name: &str) -> Result<()> {
        self.call("Runtime.addBinding", json!({"name": name})).await.map(drop)
    }

    pub async fn evaluate(&self, expression: &str) -> Result<Value> {
        self.evaluate_within(expression, EVALUATE_TIMEOUT).await
    }

    pub async fn evaluate_within(&self, expression: &str, timeout: Duration) -> Result<Value> {
        let params = json!({"expression": expression, "awaitPromise": true, "returnByValue": true});
        let result = tokio::time::timeout(timeout, self.call("Runtime.evaluate", params))
            .await
            .map_err(|_| Error::Cdp("Runtime.evaluate timed out".into()))??;
        if let Some(details) = result.get("exceptionDetails") {
            let text = details
                .pointer("/exception/description")
                .or_else(|| details.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(Error::Cdp(format!("page script failed: {text}")));
        }
        Ok(result.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    /// Runs `source` in every future document of the tab and once in the current one.
    pub async fn inject_script(&self, source: &str) -> Result<String> {
        let added = self
            .call("Page.addScriptToEvaluateOnNewDocument", json!({"source": source}))
            .await?;
        let identifier = added.get("identifier").and_then(Value::as_str).unwrap_or_default().to_owned();
        self.evaluate(source).await?;
        Ok(identifier)
    }

    pub async fn remove_injected_script(&self, identifier: &str) -> Result<()> {
        self.call("Page.removeScriptToEvaluateOnNewDocument", json!({"identifier": identifier}))
            .await
            .map(drop)
    }
}

pub(crate) async fn open(websocket_url: &str, with_events: bool) -> Result<(TabControl, Option<TabEvents>)> {
    let (stream, _) = connect_async(websocket_url)
        .await
        .map_err(|error| Error::Cdp(format!("websocket connect failed: {error}")))?;
    let channel = TabChannel::new(with_events);
    tokio::spawn(pump(stream, channel.commands, channel.event_sink));
    Ok((channel.control, channel.events))
}

async fn events_closed(events: &Option<mpsc::UnboundedSender<TabEvent>>) {
    match events {
        Some(sender) => sender.closed().await,
        None => std::future::pending().await,
    }
}

async fn pump<S>(
    mut stream: S,
    mut commands: mpsc::UnboundedReceiver<TabCommand>,
    events: Option<mpsc::UnboundedSender<TabEvent>>,
)
where
    S: Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures_util::Sink<Message>
        + Unpin,
{
    let mut pending: HashMap<u64, oneshot::Sender<Result<Value>>> = HashMap::new();
    let mut next_id: u64 = 1;
    let mut commands_open = true;
    loop {
        tokio::select! {
            command = commands.recv(), if commands_open => {
                let Some(command) = command else {
                    if events.is_none() {
                        break;
                    }
                    commands_open = false;
                    continue;
                };
                let call_id = next_id;
                next_id += 1;
                let frame = json!({"id": call_id, "method": command.method, "params": command.params});
                if stream.send(Message::text(frame.to_string())).await.is_err() {
                    let _ = command.reply.send(Err(Error::Cdp("websocket send failed".into())));
                    break;
                }
                pending.insert(call_id, command.reply);
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
                if let Some(call_id) = frame.get("id").and_then(Value::as_u64) {
                    if let Some(reply) = pending.remove(&call_id) {
                        let _ = reply.send(answer_of(&frame));
                    }
                } else if let Some(events) = &events
                    && let Some(method) = frame.get("method").and_then(Value::as_str)
                {
                    let event = TabEvent {
                        method: method.to_owned(),
                        params: frame.get("params").cloned().unwrap_or(Value::Null),
                    };
                    if events.send(event).is_err() {
                        break;
                    }
                }
            }
            _ = events_closed(&events) => break,
        }
    }
    for (_, reply) in pending {
        let _ = reply.send(Err(Error::Cdp("websocket closed".into())));
    }
}

fn answer_of(frame: &Value) -> Result<Value> {
    if let Some(failure) = frame.get("error") {
        let reason = failure.get("message").and_then(Value::as_str).unwrap_or("unknown");
        return Err(Error::Cdp(reason.to_owned()));
    }
    Ok(frame.get("result").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_payload_matches_name_only() {
        let event = TabEvent {
            method: "Runtime.bindingCalled".into(),
            params: json!({"name": "sink", "payload": "{}"}),
        };
        assert_eq!(event.binding_payload("sink"), Some("{}"));
        assert_eq!(event.binding_payload("other"), None);
        let navigation = TabEvent {
            method: "Page.frameNavigated".into(),
            params: json!({}),
        };
        assert_eq!(navigation.binding_payload("sink"), None);
    }
}
