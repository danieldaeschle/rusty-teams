//! A [`session::Transport`] for Firefox. It answers the small CDP subset the session layer and the
//! realtime worker send (evaluate, bindings, injected scripts, navigate, reload) over WebDriver BiDi,
//! so everything above the transport stays protocol-agnostic.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use session::{
    App, BoxFuture, Diagnosis, Error, OpenedTab, Result, TabChannel, TabCommand, TabEvent,
    Transport, is_login_url,
};
use tokio::sync::{broadcast, mpsc};

use crate::bidi::{Bidi, Context, Event, evaluation_value};
use crate::browser::Browser;
use crate::error::BrowserError;

const ENDPOINT_NAME: &str = "WebDriver BiDi";

static NEXT_CHANNEL: AtomicU64 = AtomicU64::new(1);

/// The transport that fits the browser: BiDi for Firefox, the CDP endpoint otherwise.
pub fn session_transport(browser: &Browser, endpoint: &str) -> Arc<dyn Transport> {
    match browser.bidi() {
        Some(bidi) => Arc::new(BidiTransport::new(bidi.clone())),
        None => Arc::new(session::CdpTransport::new(endpoint)),
    }
}

pub struct BidiTransport {
    bidi: Bidi,
}

impl BidiTransport {
    pub fn new(bidi: Bidi) -> Self {
        BidiTransport { bidi }
    }

    async fn contexts(&self) -> Result<Vec<Context>> {
        self.bidi.contexts().await.map_err(no_browser)
    }

    async fn app_context(&self, app: App) -> Result<Option<Context>> {
        Ok(self
            .contexts()
            .await?
            .into_iter()
            .find(|context| is_app_url(app, &context.url)))
    }
}

fn is_app_url(app: App, url: &str) -> bool {
    app.origins().iter().any(|origin| url.starts_with(origin))
}

fn no_browser(error: BrowserError) -> Error {
    Error::NoBrowser {
        endpoint: ENDPOINT_NAME.into(),
        reason: error.to_string(),
    }
}

fn protocol(error: BrowserError) -> Error {
    Error::Cdp(error.to_string())
}

impl Transport for BidiTransport {
    fn supports_cdp(&self) -> bool {
        false
    }

    fn check(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.contexts().await.map(drop) })
    }

    fn open(&self, app: App, with_events: bool) -> BoxFuture<'_, Result<Option<OpenedTab>>> {
        Box::pin(async move {
            let Some(context) = self.app_context(app).await? else {
                return Ok(None);
            };
            let events = match with_events {
                true => Some(self.bidi.events().await.map_err(no_browser)?),
                false => None,
            };
            let channel = TabChannel::new(with_events);
            tokio::spawn(serve(
                self.bidi.clone(),
                context.id,
                channel.commands,
                events,
                channel.event_sink,
            ));
            Ok(Some((channel.control, channel.events)))
        })
    }

    fn wake(&self, app: App) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let Some(context) = self.app_context(app).await? else {
                return Ok(());
            };
            if app.is_park_url(&context.url) {
                self.bidi.navigate(&context.id, app.start_url()).await
            } else {
                let params = json!({"context": context.id, "wait": "none"});
                self.bidi
                    .call("browsingContext.reload", params)
                    .await
                    .map(drop)
            }
            .map_err(protocol)
        })
    }

    fn diagnose(&self) -> BoxFuture<'_, Result<Diagnosis>> {
        Box::pin(async move {
            let contexts = self.contexts().await?;
            Ok(Diagnosis {
                login_pending: contexts.iter().any(|context| is_login_url(&context.url)),
                app_tab_open: App::ALL
                    .iter()
                    .any(|&app| contexts.iter().any(|context| is_app_url(app, &context.url))),
            })
        })
    }
}

/// What one tab channel registered, so it can map messages back and clean up after itself:
/// unlike CDP, BiDi keeps preload scripts after the channel is gone.
#[derive(Default)]
struct ChannelState {
    /// BiDi channel id to the CDP binding name.
    bindings: HashMap<String, String>,
    scripts: HashSet<String>,
}

type Shared = Arc<Mutex<ChannelState>>;

async fn serve(
    bidi: Bidi,
    context: String,
    mut commands: mpsc::UnboundedReceiver<TabCommand>,
    mut events: Option<broadcast::Receiver<Event>>,
    sink: Option<mpsc::UnboundedSender<TabEvent>>,
) {
    let state = Shared::default();
    let mut commands_open = true;
    loop {
        tokio::select! {
            command = commands.recv(), if commands_open => match command {
                Some(command) => {
                    let (bidi, context, state) = (bidi.clone(), context.clone(), state.clone());
                    tokio::spawn(async move {
                        let outcome =
                            translate(&bidi, &context, &state, &command.method, &command.params).await;
                        command.respond(outcome);
                    });
                }
                None if sink.is_none() => break,
                None => commands_open = false,
            },
            event = next_event(&mut events) => match event {
                Ok(event) => {
                    let Some(sink) = &sink else { break };
                    let mapped = tab_event(&context, &state.lock().expect("channel state").bindings, &event);
                    if let Some(mapped) = mapped
                        && sink.send(mapped).is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = sink_closed(&sink) => break,
        }
    }
    let scripts = std::mem::take(&mut state.lock().expect("channel state").scripts);
    for script in scripts {
        let _ = bidi
            .call("script.removePreloadScript", json!({"script": script}))
            .await;
    }
}

async fn next_event(
    events: &mut Option<broadcast::Receiver<Event>>,
) -> std::result::Result<Event, broadcast::error::RecvError> {
    match events {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

async fn sink_closed(sink: &Option<mpsc::UnboundedSender<TabEvent>>) {
    match sink {
        Some(sink) => sink.closed().await,
        None => std::future::pending().await,
    }
}

/// A function that installs `name` on the page as a binding that posts to its channel argument.
fn binding_declaration(name: &str) -> String {
    let name = Value::from(name);
    format!("(send) => {{ globalThis[{name}] = (payload) => send(String(payload)); }}")
}

async fn translate(
    bidi: &Bidi,
    context: &str,
    state: &Shared,
    method: &str,
    params: &Value,
) -> Result<Value> {
    let text = |key: &str| params[key].as_str().unwrap_or_default();
    match method {
        "Runtime.enable" | "Runtime.disable" | "Page.enable" | "Page.disable" => Ok(json!({})),
        "Runtime.evaluate" => {
            let request = json!({
                "expression": text("expression"),
                "target": {"context": context},
                "awaitPromise": params["awaitPromise"].as_bool().unwrap_or(false),
                "resultOwnership": "none",
            });
            let result = bidi
                .call("script.evaluate", request)
                .await
                .map_err(protocol)?;
            Ok(match evaluation_value(&result) {
                Ok(value) => json!({"result": {"value": value}}),
                Err(text) => json!({"exceptionDetails": {"text": text}}),
            })
        }
        "Runtime.addBinding" => {
            let name = text("name");
            let channel = format!("{name}#{}", NEXT_CHANNEL.fetch_add(1, Ordering::Relaxed));
            let declaration = binding_declaration(name);
            let arguments = json!([{"type": "channel", "value": {"channel": channel}}]);
            state
                .lock()
                .expect("channel state")
                .bindings
                .insert(channel, name.to_owned());
            let preload = json!({
                "functionDeclaration": declaration,
                "arguments": arguments,
                "contexts": [context],
            });
            add_preload(bidi, state, preload).await?;
            let now = json!({
                "functionDeclaration": declaration,
                "arguments": arguments,
                "target": {"context": context},
                "awaitPromise": false,
            });
            bidi.call("script.callFunction", now)
                .await
                .map_err(protocol)?;
            Ok(json!({}))
        }
        "Page.addScriptToEvaluateOnNewDocument" => {
            let preload = json!({
                "functionDeclaration": format!("() => {{\n{}\n}}", text("source")),
                "contexts": [context],
            });
            let identifier = add_preload(bidi, state, preload).await?;
            Ok(json!({"identifier": identifier}))
        }
        "Page.removeScriptToEvaluateOnNewDocument" => {
            let script = text("identifier");
            state.lock().expect("channel state").scripts.remove(script);
            bidi.call("script.removePreloadScript", json!({"script": script}))
                .await
                .map(|_| json!({}))
                .map_err(protocol)
        }
        "Page.navigate" => bidi
            .navigate(context, text("url"))
            .await
            .map(|_| json!({}))
            .map_err(protocol),
        "Page.reload" => bidi
            .call(
                "browsingContext.reload",
                json!({"context": context, "wait": "none"}),
            )
            .await
            .map(|_| json!({}))
            .map_err(protocol),
        other => Err(Error::Cdp(format!(
            "{other} is not available over WebDriver BiDi"
        ))),
    }
}

async fn add_preload(bidi: &Bidi, state: &Shared, params: Value) -> Result<String> {
    let added = bidi
        .call("script.addPreloadScript", params)
        .await
        .map_err(protocol)?;
    let script = added["script"].as_str().unwrap_or_default().to_owned();
    state
        .lock()
        .expect("channel state")
        .scripts
        .insert(script.clone());
    Ok(script)
}

/// The CDP event a BiDi event stands for in this tab. Binding calls match by channel, so they also
/// arrive from the tab's frames, as CDP bindings do.
fn tab_event(context: &str, bindings: &HashMap<String, String>, event: &Event) -> Option<TabEvent> {
    let params = &event["params"];
    match event["method"].as_str()? {
        "script.message" => {
            let name = bindings.get(params["channel"].as_str()?)?;
            let payload = params.pointer("/data/value")?.as_str()?;
            Some(TabEvent {
                method: "Runtime.bindingCalled".into(),
                params: json!({"name": name, "payload": payload}),
            })
        }
        "browsingContext.load" if params["context"] == context => Some(TabEvent {
            method: "Page.frameNavigated".into(),
            params: json!({"frame": {"id": context, "url": params["url"]}}),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings() -> HashMap<String, String> {
        HashMap::from([("sink#1".to_owned(), "sink".to_owned())])
    }

    #[test]
    fn channel_messages_become_binding_calls() {
        let message = json!({"method": "script.message", "params": {
            "channel": "sink#1",
            "data": {"type": "string", "value": "{\"a\":1}"},
            "source": {"context": "frame"},
        }});
        let event = tab_event("tab", &bindings(), &message).unwrap();
        assert_eq!(event.binding_payload("sink"), Some("{\"a\":1}"));
        let foreign = json!({"method": "script.message", "params": {
            "channel": "other#2", "data": {"type": "string", "value": "x"},
        }});
        assert!(tab_event("tab", &bindings(), &foreign).is_none());
    }

    #[test]
    fn only_this_tabs_load_is_a_main_frame_navigation() {
        let load = |context: &str| json!({"method": "browsingContext.load", "params": {"context": context, "url": "https://teams.cloud.microsoft/"}});
        let event = tab_event("tab", &bindings(), &load("tab")).unwrap();
        assert_eq!(event.method, "Page.frameNavigated");
        assert!(event.params.pointer("/frame/parentId").is_none());
        assert!(tab_event("tab", &bindings(), &load("other")).is_none());
    }

    #[test]
    fn binding_declaration_quotes_the_name() {
        assert_eq!(
            binding_declaration("a'b"),
            r#"(send) => { globalThis["a'b"] = (payload) => send(String(payload)); }"#
        );
    }
}
