use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep};

use crate::app::App;
use crate::error::{Error, Result};
use crate::events::TabControl;
use crate::native::NativeHttp;
use crate::request::{ApiResponse, Method, Outcome, Request, WireRequest, WireResult};
use crate::scope::Scope;
use crate::transport::{CdpTransport, Transport};

pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:9222";
pub const UNSEAL_SCRIPT: &str = include_str!("../assets/unseal.js");
pub(crate) static FETCH_SCRIPT: LazyLock<String> =
    LazyLock::new(|| include_str!("../assets/fetch.js").replace("__UNSEAL__", UNSEAL_SCRIPT));
pub(crate) const NOT_GRANTED_PREFIX: &str = "the app is not granted";
const MAX_DOWNLOAD_BYTES: u64 = 60 * 1024 * 1024;
const DEFAULT_CONCURRENCY: usize = 6;

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub refresh_margin: Duration,
    pub token_wait: Duration,
    pub token_poll: Duration,
    pub evaluate_timeout: Duration,
    pub max_throttle_retries: usize,
    pub max_retry_wait: Duration,
    pub default_retry_wait: Duration,
    pub unauthorized_reload_wait: Duration,
    pub native_http: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            refresh_margin: Duration::from_secs(120),
            token_wait: Duration::from_secs(60),
            token_poll: Duration::from_secs(3),
            evaluate_timeout: Duration::from_secs(60),
            max_throttle_retries: 3,
            max_retry_wait: Duration::from_secs(30),
            default_retry_wait: Duration::from_secs(2),
            unauthorized_reload_wait: Duration::from_secs(5),
            native_http: false,
        }
    }
}

impl SessionConfig {
    /// The app's settings: requests go out from Rust unless TEAMS_PAGE_FETCH is set.
    pub fn for_app() -> Self {
        SessionConfig {
            native_http: std::env::var_os("TEAMS_PAGE_FETCH").is_none(),
            ..SessionConfig::default()
        }
    }
}

#[derive(Default)]
struct TokenSearch {
    refresh_error: Option<String>,
    pages: Vec<String>,
    ungranted: HashSet<App>,
}

#[derive(Clone)]
pub struct Session {
    pub(crate) transport: Arc<dyn Transport>,
    pub(crate) config: SessionConfig,
    tabs: Arc<HashMap<App, Mutex<Option<TabControl>>>>,
    force_refresh: Arc<AtomicBool>,
    native: Option<Arc<NativeHttp>>,
    tenant: Option<Arc<str>>,
}

impl Session {
    pub async fn connect(endpoint: &str) -> Result<Self> {
        Self::connect_with(endpoint, SessionConfig::default()).await
    }

    pub async fn connect_with(endpoint: &str, config: SessionConfig) -> Result<Self> {
        Self::with_transport(Arc::new(CdpTransport::new(endpoint)), config).await
    }

    pub async fn with_transport(
        transport: Arc<dyn Transport>,
        config: SessionConfig,
    ) -> Result<Self> {
        transport.check().await?;
        Ok(Session {
            native: native_http(&config),
            tenant: None,
            transport,
            config,
            tabs: Arc::new(
                App::ALL
                    .into_iter()
                    .map(|app| (app, Mutex::new(None)))
                    .collect(),
            ),
            force_refresh: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The same browser session with tokens for `tenant_id`, e.g. a tenant the account is a guest in.
    /// Tokens are held apart from the home tenant's, so both can run side by side.
    pub fn in_tenant(&self, tenant_id: &str) -> Session {
        Session {
            native: native_http(&self.config),
            force_refresh: Arc::new(AtomicBool::new(false)),
            tenant: Some(tenant_id.to_ascii_lowercase().into()),
            ..self.clone()
        }
    }

    /// The tenant set with [`Session::in_tenant`]; `None` is the account's home tenant.
    pub fn tenant(&self) -> Option<&str> {
        self.tenant.as_deref()
    }

    pub async fn request(
        &self,
        method: Method,
        url: &str,
        scope: &Scope,
        body: Option<Value>,
    ) -> Result<ApiResponse> {
        let request = Request {
            method,
            body,
            ..Request::get(url)
        };
        self.send(request, scope).await
    }

    pub async fn send(&self, request: Request, scope: &Scope) -> Result<ApiResponse> {
        let url = request.url.clone();
        let response = self.batch(&[request], scope).await?.remove(0);
        if response.status >= 400 || response.status == 0 {
            return Err(Error::api(response.status, &url, response.body));
        }
        Ok(response)
    }

    pub async fn batch(&self, requests: &[Request], scope: &Scope) -> Result<Vec<ApiResponse>> {
        let mut results: Vec<Option<ApiResponse>> = vec![None; requests.len()];
        let mut pending: Vec<usize> = (0..requests.len()).collect();
        let mut forced_here = false;
        for attempt in 0..=self.config.max_throttle_retries {
            let batch: Vec<&Request> = pending.iter().map(|&index| &requests[index]).collect();
            let answers = match self.run_with_fresh_token(&batch, scope).await {
                Ok(answers) => answers,
                Err(error) => {
                    if forced_here {
                        self.force_refresh.store(false, Ordering::SeqCst);
                    }
                    return Err(error);
                }
            };
            let unauthorized_first_try =
                attempt == 0 && answers.iter().any(|answer| answer.status == 401);
            if unauthorized_first_try {
                // A CAE-revoked token stays unexpired in the app cache, so only a refresh-token grant replaces it.
                self.force_refresh.store(true, Ordering::SeqCst);
                forced_here = true;
                self.reload_all().await;
                sleep(self.config.unauthorized_reload_wait).await;
                self.drop_all().await;
            }
            let mut retry = Vec::new();
            for (&index, answer) in pending.iter().zip(answers) {
                let throttled = answer.status == 429 || (attempt == 0 && answer.status == 401);
                results[index] = Some(answer);
                if throttled {
                    retry.push(index);
                }
            }
            if forced_here && !unauthorized_first_try {
                self.force_refresh.store(false, Ordering::SeqCst);
                forced_here = false;
            }
            if retry.is_empty() || attempt == self.config.max_throttle_retries {
                break;
            }
            let wait = retry
                .iter()
                .filter_map(|&index| results[index].as_ref())
                .map(|answer| {
                    retry_wait(
                        answer.retry_after.as_deref(),
                        self.config.default_retry_wait,
                    )
                })
                .max()
                .unwrap_or_default();
            sleep(wait.min(self.config.max_retry_wait)).await;
            pending = retry;
        }
        Ok(results.into_iter().flatten().collect())
    }

    /// Keeps the app tab where it is until the guard drops, e.g. for the length of a call.
    pub fn hold_tab(&self, app: App) -> TabHold {
        self.transport.hold(app, true);
        TabHold {
            transport: self.transport.clone(),
            app,
        }
    }

    pub async fn close(&self) {
        self.drop_all().await;
    }

    async fn run_with_fresh_token(
        &self,
        requests: &[&Request],
        scope: &Scope,
    ) -> Result<Vec<ApiResponse>> {
        if let Some(native) = &self.native
            && let Some(results) = native
                .run(
                    self,
                    requests,
                    scope,
                    self.force_refresh.load(Ordering::SeqCst),
                )
                .await
        {
            return Ok(results);
        }
        let mut search = TokenSearch::default();
        if let Some(results) = self.run_in_any_tab(requests, scope, &mut search).await? {
            return Ok(results);
        }
        if search.ungranted.len() == App::ALL.len() {
            return Err(Error::LoginRequired(format!(
                "the Teams web app is not granted {scope}"
            )));
        }
        // A guest tenant's refusal does not heal by waiting, and a reload would reload the home org's tabs.
        let refused_guest = self.tenant().is_some()
            && search
                .refresh_error
                .as_deref()
                .is_some_and(crate::error::is_refusal);
        if !refused_guest {
            self.reload_all().await;
            let deadline = Instant::now() + self.config.token_wait;
            while Instant::now() < deadline {
                sleep(self.config.token_poll).await;
                self.drop_all().await;
                if let Some(results) = self.run_in_any_tab(requests, scope, &mut search).await? {
                    return Ok(results);
                }
            }
        }
        let diagnosis = self.transport.diagnose().await?;
        if diagnosis.login_pending {
            return Err(Error::LoginRequired("sign in in the browser window".into()));
        }
        if !diagnosis.app_tab_open {
            return Err(Error::NoAppTab);
        }
        let mut detail = search
            .refresh_error
            .map(|reason| format!(" (token refresh failed in {reason})"))
            .unwrap_or_default();
        if !search.pages.is_empty() {
            detail.push_str(&format!(" [{}]", search.pages.join("; ")));
        }
        Err(Error::NoFreshToken {
            scope: scope.to_string(),
            detail,
        })
    }

    async fn run_in_any_tab(
        &self,
        requests: &[&Request],
        scope: &Scope,
        search: &mut TokenSearch,
    ) -> Result<Option<Vec<ApiResponse>>> {
        let payload = self.payload(requests, scope);
        let expression = format!("({})({payload})", FETCH_SCRIPT.as_str());
        let has_write = requests.iter().any(|request| request.method != Method::Get);
        for app in App::ALL {
            let outcome = match self.evaluate_in(app, &expression).await {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(error) => {
                    self.drop_connection(app).await;
                    if has_write {
                        return Err(Error::WriteInterrupted(error.to_string()));
                    }
                    continue;
                }
            };
            let outcome: Outcome = serde_json::from_value(outcome)
                .map_err(|error| Error::Cdp(format!("unexpected page answer: {error}")))?;
            if let Some(results) = outcome.results {
                return Ok(Some(results.into_iter().map(WireResult::into).collect()));
            }
            if let Some(page) = &outcome.page {
                let line = format!("{app}: {page}");
                search
                    .pages
                    .retain(|seen| !seen.starts_with(&format!("{app}:")));
                search.pages.push(line);
            }
            if let Some(reason) = outcome.refresh_error {
                if reason.starts_with(NOT_GRANTED_PREFIX) {
                    search.ungranted.insert(app);
                }
                keep_refresh_error(&mut search.refresh_error, &format!("{app}: {reason}"));
            }
        }
        Ok(None)
    }

    fn payload(&self, requests: &[&Request], scope: &Scope) -> String {
        let wire: Vec<WireRequest> = requests.iter().map(|&request| request.into()).collect();
        json!({
            "requests": wire,
            "resource": scope.resource,
            "scope": scope.name,
            "nestedApp": scope.nested_app_json(),
            "marginMs": self.config.refresh_margin.as_millis() as u64,
            "concurrency": DEFAULT_CONCURRENCY,
            "maxBytes": MAX_DOWNLOAD_BYTES,
            "forceRefresh": self.force_refresh.load(Ordering::SeqCst),
            "tenant": self.tenant(),
        })
        .to_string()
    }

    pub(crate) async fn evaluate_in(&self, app: App, expression: &str) -> Result<Option<Value>> {
        let mut slot = self.tabs[&app].lock().await;
        if slot.is_none() {
            let Some((control, _)) = self.transport.open(app, false).await? else {
                return Ok(None);
            };
            *slot = Some(control);
        }
        let control = slot.as_ref().expect("connection set above");
        let outcome = control
            .evaluate_within(expression, self.config.evaluate_timeout)
            .await;
        if outcome.is_err() {
            *slot = None;
        }
        outcome.map(Some)
    }

    async fn drop_connection(&self, app: App) {
        *self.tabs[&app].lock().await = None;
    }

    async fn drop_all(&self) {
        for app in App::ALL {
            self.drop_connection(app).await;
        }
    }

    async fn reload_all(&self) {
        for app in App::ALL {
            let _ = self.transport.wake(app).await;
        }
    }
}

pub struct TabHold {
    transport: Arc<dyn Transport>,
    app: App,
}

impl Drop for TabHold {
    fn drop(&mut self) {
        self.transport.hold(self.app, false);
    }
}

fn native_http(config: &SessionConfig) -> Option<Arc<NativeHttp>> {
    config
        .native_http
        .then(|| NativeHttp::new(config.evaluate_timeout, MAX_DOWNLOAD_BYTES))
        .flatten()
        .map(Arc::new)
}

/// The first error, unless a later one is a refusal: that decides what the app does next.
fn keep_refresh_error(kept: &mut Option<String>, line: &str) {
    if kept.is_none() || crate::error::is_refusal(line) {
        *kept = Some(line.to_owned());
    }
}

pub(crate) fn retry_wait(header: Option<&str>, default: Duration) -> Duration {
    header
        .and_then(|value| value.trim().parse::<f64>().ok())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::transport::{BoxFuture, Diagnosis, OpenedTab};

    #[derive(Default)]
    struct HoldRecorder {
        holds: StdMutex<Vec<(App, bool)>>,
    }

    impl Transport for HoldRecorder {
        fn check(&self) -> BoxFuture<'_, Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn open(&self, _app: App, _with_events: bool) -> BoxFuture<'_, Result<Option<OpenedTab>>> {
            Box::pin(async { Ok(None) })
        }

        fn wake(&self, _app: App) -> BoxFuture<'_, Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn diagnose(&self) -> BoxFuture<'_, Result<Diagnosis>> {
            Box::pin(async { Ok(Diagnosis::default()) })
        }

        fn hold(&self, app: App, held: bool) {
            self.holds.lock().unwrap().push((app, held));
        }
    }

    #[tokio::test]
    async fn tab_hold_releases_the_tab_when_dropped() {
        let recorder = Arc::new(HoldRecorder::default());
        let session = Session::with_transport(recorder.clone(), SessionConfig::default())
            .await
            .unwrap();
        let hold = session.hold_tab(App::Teams);
        assert_eq!(*recorder.holds.lock().unwrap(), vec![(App::Teams, true)]);
        drop(hold);
        assert_eq!(
            *recorder.holds.lock().unwrap(),
            vec![(App::Teams, true), (App::Teams, false)]
        );
    }

    #[tokio::test]
    async fn a_tenant_session_asks_the_page_for_that_tenant_only() {
        let home =
            Session::with_transport(Arc::new(HoldRecorder::default()), SessionConfig::default())
                .await
                .unwrap();
        let guest = home.in_tenant("AAAA-Guest");
        let request = Request::get("https://graph.microsoft.com/v1.0/me");
        let tenant_of = |session: &Session| {
            let payload: Value =
                serde_json::from_str(&session.payload(&[&request], &Scope::graph("User.Read")))
                    .unwrap();
            payload["tenant"].clone()
        };
        assert_eq!(tenant_of(&home), Value::Null);
        assert_eq!(tenant_of(&guest), json!("aaaa-guest"));
        assert_eq!(home.tenant(), None);
    }

    #[test]
    fn a_refusal_replaces_an_earlier_refresh_error_and_nothing_else_does() {
        let mut kept = None;
        keep_refresh_error(&mut kept, "teams: HTTP 0");
        keep_refresh_error(&mut kept, "outlook: timeout");
        assert_eq!(kept.as_deref(), Some("teams: HTTP 0"));
        keep_refresh_error(&mut kept, "outlook: invalid_grant: AADSTS50076");
        keep_refresh_error(&mut kept, "teams: HTTP 0");
        assert_eq!(kept.as_deref(), Some("outlook: invalid_grant: AADSTS50076"));
    }

    #[test]
    fn retry_after_seconds_parse() {
        let default = Duration::from_secs(2);
        assert_eq!(retry_wait(Some("7"), default), Duration::from_secs(7));
        assert_eq!(
            retry_wait(Some(" 1.5 "), default),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn retry_after_falls_back() {
        let default = Duration::from_secs(2);
        assert_eq!(retry_wait(None, default), default);
        assert_eq!(
            retry_wait(Some("Wed, 21 Oct 2026 07:28:00 GMT"), default),
            default
        );
        assert_eq!(retry_wait(Some("-3"), default), default);
        assert_eq!(retry_wait(Some("1e20"), default), default);
    }
}
