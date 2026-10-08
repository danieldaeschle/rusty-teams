use std::time::Duration;

use serde_json::{Value, json};
use session::{App, Session, TabControl, TabEvents};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{MissedTickBehavior, interval, sleep};

use super::RealtimeConfig;
use super::event::{RealtimeEvent, StatusEvent, StatusKind, decode_payload};
use super::host::ic3_scope;

pub(super) const BINDING_NAME: &str = "__chatsvcRealtime";
const WORKER_SOURCE: &str = include_str!("../../assets/trouter.js");
const ENSURE_BODY: &str = concat!(
    include_str!("../../assets/trouter.js"),
    "\nreturn window.__chatsvcTrouter.ensure({token, host: args.host});"
);
const STOP_EXPRESSION: &str = "window.__chatsvcTrouter ? window.__chatsvcTrouter.stop() : null";
const STOP_STEP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct Attached {
    control: TabControl,
    events: TabEvents,
    script_identifier: String,
}

pub(super) async fn attach(session: &Session) -> crate::error::Result<Attached> {
    let (control, events) = session.subscribe(App::Teams).await?;
    control.enable_events().await?;
    control.add_binding(BINDING_NAME).await?;
    let script_identifier = control.inject_script(WORKER_SOURCE).await?;
    Ok(Attached {
        control,
        events,
        script_identifier,
    })
}

impl Attached {
    pub(super) async fn shutdown(self) {
        let _ =
            tokio::time::timeout(STOP_STEP_TIMEOUT, self.control.evaluate(STOP_EXPRESSION)).await;
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
    host: String,
    output: mpsc::UnboundedSender<RealtimeEvent>,
    force_refresh: bool,
    last_error: Option<String>,
}

impl Runner {
    pub(super) fn new(
        session: Session,
        config: RealtimeConfig,
        host: String,
        output: mpsc::UnboundedSender<RealtimeEvent>,
    ) -> Self {
        Runner {
            session,
            config,
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
                ENSURE_BODY,
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
                None => match attach(&self.session).await {
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
                    if let Some(payload) = event.binding_payload(BINDING_NAME) {
                        self.forward_payload(payload);
                    } else if is_main_frame_navigation(&event.method, &event.params) {
                        sleep(self.config.navigation_settle).await;
                        self.ensure_and_report().await;
                    }
                }
                _ = tick.tick() => self.ensure_and_report().await,
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
        assert!(ENSURE_BODY.starts_with("(() => {"));
        assert!(
            ENSURE_BODY
                .trim_end()
                .ends_with("ensure({token, host: args.host});")
        );
        assert_eq!(ENSURE_BODY.matches("const VERSION").count(), 1);
    }
}
