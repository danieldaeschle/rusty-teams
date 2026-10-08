use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use session::{
    App, BoxFuture, Diagnosis, Error, OpenedTab, Result, TabChannel, TabCommand, TabEvent,
    Transport, is_login_url,
};
use tokio::sync::{
    mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    watch,
};

use crate::dialog::{DialogCommand, DialogEvent, DialogHandle, DialogSpec};

const ENDPOINT_NAME: &str = "WebView2";
const READY_WAIT: Duration = Duration::from_secs(30);

pub(crate) enum UiRequest {
    Call {
        app: App,
        command: TabCommand,
    },
    Subscribe {
        app: App,
        sink: UnboundedSender<TabEvent>,
    },
    Wake {
        app: App,
    },
    Retry,
    OpenDialog {
        id: u64,
        spec: DialogSpec,
        events: UnboundedSender<DialogEvent>,
    },
    Dialog {
        id: u64,
        command: DialogCommand,
    },
}

#[derive(Debug, Clone, Default)]
pub struct HostState {
    pub ready: bool,
    pub failure: Option<String>,
    pub urls: HashMap<App, String>,
}

pub struct WebViewTransport {
    requests: mpsc::Sender<UiRequest>,
    waker: Arc<dyn Fn() + Send + Sync>,
    state: watch::Receiver<HostState>,
    ready_wait: Duration,
    next_dialog: AtomicU64,
}

impl WebViewTransport {
    pub(crate) fn new(
        requests: mpsc::Sender<UiRequest>,
        waker: Arc<dyn Fn() + Send + Sync>,
        state: watch::Receiver<HostState>,
    ) -> Self {
        WebViewTransport {
            requests,
            waker,
            state,
            ready_wait: READY_WAIT,
            next_dialog: AtomicU64::new(1),
        }
    }

    pub fn open_dialog(&self, spec: DialogSpec) -> (DialogHandle, UnboundedReceiver<DialogEvent>) {
        let id = self.next_dialog.fetch_add(1, Ordering::Relaxed);
        let (events, receiver) = unbounded_channel();
        let handle = DialogHandle {
            id,
            requests: self.requests.clone(),
            waker: self.waker.clone(),
        };
        let _ = self.send(UiRequest::OpenDialog { id, spec, events });
        (handle, receiver)
    }

    fn send(&self, request: UiRequest) -> Result<()> {
        self.requests
            .send(request)
            .map_err(|_| no_browser("the webview thread stopped".into()))?;
        (self.waker)();
        Ok(())
    }
}

fn no_browser(reason: String) -> Error {
    Error::NoBrowser {
        endpoint: ENDPOINT_NAME.into(),
        reason,
    }
}

impl Transport for WebViewTransport {
    fn check(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut state = self.state.clone();
            let settled = state.wait_for(|state| state.ready || state.failure.is_some());
            let state = tokio::time::timeout(self.ready_wait, settled)
                .await
                .map_err(|_| no_browser("WebView2 did not start in time".into()))?
                .map_err(|_| no_browser("the webview thread stopped".into()))?;
            match &state.failure {
                Some(reason) if !state.ready => {
                    let error = no_browser(reason.clone());
                    let _ = self.send(UiRequest::Retry);
                    Err(error)
                }
                _ => Ok(()),
            }
        })
    }

    fn open(&self, app: App, with_events: bool) -> BoxFuture<'_, Result<Option<OpenedTab>>> {
        Box::pin(async move {
            if !self.state.borrow().ready {
                return Ok(None);
            }
            let channel = TabChannel::new(with_events);
            if let Some(sink) = channel.event_sink {
                self.send(UiRequest::Subscribe { app, sink })?;
            }
            let requests = self.requests.clone();
            let waker = self.waker.clone();
            let mut commands = channel.commands;
            tokio::spawn(async move {
                while let Some(command) = commands.recv().await {
                    if requests.send(UiRequest::Call { app, command }).is_err() {
                        break;
                    }
                    waker();
                }
            });
            Ok(Some((channel.control, channel.events)))
        })
    }

    fn wake(&self, app: App) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.send(UiRequest::Wake { app }) })
    }

    fn diagnose(&self) -> BoxFuture<'_, Result<Diagnosis>> {
        Box::pin(async move {
            let state = self.state.borrow();
            Ok(Diagnosis {
                login_pending: state.urls.values().any(|url| is_login_url(url)),
                app_tab_open: state.ready,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    struct FakeUi {
        transport: WebViewTransport,
        state: watch::Sender<HostState>,
        requests: mpsc::Receiver<UiRequest>,
    }

    fn fake_ui() -> FakeUi {
        let (sender, requests) = mpsc::channel();
        let (state, receiver) = watch::channel(HostState::default());
        FakeUi {
            transport: WebViewTransport::new(sender, Arc::new(|| {}), receiver),
            state,
            requests,
        }
    }

    fn ready(urls: &[(App, &str)]) -> HostState {
        HostState {
            ready: true,
            failure: None,
            urls: urls
                .iter()
                .map(|&(app, url)| (app, url.to_owned()))
                .collect(),
        }
    }

    #[tokio::test]
    async fn check_waits_for_the_webviews() {
        let ui = fake_ui();
        let state = ui.state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            state.send_replace(ready(&[]));
        });
        ui.transport.check().await.unwrap();
    }

    #[tokio::test]
    async fn check_reports_a_start_failure_as_no_browser() {
        let ui = fake_ui();
        ui.state.send_replace(HostState {
            failure: Some("WebView2 runtime missing".into()),
            ..HostState::default()
        });
        let error = ui.transport.check().await.unwrap_err();
        assert!(matches!(error, Error::NoBrowser { .. }), "{error}");
        assert!(matches!(ui.requests.try_recv(), Ok(UiRequest::Retry)));
    }

    #[tokio::test]
    async fn open_before_ready_has_no_tab() {
        let ui = fake_ui();
        assert!(
            ui.transport
                .open(App::Teams, false)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn calls_reach_the_ui_thread_and_answers_come_back() {
        let ui = fake_ui();
        ui.state.send_replace(ready(&[]));
        let (control, events) = ui
            .transport
            .open(App::Outlook, false)
            .await
            .unwrap()
            .unwrap();
        assert!(events.is_none());
        let requests = ui.requests;
        let answer = tokio::task::spawn_blocking(move || match requests.recv().unwrap() {
            UiRequest::Call { app, command } => {
                assert_eq!(app, App::Outlook);
                assert_eq!(command.method, "Page.reload");
                command.respond(Ok(json!({"done": true})));
            }
            _ => panic!("expected a call"),
        });
        let result: Value = control.call("Page.reload", json!({})).await.unwrap();
        answer.await.unwrap();
        assert_eq!(result, json!({"done": true}));
    }

    #[tokio::test]
    async fn open_with_events_subscribes_first() {
        let ui = fake_ui();
        ui.state.send_replace(ready(&[]));
        let (_control, events) = ui.transport.open(App::Teams, true).await.unwrap().unwrap();
        let mut events = events.unwrap();
        let UiRequest::Subscribe { app, sink } = ui.requests.recv().unwrap() else {
            panic!("expected a subscription");
        };
        assert_eq!(app, App::Teams);
        let event = TabEvent {
            method: "Runtime.bindingCalled".into(),
            params: json!({"name": "sink", "payload": "{}"}),
        };
        sink.send(event.clone()).unwrap();
        assert_eq!(events.recv().await, Some(event));
    }

    #[tokio::test]
    async fn diagnose_sees_a_login_page() {
        let ui = fake_ui();
        ui.state.send_replace(ready(&[
            (
                App::Teams,
                "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            ),
            (App::Outlook, "https://outlook.cloud.microsoft/mail/"),
        ]));
        let diagnosis = ui.transport.diagnose().await.unwrap();
        assert!(diagnosis.login_pending);
        assert!(diagnosis.app_tab_open);
    }
}
