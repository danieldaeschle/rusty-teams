use std::sync::OnceLock;
use std::time::Duration;

use chatsvc::cards::CardTransport;
use chatsvc::{PresenceService, is_endpoint_missing};
use session::Session;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, MissedTickBehavior, interval};

const FALLBACK_AFTER: Duration = Duration::from_secs(120);
const FALLBACK_INTERVAL: Duration = Duration::from_secs(120);
const CHECK_INTERVAL: Duration = Duration::from_secs(30);

static UNREGISTER: OnceLock<mpsc::UnboundedSender<oneshot::Sender<()>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSignal {
    Endpoint(String),
    Connected,
    Disconnected,
}

pub fn report_due(down_for: Option<Duration>, since_last_report: Option<Duration>, activity_changed: bool) -> bool {
    down_for.is_some_and(|down_for| down_for >= FALLBACK_AFTER)
        && (activity_changed || since_last_report.is_none_or(|since| since >= FALLBACK_INTERVAL))
}

pub fn start(session: &Session, activity: watch::Receiver<bool>) -> mpsc::UnboundedSender<LinkSignal> {
    let (signals, receiver) = mpsc::unbounded_channel();
    let (unregister, unregisters) = mpsc::unbounded_channel();
    let _ = UNREGISTER.set(unregister);
    let service = PresenceService::new(session);
    tokio::spawn(run(service, activity, receiver, unregisters));
    signals
}

/// Gives up after `timeout` so quitting is never held back.
pub fn unregister_on_exit(timeout: Duration) {
    let Some(unregister) = UNREGISTER.get() else {
        return;
    };
    let (done, finished) = oneshot::channel();
    if unregister.send(done).is_err() {
        return;
    }
    let _ = crate::runtime::handle().block_on(tokio::time::timeout(timeout, finished));
}

#[derive(Default)]
struct LinkState {
    endpoint_id: Option<String>,
    registered: bool,
    down_since: Option<Instant>,
    last_report: Option<Instant>,
}

async fn run<T: CardTransport>(
    service: PresenceService<T>,
    mut activity: watch::Receiver<bool>,
    mut signals: mpsc::UnboundedReceiver<LinkSignal>,
    mut unregisters: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
) {
    let mut state = LinkState::default();
    let mut check = interval(CHECK_INTERVAL);
    check.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            signal = signals.recv() => {
                let Some(signal) = signal else { return };
                apply_signal(&service, &mut state, signal).await;
            }
            Ok(()) = activity.changed() => {
                let active = *activity.borrow_and_update();
                report_if_due(&service, &mut state, active, true).await;
            }
            _ = check.tick() => {
                if let Some(endpoint_id) = state.endpoint_id.clone().filter(|_| !state.registered) {
                    state.registered = register(&service, &endpoint_id).await;
                }
                let active = *activity.borrow();
                report_if_due(&service, &mut state, active, false).await;
            }
            Some(done) = unregisters.recv() => {
                if let Some(endpoint_id) = &state.endpoint_id {
                    let _ = service.unregister_endpoint(endpoint_id).await;
                }
                let _ = done.send(());
            }
        }
    }
}

async fn apply_signal<T: CardTransport>(service: &PresenceService<T>, state: &mut LinkState, signal: LinkSignal) {
    match signal {
        LinkSignal::Endpoint(endpoint_id) => {
            state.registered = register(service, &endpoint_id).await;
            state.endpoint_id = Some(endpoint_id);
            state.down_since = None;
            state.last_report = None;
        }
        LinkSignal::Connected => {
            state.down_since = None;
            state.last_report = None;
        }
        LinkSignal::Disconnected => {
            state.down_since.get_or_insert_with(Instant::now);
        }
    }
}

async fn register<T: CardTransport>(service: &PresenceService<T>, endpoint_id: &str) -> bool {
    match service.register_endpoint(endpoint_id).await {
        Ok(()) => true,
        Err(error) => {
            crate::diagnostics::record(format!("presence: endpoint not registered: {error}"));
            false
        }
    }
}

async fn report_if_due<T: CardTransport>(
    service: &PresenceService<T>,
    state: &mut LinkState,
    active: bool,
    activity_changed: bool,
) {
    let Some(endpoint_id) = state.endpoint_id.clone() else {
        return;
    };
    let now = Instant::now();
    let down_for = state.down_since.map(|since| now.duration_since(since));
    let since_last_report = state.last_report.map(|last| now.duration_since(last));
    if !report_due(down_for, since_last_report, activity_changed) {
        return;
    }
    state.last_report = Some(now);
    report_with_recovery(service, &endpoint_id, active).await;
}

async fn report_with_recovery<T: CardTransport>(service: &PresenceService<T>, endpoint_id: &str, active: bool) {
    let Err(error) = service.report_activity(endpoint_id, active).await else {
        return;
    };
    if is_endpoint_missing(&error)
        && service.register_endpoint(endpoint_id).await.is_ok()
        && service.report_activity(endpoint_id, active).await.is_ok()
    {
        return;
    }
    crate::diagnostics::record(format!("presence: activity not reported: {error}"));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use chatsvc::Result;
    use session::{ApiResponse, Method, Request, Scope};

    use super::*;

    struct Scripted {
        statuses: Mutex<Vec<u16>>,
        sent: Arc<Mutex<Vec<(Method, String)>>>,
    }

    impl CardTransport for Scripted {
        async fn send(&self, request: Request, _scope: Scope) -> Result<ApiResponse> {
            self.sent.lock().unwrap().push((request.method, request.url));
            let status = self.statuses.lock().unwrap().remove(0);
            Ok(ApiResponse {
                status,
                body: serde_json::Value::Null,
                retry_after: None,
            })
        }
    }

    fn seconds(count: u64) -> Duration {
        Duration::from_secs(count)
    }

    #[test]
    fn fallback_starts_after_two_minutes_down_and_repeats_every_two_minutes() {
        assert!(!report_due(None, None, true));
        assert!(!report_due(Some(seconds(119)), None, true));
        assert!(report_due(Some(seconds(120)), None, false));
        assert!(!report_due(Some(seconds(150)), Some(seconds(30)), false));
        assert!(report_due(Some(seconds(240)), Some(seconds(120)), false));
        assert!(report_due(Some(seconds(150)), Some(seconds(30)), true));
    }

    #[tokio::test]
    async fn a_missing_endpoint_is_registered_again_before_the_report_is_repeated() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let service = PresenceService::with_transport(Scripted {
            statuses: Mutex::new(vec![404, 200, 200]),
            sent: sent.clone(),
        });
        report_with_recovery(&service, "epid-1", true).await;
        let sent = sent.lock().unwrap();
        let calls: Vec<(Method, &str)> = sent.iter().map(|(method, url)| (*method, url.as_str())).collect();
        assert_eq!(
            calls,
            [
                (Method::Post, "https://presence.teams.microsoft.com/v1/me/reportmyactivity/"),
                (Method::Put, "https://presence.teams.microsoft.com/v1/me/endpoints/"),
                (Method::Post, "https://presence.teams.microsoft.com/v1/me/reportmyactivity/"),
            ]
        );
    }
}
