use std::time::Duration;

use serde_json::{Value, json};
use session::{App, Session, TabControl, TabEvents};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{MissedTickBehavior, interval, sleep};

use super::RealtimeConfig;
use super::callback::{CallbackReply, TrouterCallback, decode_callback, reply_expression};
use super::event::{RealtimeEvent, StatusEvent, StatusKind, decode_payload};
use super::host::ic3_scope;

pub(super) const DEFAULT_GLOBAL_NAME: &str = "__chatsvcTrouter";
pub(super) const DEFAULT_BINDING_NAME: &str = "__chatsvcRealtime";
pub(super) const DEFAULT_EPID_KEY: &str = "__chatsvcEpid";
const WORKER_TEMPLATE: &str = include_str!("../../assets/trouter.js");
const FORWARD_CALLBACKS_OFF: &str = "const FORWARD_CALLBACKS = false;";
const STOP_STEP_TIMEOUT: Duration = Duration::from_secs(10);

/// The page worker with this instance's names baked in.
#[derive(Clone)]
pub(super) struct PageScript {
    pub global_name: String,
    pub binding_name: String,
    pub worker_source: String,
}

impl PageScript {
    pub(super) fn new(config: &RealtimeConfig) -> Self {
        let names = &config.instance;
        let quoted = |name: &str| format!("'{name}'");
        let mut worker_source = WORKER_TEMPLATE
            .replace(&quoted(DEFAULT_GLOBAL_NAME), &quoted(&names.global))
            .replace(&quoted(DEFAULT_BINDING_NAME), &quoted(&names.binding))
            .replace(&quoted(DEFAULT_EPID_KEY), &quoted(&names.endpoint_storage_key));
        if config.forward_callbacks {
            worker_source = worker_source.replace(FORWARD_CALLBACKS_OFF, "const FORWARD_CALLBACKS = true;");
        }
        PageScript {
            global_name: names.global.clone(),
            binding_name: names.binding.clone(),
            worker_source,
        }
    }

    fn ensure_body(&self) -> String {
        format!(
            "{}\nreturn window.{}.ensure({{token, host: args.host}});",
            self.worker_source, self.global_name
        )
    }

    fn stop_expression(&self) -> String {
        format!(
            "window.{name} ? window.{name}.stop() : null",
            name = self.global_name
        )
    }
}

pub(super) struct CallbackPipes {
    pub output: mpsc::UnboundedSender<TrouterCallback>,
    pub replies: mpsc::UnboundedReceiver<CallbackReply>,
}

pub(super) struct Attached {
    control: TabControl,
    events: TabEvents,
    script_identifier: String,
    stop_expression: String,
}

pub(super) async fn attach(session: &Session, script: &PageScript) -> crate::error::Result<Attached> {
    let (control, events) = session.subscribe(App::Teams).await?;
    control.enable_events().await?;
    control.add_binding(&script.binding_name).await?;
    let script_identifier = control.inject_script(&script.worker_source).await?;
    Ok(Attached {
        control,
        events,
        script_identifier,
        stop_expression: script.stop_expression(),
    })
}

impl Attached {
    pub(super) async fn shutdown(self) {
        let _ = tokio::time::timeout(
            STOP_STEP_TIMEOUT,
            self.control.evaluate(&self.stop_expression),
        )
        .await;
        let _ = tokio::time::timeout(
            STOP_STEP_TIMEOUT,
            self.control.remove_injected_script(&self.script_identifier),
        )
        .await;
    }
}

enum DriveEnd {
    Stopped(Option<oneshot::Sender<()>>),
    TabLost,
}

pub(super) struct Runner {
    session: Session,
    config: RealtimeConfig,
    script: PageScript,
    callbacks: CallbackPipes,
    host: String,
    output: mpsc::UnboundedSender<RealtimeEvent>,
    force_refresh: bool,
    last_error: Option<String>,
}

impl Runner {
    pub(super) fn new(
        session: Session,
        config: RealtimeConfig,
        script: PageScript,
        host: String,
        output: mpsc::UnboundedSender<RealtimeEvent>,
        callbacks: CallbackPipes,
    ) -> Self {
        Runner {
            session,
            config,
            script,
            callbacks,
            host,
            output,
            force_refresh: false,
            last_error: None,
        }
    }

    pub(super) async fn ensure(&mut self) -> session::Result<Value> {
        let args = json!({"host": self.host});
        let outcome = self
            .session
            .run_with_token(
                App::Teams,
                &ic3_scope(),
                &self.script.ensure_body(),
                &args,
                self.force_refresh,
            )
            .await;
        if outcome.is_ok() {
            self.force_refresh = false;
        }
        outcome
    }

    pub(super) async fn run(
        mut self,
        first: Attached,
        mut stop: oneshot::Receiver<oneshot::Sender<()>>,
    ) {
        let mut attached = Some(first);
        let mut backoff = Duration::from_secs(1);
        loop {
            let current = match attached.take() {
                Some(current) => current,
                None => match attach(&self.session, &self.script).await {
                    Ok(current) => {
                        backoff = Duration::from_secs(1);
                        current
                    }
                    Err(error) => {
                        self.report_error(format!("reattach_failed: {error}"));
                        tokio::select! {
                            _ = sleep(backoff) => {}
                            _ = &mut stop => return,
                            _ = self.output.closed() => return,
                        }
                        backoff = (backoff * 2).min(self.config.max_reattach_backoff);
                        continue;
                    }
                },
            };
            match self.drive(current, &mut stop).await {
                DriveEnd::Stopped(done) => {
                    if let Some(done) = done {
                        let _ = done.send(());
                    }
                    return;
                }
                DriveEnd::TabLost => {
                    self.send(RealtimeEvent::Status(StatusEvent {
                        kind: StatusKind::Disconnected,
                        detail: "tab_connection_lost".into(),
                    }));
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn drive(
        &mut self,
        mut attached: Attached,
        stop: &mut oneshot::Receiver<oneshot::Sender<()>>,
    ) -> DriveEnd {
        let mut tick = interval(self.config.ensure_interval);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await;
        self.ensure_and_report().await;
        loop {
            tokio::select! {
                event = attached.events.recv() => {
                    let Some(event) = event else { return DriveEnd::TabLost };
                    if let Some(payload) = event.binding_payload(&self.script.binding_name) {
                        self.forward_payload(payload);
                    } else if is_main_frame_navigation(&event.method, &event.params) {
                        sleep(self.config.navigation_settle).await;
                        self.ensure_and_report().await;
                    }
                }
                _ = tick.tick() => self.ensure_and_report().await,
                Some(reply) = self.callbacks.replies.recv() => {
                    let expression = reply_expression(&self.script.global_name, &reply);
                    let _ = attached.control.evaluate(&expression).await;
                }
                done = &mut *stop => {
                    attached.shutdown().await;
                    return DriveEnd::Stopped(done.ok());
                }
                _ = self.output.closed() => {
                    attached.shutdown().await;
                    return DriveEnd::Stopped(None);
                }
            }
        }
    }

    fn forward_payload(&mut self, payload: &str) {
        if let Some(callback) = decode_callback(payload) {
            let _ = self.callbacks.output.send(callback);
            return;
        }
        let Ok(event) = decode_payload(payload) else {
            return;
        };
        if let RealtimeEvent::Status(status) = &event
            && status.kind == StatusKind::Error
            && is_auth_failure(&status.detail)
        {
            self.force_refresh = true;
        }
        self.send(event);
    }

    async fn ensure_and_report(&mut self) {
        match self.ensure().await {
            Ok(_) => self.last_error = None,
            Err(error) => self.report_error(format!("ensure_failed: {error}")),
        }
    }

    fn report_error(&mut self, detail: String) {
        if self.last_error.as_deref() == Some(detail.as_str()) {
            return;
        }
        self.last_error = Some(detail.clone());
        self.send(RealtimeEvent::Status(StatusEvent {
            kind: StatusKind::Error,
            detail,
        }));
    }

    fn send(&self, event: RealtimeEvent) {
        let _ = self.output.send(event);
    }
}

fn is_main_frame_navigation(method: &str, params: &Value) -> bool {
    method == "Page.frameNavigated" && params.pointer("/frame/parentId").is_none()
}

fn is_auth_failure(detail: &str) -> bool {
    detail == "registrar_http_401" || detail == "registrar_http_403"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_top_level_navigations_trigger_ensure() {
        assert!(is_main_frame_navigation(
            "Page.frameNavigated",
            &json!({"frame": {"id": "a"}})
        ));
        assert!(!is_main_frame_navigation(
            "Page.frameNavigated",
            &json!({"frame": {"id": "b", "parentId": "a"}})
        ));
        assert!(!is_main_frame_navigation("Page.loadEventFired", &json!({})));
    }

    #[test]
    fn rejected_bearer_forces_token_refresh() {
        assert!(is_auth_failure("registrar_http_401"));
        assert!(!is_auth_failure("registrar_http_500"));
        assert!(!is_auth_failure("registrar_failed"));
    }

    #[test]
    fn ensure_body_embeds_the_worker_once_before_the_call() {
        let ensure_body = PageScript::new(&RealtimeConfig::default()).ensure_body();
        assert!(ensure_body.starts_with("(() => {"));
        assert!(
            ensure_body
                .trim_end()
                .ends_with("window.__chatsvcTrouter.ensure({token, host: args.host});")
        );
        assert_eq!(ensure_body.matches("const VERSION").count(), 1);
    }

    #[test]
    fn second_instance_gets_its_own_page_names_and_callbacks() {
        let config = RealtimeConfig {
            instance: super::super::InstanceNames {
                global: "__callingTrouter".into(),
                binding: "__callingRealtime".into(),
                endpoint_storage_key: "__callingEpid".into(),
            },
            forward_callbacks: true,
            ..RealtimeConfig::default()
        };
        let script = PageScript::new(&config);
        assert!(!script.worker_source.contains("__chatsvc"));
        assert!(script.worker_source.contains("const GLOBAL_NAME = '__callingTrouter';"));
        assert!(script.worker_source.contains("const FORWARD_CALLBACKS = true;"));
        assert_eq!(script.stop_expression(), "window.__callingTrouter ? window.__callingTrouter.stop() : null");
        let default_script = PageScript::new(&RealtimeConfig::default());
        assert!(default_script.worker_source.contains(FORWARD_CALLBACKS_OFF));
    }
}
