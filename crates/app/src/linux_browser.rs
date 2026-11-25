use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::{Mutex as AsyncMutex, Notify, mpsc, watch};

use crate::backend::BackendEvent;
use crate::browser_sign_in::BrowserSignInState;
use crate::linux_park::{self, ParkClock};

const STARTUP_WAIT: Duration = Duration::from_secs(20);
const RELAUNCH_WAIT: Duration = Duration::from_secs(5);
const POLL_STEP: Duration = Duration::from_millis(100);
const CHECK_WAIT: Duration = Duration::from_millis(250);
const HIDE_POLL: Duration = Duration::from_millis(500);
const HIDE_STABLE_CHECKS: u32 = 4;
const CREDENTIAL_READ_ATTEMPTS: usize = 5;
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
const NATIVE_REFRESH_TOKEN_FILE: &str = "rusty-teams-native-refresh-token";

static LINUX_BROWSER: OnceLock<browser::Browser> = OnceLock::new();
static LIVE_RETRY: OnceLock<Mutex<Option<Arc<Notify>>>> = OnceLock::new();
static UI_EVENTS: OnceLock<mpsc::UnboundedSender<BackendEvent>> = OnceLock::new();

fn live_retry_slot() -> &'static Mutex<Option<Arc<Notify>>> {
    LIVE_RETRY.get_or_init(|| Mutex::new(None))
}

/// Lets the managed-browser supervisor push splash/banner updates into the UI event loop.
pub fn attach_ui_events(sender: mpsc::UnboundedSender<BackendEvent>) {
    let _ = UI_EVENTS.set(sender);
}

fn publish_sign_in(state: BrowserSignInState) {
    crate::browser_sign_in::set(state.clone());
    if let Some(sender) = UI_EVENTS.get() {
        let _ = sender.send(BackendEvent::BrowserSignIn(state));
    }
}

pub fn note_backend_event(event: &BackendEvent) {
    if let BackendEvent::LiveRetry(retry) = event
        && let Ok(mut slot) = live_retry_slot().lock()
    {
        *slot = Some(retry.clone());
    }
    if let BackendEvent::Connection(crate::backend::ConnectionState::Online) = event {
        publish_sign_in(BrowserSignInState::Idle);
    }
}

fn trigger_live_retry() {
    if let Ok(slot) = live_retry_slot().lock()
        && let Some(retry) = slot.as_ref()
    {
        retry.notify_one();
    }
}

#[derive(Default)]
struct HideAfterLogin {
    stable: u32,
}

impl HideAfterLogin {
    fn reset(&mut self) {
        self.stable = 0;
    }

    fn step(&mut self, status: Option<&browser::Status>, has_msal_cache: bool) -> bool {
        let Some(status) = status else {
            self.stable = 0;
            return false;
        };
        if !status.running || status.mode != browser::Mode::Visible {
            self.reset();
            return false;
        }
        let teams_loaded = status.login_state == browser::LoginState::NotRequired
            && status
                .tabs
                .iter()
                .any(|tab| tab.app == Some(browser::App::Teams) && !tab.parked);
        if !teams_loaded || !has_msal_cache {
            self.stable = 0;
            return false;
        }
        self.stable += 1;
        if self.stable >= HIDE_STABLE_CHECKS {
            self.reset();
            return true;
        }
        false
    }
}

/// Moves a managed visible browser back to headless once Teams sign-in completes.
async fn hide_after_login(browser: browser::Browser, headless_notify: Arc<Notify>) {
    let mut tracker = HideAfterLogin::default();
    loop {
        tokio::time::sleep(HIDE_POLL).await;
        if browser.is_busy() {
            tracker.stable = 0;
            continue;
        }
        if !browser.is_visible_managed() {
            tracker.reset();
            continue;
        }
        let status = browser.status().await.ok();
        let has_msal_cache = match status.as_ref() {
            Some(status)
                if status.running
                    && status.mode == browser::Mode::Visible
                    && status.login_state == browser::LoginState::NotRequired
                    && status
                        .tabs
                        .iter()
                        .any(|tab| tab.app == Some(browser::App::Teams) && !tab.parked) =>
            {
                browser.teams_has_msal_cache().await
            }
            _ => false,
        };
        if tracker.step(status.as_ref(), has_msal_cache) {
            log::info!("Teams sign-in finished: moving the browser to the background");
            publish_sign_in(BrowserSignInState::Finishing);
            if let Err(error) = browser.headless().await {
                log::error!("Failed to switch the browser to headless: {error}");
                publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            } else {
                headless_notify.notify_waiters();
                trigger_live_retry();
            }
        }
    }
}

/// Native authentication normally starts from its saved refresh credential. If Microsoft requires
/// a sign-in, this transport reuses the application's normal browser flow, imports the resulting
/// Teams credential, closes the browser, and exposes the native credential provider.
struct NativeLoginTransport {
    browser: Mutex<Option<browser::Browser>>,
    config: session::NativeAuthConfig,
    auth: Mutex<Option<Arc<session::NativeAuth>>>,
    initialize: AsyncMutex<()>,
}

impl NativeLoginTransport {
    fn new(refresh_token_file: impl Into<PathBuf>) -> Self {
        NativeLoginTransport {
            browser: Mutex::new(None),
            config: session::NativeAuthConfig::teams(refresh_token_file),
            auth: Mutex::new(None),
            initialize: AsyncMutex::new(()),
        }
    }

    fn sign_in_browser(&self) -> session::Result<browser::Browser> {
        if let Ok(saved) = self.browser.lock()
            && let Some(browser) = saved.as_ref()
        {
            return Ok(browser.clone());
        }

        let detected = browser::Browser::detect().map_err(|error| {
            session::Error::NativeAuth(format!("could not locate a sign-in browser: {error}"))
        })?;
        let _ = LINUX_BROWSER.set(detected.clone());
        if let Ok(mut saved) = self.browser.lock() {
            *saved = Some(detected.clone());
        }
        Ok(detected)
    }

    fn saved_auth(&self) -> Option<Arc<session::NativeAuth>> {
        self.auth
            .lock()
            .ok()
            .and_then(|auth| auth.as_ref().cloned())
    }

    fn replace_auth(&self, auth: Option<Arc<session::NativeAuth>>) {
        if let Ok(mut saved) = self.auth.lock() {
            *saved = auth;
        }
    }

    async fn load_auth(&self) -> session::Result<Arc<session::NativeAuth>> {
        let auth = Arc::new(session::NativeAuth::new(self.config.clone())?);
        auth.check().await?;
        Ok(auth)
    }

    async fn wait_for_login(browser: &browser::Browser) -> session::Result<String> {
        let mut tracker = HideAfterLogin::default();
        let mut credential_attempts = 0;
        loop {
            tokio::time::sleep(HIDE_POLL).await;
            if browser.is_busy() {
                tracker.reset();
                continue;
            }
            if !browser.is_running().await {
                return Err(session::Error::LoginRequired(
                    "the sign-in window was closed before sign-in finished".into(),
                ));
            }
            let status = browser.status().await.ok();
            let has_msal_cache = match status.as_ref() {
                Some(status)
                    if status.running
                        && status.mode == browser::Mode::Visible
                        && status.login_state == browser::LoginState::NotRequired
                        && status
                            .tabs
                            .iter()
                            .any(|tab| tab.app == Some(browser::App::Teams) && !tab.parked) =>
                {
                    browser.teams_has_msal_cache().await
                }
                _ => false,
            };
            if tracker.step(status.as_ref(), has_msal_cache) {
                match browser
                    .teams_refresh_token(session::TEAMS_WEB_CLIENT_ID)
                    .await
                {
                    Ok(credential) => return Ok(credential),
                    Err(error) => {
                        credential_attempts += 1;
                        if credential_attempts >= CREDENTIAL_READ_ATTEMPTS {
                            return Err(session::Error::NativeAuth(format!(
                                "could not import the signed-in Teams credential: {error}"
                            )));
                        }
                    }
                }
            }
        }
    }

    async fn interactive_auth(&self) -> session::Result<Arc<session::NativeAuth>> {
        let browser = self.sign_in_browser()?;
        publish_sign_in(BrowserSignInState::Opening);
        if let Err(error) = browser.login_window().await {
            let _ = browser.stop().await;
            return Err(session::Error::NativeAuth(format!(
                "could not open the sign-in browser: {error}"
            )));
        }
        publish_sign_in(BrowserSignInState::AwaitingSignIn);
        let credential = Self::wait_for_login(&browser).await;
        if credential.is_ok() {
            publish_sign_in(BrowserSignInState::Finishing);
        }
        let stopped = browser.stop().await.map_err(|error| {
            session::Error::NativeAuth(format!(
                "sign-in finished, but the browser could not be closed: {error}"
            ))
        });
        let credential = credential?;
        stopped?;
        let auth = Arc::new(session::NativeAuth::from_refresh_token(
            self.config.clone(),
            credential,
        )?);
        auth.check().await?;
        Ok(auth)
    }

    async fn check_ready(&self) -> session::Result<()> {
        if let Some(auth) = self.saved_auth() {
            match auth.check().await {
                Ok(()) => return Ok(()),
                Err(session::Error::LoginRequired(_)) => self.replace_auth(None),
                Err(error) => return Err(error),
            }
        }

        let _initializing = self.initialize.lock().await;
        if let Some(auth) = self.saved_auth() {
            return auth.check().await;
        }

        let initial = self.load_auth().await;
        let auth = match initial {
            Ok(auth) => auth,
            Err(session::Error::LoginRequired(_)) => self.interactive_auth().await?,
            Err(error) => return Err(error),
        };
        self.replace_auth(Some(auth));
        publish_sign_in(BrowserSignInState::Idle);
        Ok(())
    }
}

impl session::Transport for NativeLoginTransport {
    fn check(&self) -> session::BoxFuture<'_, session::Result<()>> {
        Box::pin(async move {
            let result = self.check_ready().await;
            if let Err(error) = &result {
                match error {
                    session::Error::LoginRequired(_) => {
                        publish_sign_in(BrowserSignInState::Idle)
                    }
                    _ => publish_sign_in(BrowserSignInState::Failed(error.to_string())),
                }
            }
            result
        })
    }

    fn open(
        &self,
        _app: session::App,
        _with_events: bool,
    ) -> session::BoxFuture<'_, session::Result<Option<session::OpenedTab>>> {
        Box::pin(async { Ok(None) })
    }

    fn wake(&self, _app: session::App) -> session::BoxFuture<'_, session::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn diagnose(&self) -> session::BoxFuture<'_, session::Result<session::Diagnosis>> {
        Box::pin(async { Ok(session::Diagnosis::default()) })
    }

    fn native_auth(&self) -> Option<Arc<session::NativeAuth>> {
        self.saved_auth()
    }

    fn supports_tabs(&self) -> bool {
        false
    }
}

/// Starts strict native authentication. Browser discovery and launch happen only when the saved
/// refresh credential needs an interactive renewal.
pub fn start_native() -> Arc<dyn session::Transport> {
    let Some(base) = directories::BaseDirs::new() else {
        return Arc::new(session::NativeTransport::failed(
            "could not locate the native credential directory",
        ));
    };
    let refresh_token_file = base.data_dir().join(NATIVE_REFRESH_TOKEN_FILE);
    Arc::new(NativeLoginTransport::new(refresh_token_file))
}

pub fn start(endpoint: &str) -> Arc<dyn session::Transport> {
    let browser = match browser::Browser::detect() {
        Ok(browser) => browser,
        Err(error) => {
            log::warn!("Could not detect a supported sign-in browser: {error}");
            publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            return Arc::new(session::CdpTransport::new(endpoint));
        }
    };
    let _ = LINUX_BROWSER.set(browser.clone());
    let (init_done_tx, init_done_rx) = watch::channel(false);
    let headless_notify = Arc::new(Notify::new());
    let supervisor_browser = browser.clone();
    let supervisor_notify = headless_notify.clone();
    app_base::runtime::handle().spawn(async move {
        let first_run = !Path::new(&supervisor_browser.config().profile).exists();
        let outcome = if first_run {
            publish_sign_in(BrowserSignInState::Opening);
            supervisor_browser.login_window().await
        } else {
            supervisor_browser.ensure_running().await
        };
        match &outcome {
            Ok(()) if supervisor_browser.is_visible_managed() => {
                publish_sign_in(BrowserSignInState::AwaitingSignIn);
            }
            Ok(()) => {}
            Err(error) => {
                log::warn!("browser launch failed: {error}");
                publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            }
        }
        let _ = init_done_tx.send(true);
        app_base::runtime::handle().spawn(hide_after_login(
            supervisor_browser.clone(),
            supervisor_notify.clone(),
        ));
        let mut watchdog = browser::Watchdog::spawn(
            supervisor_browser.clone(),
            browser::WatchdogConfig::default(),
        );
        while let Some(event) = watchdog.events.recv().await {
            match event {
                browser::BrowserEvent::LoginRequired => {
                    if supervisor_browser.is_visible_managed() {
                        publish_sign_in(BrowserSignInState::AwaitingSignIn);
                        continue;
                    }
                    log::info!("Teams sign-in required: opening browser window");
                    publish_sign_in(BrowserSignInState::Opening);
                    match supervisor_browser.login_window().await {
                        Ok(()) => publish_sign_in(BrowserSignInState::AwaitingSignIn),
                        Err(error) => {
                            log::error!("Failed to open login window: {error}");
                            publish_sign_in(BrowserSignInState::Failed(error.to_string()));
                        }
                    }
                }
                browser::BrowserEvent::Hung => {
                    log::warn!("Chrome renderer hung, watchdog restarting");
                }
                browser::BrowserEvent::Died => {
                    log::info!("Chrome process closed, watchdog will restart headless if needed");
                }
                browser::BrowserEvent::Started | browser::BrowserEvent::Restarted => {
                    log::info!("Chrome browser ready on port");
                    if supervisor_browser.is_visible_managed() {
                        publish_sign_in(BrowserSignInState::AwaitingSignIn);
                    }
                    supervisor_notify.notify_waiters();
                }
            }
        }
    });
    let park = ParkClock::new();
    // Parking checks the MSAL cache over CDP, so it runs with a Chromium browser only.
    // macOS keeps the clock but never parks: nobody has run it there yet.
    if cfg!(target_os = "linux") && browser.bidi().is_none() {
        linux_park::spawn(park.clone(), browser.clone(), endpoint);
    }
    Arc::new(LinuxTransport::new(
        endpoint,
        browser,
        init_done_rx,
        headless_notify,
        park,
    ))
}

pub fn stop() {
    let Some(browser) = LINUX_BROWSER.get().cloned() else {
        return;
    };
    let _ = app_base::runtime::handle().block_on(async move {
        let _ = tokio::time::timeout(STOP_TIMEOUT, browser.stop()).await;
    });
}

struct LinuxTransport {
    inner: Arc<dyn session::Transport>,
    browser: browser::Browser,
    init_done: watch::Receiver<bool>,
    headless_notify: Arc<Notify>,
    park: Arc<ParkClock>,
}

impl LinuxTransport {
    fn new(
        endpoint: &str,
        browser: browser::Browser,
        init_done: watch::Receiver<bool>,
        headless_notify: Arc<Notify>,
        park: Arc<ParkClock>,
    ) -> Self {
        LinuxTransport {
            inner: browser::session_transport(&browser, endpoint),
            browser,
            init_done,
            headless_notify,
            park,
        }
    }

    async fn wait_if_busy(&self) {
        let deadline = tokio::time::Instant::now() + RELAUNCH_WAIT;
        while self.browser.is_busy() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(POLL_STEP).await;
        }
    }
}

impl session::Transport for LinuxTransport {
    fn check(&self) -> session::BoxFuture<'_, session::Result<()>> {
        Box::pin(async move {
            let mut init_done = self.init_done.clone();
            let _ = tokio::time::timeout(STARTUP_WAIT, init_done.wait_for(|done| *done)).await;
            let mut down_since: Option<tokio::time::Instant> = None;
            loop {
                let busy = self.browser.is_busy();
                let visible_login = self.browser.is_visible_managed();
                if !busy && !visible_login && self.inner.check().await.is_ok() {
                    return Ok(());
                }
                if visible_login && (busy || self.browser.is_running().await) {
                    down_since = None;
                    tokio::select! {
                        _ = self.headless_notify.notified() => {}
                        _ = tokio::time::sleep(CHECK_WAIT) => {}
                    }
                    continue;
                }
                let started = *down_since.get_or_insert_with(tokio::time::Instant::now);
                if started.elapsed() >= STARTUP_WAIT {
                    return self.inner.check().await;
                }
                tokio::time::sleep(CHECK_WAIT).await;
            }
        })
    }

    fn open(
        &self,
        app: session::App,
        with_events: bool,
    ) -> session::BoxFuture<'_, session::Result<Option<session::OpenedTab>>> {
        Box::pin(async move {
            self.wait_if_busy().await;
            if self.browser.is_visible_managed() {
                return Ok(None);
            }
            self.park.touch();
            self.inner.open(app, with_events).await
        })
    }

    fn wake(&self, app: session::App) -> session::BoxFuture<'_, session::Result<()>> {
        Box::pin(async move {
            self.wait_if_busy().await;
            if self.browser.is_visible_managed() {
                return Ok(());
            }
            self.park.touch();
            self.inner.wake(app).await
        })
    }

    fn diagnose(&self) -> session::BoxFuture<'_, session::Result<session::Diagnosis>> {
        Box::pin(async move {
            self.wait_if_busy().await;
            if self.browser.is_visible_managed() {
                return Ok(session::Diagnosis {
                    login_pending: true,
                    app_tab_open: false,
                });
            }
            self.inner.diagnose().await
        })
    }

    fn hold(&self, app: session::App, held: bool) {
        self.park.hold(held);
        self.inner.hold(app, held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(app: Option<browser::App>, login: bool, parked: bool) -> browser::Tab {
        browser::Tab {
            id: "1".into(),
            app,
            host: Some("teams.cloud.microsoft".into()),
            login,
            parked,
        }
    }

    fn status(
        mode: browser::Mode,
        login_state: browser::LoginState,
        tabs: Vec<browser::Tab>,
    ) -> browser::Status {
        browser::Status {
            running: true,
            mode,
            tabs,
            login_state,
        }
    }

    #[test]
    fn waits_for_stable_msal_cache_before_hiding() {
        let mut tracker = HideAfterLogin::default();
        let login = status(
            browser::Mode::Visible,
            browser::LoginState::Required,
            vec![tab(None, true, false)],
        );
        assert!(!tracker.step(Some(&login), false));

        // OAuth redirected back to Teams, but MSAL has not saved tokens yet.
        let pre_token = status(
            browser::Mode::Visible,
            browser::LoginState::NotRequired,
            vec![tab(Some(browser::App::Teams), false, false)],
        );
        assert!(!tracker.step(Some(&pre_token), false));
        assert!(!tracker.step(Some(&pre_token), true));

        // Transient login/poll blip resets stability counter.
        assert!(!tracker.step(None, false));

        for _ in 0..HIDE_STABLE_CHECKS - 1 {
            assert!(!tracker.step(Some(&pre_token), true));
        }
        assert!(tracker.step(Some(&pre_token), true));
    }

    #[test]
    fn never_hides_when_login_tab_still_open() {
        let mut tracker = HideAfterLogin::default();
        let stuck_login = status(
            browser::Mode::Visible,
            browser::LoginState::Required,
            vec![
                tab(Some(browser::App::Teams), false, false),
                tab(None, true, false),
            ],
        );
        for _ in 0..HIDE_STABLE_CHECKS + 2 {
            assert!(!tracker.step(Some(&stuck_login), true));
        }
    }
}
