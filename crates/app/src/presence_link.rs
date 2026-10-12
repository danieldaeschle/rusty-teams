use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chatsvc::cards::CardTransport;
use chatsvc::{ActivityReporting, PresenceService, error_status, is_endpoint_missing};
use session::Session;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::time::{Instant, MissedTickBehavior, interval};

const FALLBACK_AFTER: Duration = Duration::from_secs(120);
const FALLBACK_INTERVAL: Duration = Duration::from_secs(120);
const CHECK_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_BASE: Duration = Duration::from_secs(30);
const RETRY_MAX: Duration = Duration::from_secs(240);
const PERMANENT_FAILURE_RETRY: Duration = Duration::from_secs(30 * 60);

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

pub fn retry_delay(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(8);
    (RETRY_BASE * 2u32.pow(doublings)).min(RETRY_MAX)
}

pub fn is_permanent_failure(status: Option<u16>) -> bool {
    matches!(status, Some(400..=499)) && !matches!(status, Some(401 | 404 | 408 | 429))
}

pub fn start(
    session: &Session,
    activity: watch::Receiver<bool>,
    resend: Arc<Notify>,
) -> mpsc::UnboundedSender<LinkSignal> {
    let (signals, receiver) = mpsc::unbounded_channel();
    let (unregister, unregisters) = mpsc::unbounded_channel();
    let _ = UNREGISTER.set(unregister);
    let link = Link::new(PresenceService::new(session), activity, resend);
    tokio::spawn(link.run(receiver, unregisters));
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
    let _ = crate::runtime::handle().block_on(async move { tokio::time::timeout(timeout, finished).await });
}

#[derive(Default)]
struct RetryState {
    failures: u32,
    next_attempt: Option<Instant>,
    last_error: Option<String>,
}

impl RetryState {
    fn may_attempt(&self, now: Instant) -> bool {
        self.next_attempt.is_none_or(|next| now >= next)
    }

    fn succeeded(&mut self) {
        *self = RetryState::default();
    }

    fn failed(&mut self, now: Instant, error: &chatsvc::Error) -> Option<String> {
        self.failures += 1;
        let delay = if is_permanent_failure(error_status(error)) {
            PERMANENT_FAILURE_RETRY
        } else {
            retry_delay(self.failures)
        };
        self.next_attempt = Some(now + delay);
        let message = error.to_string();
        let is_new = self.last_error.as_deref() != Some(message.as_str());
        self.last_error = Some(message.clone());
        is_new.then_some(message)
    }
}

struct Link<T: CardTransport> {
    service: PresenceService<T>,
    activity: watch::Receiver<bool>,
    resend: Arc<Notify>,
    endpoint_id: Option<String>,
    registered: bool,
    applied: ActivityReporting,
    down_since: Option<Instant>,
    last_report: Option<Instant>,
    retry: RetryState,
}

impl<T: CardTransport> Link<T> {
    fn new(service: PresenceService<T>, activity: watch::Receiver<bool>, resend: Arc<Notify>) -> Self {
        Link {
            service,
            activity,
            resend,
            endpoint_id: None,
            registered: false,
            applied: ActivityReporting::Transport,
            down_since: None,
            last_report: None,
            retry: RetryState::default(),
        }
    }

    async fn run(
        mut self,
        mut signals: mpsc::UnboundedReceiver<LinkSignal>,
        mut unregisters: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    ) {
        let mut check = interval(CHECK_INTERVAL);
        check.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                signal = signals.recv() => {
                    let Some(signal) = signal else { return };
                    self.apply(signal).await;
                }
                Ok(()) = self.activity.changed() => {
                    self.activity.borrow_and_update();
                    self.reconcile(true).await;
                }
                _ = check.tick() => self.reconcile(false).await,
                Some(done) = unregisters.recv() => {
                    if let Some(endpoint_id) = &self.endpoint_id {
                        let _ = self.service.unregister_endpoint(endpoint_id).await;
                    }
                    let _ = done.send(());
                }
            }
        }
    }

    async fn apply(&mut self, signal: LinkSignal) {
        match signal {
            LinkSignal::Endpoint(endpoint_id) => {
                if self.endpoint_id.as_deref() == Some(endpoint_id.as_str()) {
                    return;
                }
                if let Some(previous) = self.endpoint_id.replace(endpoint_id) {
                    let _ = self.service.unregister_endpoint(&previous).await;
                }
                self.registered = false;
                self.applied = ActivityReporting::Transport;
                self.down_since = None;
                self.last_report = None;
                self.retry.succeeded();
            }
            LinkSignal::Connected => {
                if self.down_since.take().is_some() {
                    self.last_report = None;
                    self.retry.succeeded();
                }
            }
            LinkSignal::Disconnected => {
                self.down_since.get_or_insert_with(Instant::now);
            }
        }
        self.reconcile(false).await;
    }

    async fn reconcile(&mut self, activity_changed: bool) {
        let Some(endpoint_id) = self.endpoint_id.clone() else {
            return;
        };
        let now = Instant::now();
        let down_for = self.down_since.map(|since| now.duration_since(since));
        let wanted = match down_for {
            Some(down_for) if down_for >= FALLBACK_AFTER => ActivityReporting::Direct,
            _ => ActivityReporting::Transport,
        };
        if (!self.registered || self.applied != wanted) && self.retry.may_attempt(now) {
            self.register(&endpoint_id, wanted, now).await;
        }
        if wanted != ActivityReporting::Direct || !self.registered || self.applied != wanted {
            return;
        }
        let since_last_report = self.last_report.map(|last| now.duration_since(last));
        if report_due(down_for, since_last_report, activity_changed) {
            self.last_report = Some(now);
            let active = *self.activity.borrow();
            self.report_with_recovery(&endpoint_id, active).await;
        }
    }

    async fn register(&mut self, endpoint_id: &str, reporting: ActivityReporting, now: Instant) {
        match self.service.register_endpoint(endpoint_id, reporting).await {
            Ok(()) => {
                self.registered = true;
                self.applied = reporting;
                self.retry.succeeded();
                if reporting == ActivityReporting::Transport {
                    self.resend.notify_one();
                }
            }
            Err(error) => {
                if let Some(message) = self.retry.failed(now, &error) {
                    crate::diagnostics::record(format!("presence: endpoint not registered: {message}"));
                }
            }
        }
    }

    async fn report_with_recovery(&self, endpoint_id: &str, active: bool) {
        let Err(error) = self.service.report_activity(endpoint_id, active).await else {
            return;
        };
        if is_endpoint_missing(&error)
            && self
                .service
                .register_endpoint(endpoint_id, ActivityReporting::Direct)
                .await
                .is_ok()
            && self.service.report_activity(endpoint_id, active).await.is_ok()
        {
            return;
        }
        crate::diagnostics::record(format!("presence: activity not reported: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chatsvc::Result;
    use session::{ApiResponse, Method, Request, Scope};

    use super::*;

    const PRESENCE: &str = "https://presence.teams.microsoft.com/v1/me";

    type Sent = Arc<Mutex<Vec<(Method, String, Option<String>)>>>;

    struct Scripted {
        statuses: Mutex<Vec<u16>>,
        sent: Sent,
    }

    impl CardTransport for Scripted {
        async fn send(&self, request: Request, _scope: Scope) -> Result<ApiResponse> {
            let reporting = request
                .body
                .as_ref()
                .and_then(|body| body["activityReporting"].as_str())
                .map(str::to_owned);
            self.sent.lock().unwrap().push((request.method, request.url, reporting));
            let mut statuses = self.statuses.lock().unwrap();
            let status = if statuses.is_empty() { 200 } else { statuses.remove(0) };
            Ok(ApiResponse {
                status,
                body: serde_json::Value::Null,
                retry_after: None,
            })
        }
    }

    fn link(statuses: Vec<u16>) -> (Link<Scripted>, Sent, Arc<Notify>) {
        let sent: Sent = Arc::default();
        let resend = Arc::new(Notify::new());
        let (_, activity) = watch::channel(true);
        let service = PresenceService::with_transport(Scripted {
            statuses: Mutex::new(statuses),
            sent: sent.clone(),
        });
        (Link::new(service, activity, resend.clone()), sent, resend)
    }

    fn calls(sent: &Sent) -> Vec<String> {
        sent.lock()
            .unwrap()
            .iter()
            .map(|(method, url, reporting)| {
                let path = url.strip_prefix(PRESENCE).unwrap();
                format!("{} {path} {}", method.as_str(), reporting.as_deref().unwrap_or("-"))
            })
            .collect()
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

    #[test]
    fn retry_delay_doubles_up_to_four_minutes() {
        let delays: Vec<u64> = (1..=6).map(|failures| retry_delay(failures).as_secs()).collect();
        assert_eq!(delays, [30, 60, 120, 240, 240, 240]);
    }

    #[test]
    fn only_client_errors_other_than_auth_and_missing_are_permanent() {
        assert!(is_permanent_failure(Some(400)));
        assert!(is_permanent_failure(Some(403)));
        assert!(!is_permanent_failure(Some(401)));
        assert!(!is_permanent_failure(Some(404)));
        assert!(!is_permanent_failure(Some(429)));
        assert!(!is_permanent_failure(Some(500)));
        assert!(!is_permanent_failure(None));
    }

    #[tokio::test]
    async fn a_repeated_identical_failure_is_logged_once_and_backs_off() {
        let error = chatsvc::Error::UnexpectedAnswer("x".into());
        let mut retry = RetryState::default();
        let now = Instant::now();
        assert!(retry.failed(now, &error).is_some());
        assert!(retry.failed(now, &error).is_none());
        assert!(!retry.may_attempt(now + seconds(59)));
        assert!(retry.may_attempt(now + seconds(60)));
        retry.succeeded();
        assert!(retry.may_attempt(now));
    }

    #[tokio::test]
    async fn a_permanent_failure_retries_only_after_half_an_hour_or_a_new_endpoint() {
        let (mut link, sent, _) = link(vec![403]);
        link.apply(LinkSignal::Endpoint("a".into())).await;
        link.reconcile(false).await;
        assert_eq!(calls(&sent).len(), 1);
        assert!(!link.retry.may_attempt(Instant::now() + seconds(29 * 60)));
        assert!(link.retry.may_attempt(Instant::now() + seconds(31 * 60)));
        link.apply(LinkSignal::Endpoint("b".into())).await;
        assert_eq!(calls(&sent).len(), 3);
    }

    #[tokio::test]
    async fn the_same_endpoint_announced_again_is_not_registered_again() {
        let (mut link, sent, resend) = link(vec![]);
        link.apply(LinkSignal::Endpoint("a".into())).await;
        link.apply(LinkSignal::Endpoint("a".into())).await;
        link.apply(LinkSignal::Connected).await;
        assert_eq!(calls(&sent), ["PUT /endpoints/ Transport"]);
        tokio::time::timeout(Duration::from_millis(500), resend.notified()).await.expect("resend");
    }

    #[tokio::test]
    async fn a_new_endpoint_removes_the_old_one_first() {
        let (mut link, sent, _) = link(vec![]);
        link.apply(LinkSignal::Endpoint("a".into())).await;
        link.apply(LinkSignal::Endpoint("b".into())).await;
        assert_eq!(
            calls(&sent),
            ["PUT /endpoints/ Transport", "DELETE /endpoints/a -", "PUT /endpoints/ Transport"]
        );
    }

    #[tokio::test]
    async fn long_downtime_switches_to_direct_reporting_and_a_reconnect_switches_back() {
        let (mut link, sent, resend) = link(vec![]);
        link.apply(LinkSignal::Endpoint("a".into())).await;
        tokio::time::timeout(Duration::from_millis(500), resend.notified()).await.expect("resend");
        link.down_since = Some(Instant::now().checked_sub(seconds(130)).unwrap_or_else(Instant::now));
        link.reconcile(false).await;
        link.apply(LinkSignal::Connected).await;
        assert_eq!(
            calls(&sent),
            [
                "PUT /endpoints/ Transport",
                "PUT /endpoints/ Direct",
                "POST /reportmyactivity/ -",
                "PUT /endpoints/ Transport",
            ]
        );
        tokio::time::timeout(Duration::from_millis(500), resend.notified()).await.expect("resend");
    }

    #[tokio::test]
    async fn a_missing_endpoint_is_registered_again_before_the_report_is_repeated() {
        let (link, sent, _) = link(vec![404, 200, 200]);
        link.report_with_recovery("epid-1", true).await;
        assert_eq!(
            calls(&sent),
            [
                "POST /reportmyactivity/ -",
                "PUT /endpoints/ Direct",
                "POST /reportmyactivity/ -",
            ]
        );
    }
}
