use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::{Notify, mpsc, watch};

use crate::backend::BackendEvent;
use crate::browser_sign_in::BrowserSignInState;
use crate::linux_park::{self, ParkClock};

const STARTUP_WAIT: Duration = Duration::from_secs(20);
const RELAUNCH_WAIT: Duration = Duration::from_secs(5);
const POLL_STEP: Duration = Duration::from_millis(100);
const CHECK_WAIT: Duration = Duration::from_millis(250);
const HIDE_POLL: Duration = Duration::from_millis(500);
const HIDE_STABLE_CHECKS: u32 = 4;
/// A refusal sooner than this after a guest window signed in means the window did not help: leave instead.
const GUEST_RENEW_GAP: Duration = Duration::from_secs(10 * 60);
static GUEST_SIGNED_IN_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);
/// Guest tenants whose login pages are stale in this run, because the window shows the home org.
static STALE_GUEST_LOGINS: OnceLock<Vec<String>> = OnceLock::new();

/// A guest org's sign-in page left open (e.g. after leaving that org) would ask the home org to sign in,
/// so it is closed whenever the browser comes up. Call before `start`.
pub fn close_guest_logins_on_start(tenants: Vec<String>) {
    let _ = STALE_GUEST_LOGINS.set(tenants);
}

fn close_stale_guest_logins(browser: &browser::Browser) {
    let Some(tenants) = STALE_GUEST_LOGINS
        .get()
        .filter(|tenants| !tenants.is_empty())
    else {
        return;
    };
    let browser = browser.clone();
    app_base::runtime::handle().spawn(async move {
        let closed = browser.close_tenant_login_tabs(tenants).await;
        if closed > 0 {
            log::info!("closed {closed} stale guest org sign-in page(s)");
        }
    });
}
// MSAL key budget (500 ms) + CDP close (up to 3 s) + profile release (2 s) must fit.
const STOP_TIMEOUT: Duration = Duration::from_secs(6);

static LINUX_BROWSER: OnceLock<browser::Browser> = OnceLock::new();
static LIVE_RETRY: OnceLock<Mutex<Option<Arc<Notify>>>> = OnceLock::new();
static UI_EVENTS: OnceLock<mpsc::UnboundedSender<BackendEvent>> = OnceLock::new();
static LOGIN_LAUNCH: AtomicU8 = AtomicU8::new(LoginLaunch::Idle as u8);
/// Launches of the guest org's sign-in window per run; a window that showed uses them all up.
static GUEST_SIGN_IN_TRIES: AtomicU8 = AtomicU8::new(0);
const GUEST_SIGN_IN_MAX_TRIES: u8 = 2;
/// Tenant of the guest sign-in in the visible window: it hides only once that tenant has a token.
static GUEST_SIGN_IN_TENANT: Mutex<Option<String>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum LoginLaunch {
    Idle = 0,
    Opening = 1,
    Shown = 2,
}

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

/// A finished launch that is not showing a login window must not leave the
/// splash on "opening the browser". A visible window still waits for sign-in.
fn sign_in_after_browser_ready(visible_login: bool) -> BrowserSignInState {
    if visible_login {
        BrowserSignInState::AwaitingSignIn
    } else {
        BrowserSignInState::Idle
    }
}

pub fn note_backend_event(event: &BackendEvent) {
    if let BackendEvent::LiveRetry(retry) = event
        && let Ok(mut slot) = live_retry_slot().lock()
    {
        *slot = Some(retry.clone());
    }
    match event {
        BackendEvent::Connection(crate::backend::ConnectionState::Online) => {
            LOGIN_LAUNCH.store(LoginLaunch::Idle as u8, Ordering::SeqCst);
            publish_sign_in(BrowserSignInState::Idle);
        }
        BackendEvent::Connection(crate::backend::ConnectionState::LoginRequired) => {
            request_login_window(false);
        }
        _ => {}
    }
}

/// Opens the managed browser for Microsoft sign-in when it is hidden.
/// `force` is the banner action: it may open the window again after the user closed it.
/// An automatic request opens it once and does not reopen a closed window.
pub fn request_login_window(force: bool) {
    let Some(browser) = LINUX_BROWSER.get().cloned() else {
        return;
    };
    let visible = browser.is_visible_managed();
    let busy = browser.is_busy();
    let current = login_launch();
    let Some(next) = login_launch_decision(current, visible, busy, force) else {
        return;
    };
    if LOGIN_LAUNCH
        .compare_exchange(
            current as u8,
            next as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }
    publish_sign_in(BrowserSignInState::Opening);
    app_base::runtime::handle().spawn(async move {
        match browser.login_window().await {
            Ok(()) => {
                LOGIN_LAUNCH.store(LoginLaunch::Shown as u8, Ordering::SeqCst);
                publish_sign_in(BrowserSignInState::AwaitingSignIn);
            }
            Err(error) => {
                log::warn!("browser login window failed: {error}");
                LOGIN_LAUNCH.store(LoginLaunch::Idle as u8, Ordering::SeqCst);
                publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            }
        }
    });
}

/// Opens the sign-in window on the guest org's Teams, where its tenant can ask for MFA: one window per
/// sign-in, with a second launch only when the first failed. A window that signed in allows another one
/// after GUEST_RENEW_GAP, since Teams web may renew the guest tokens only while it shows that tenant.
/// `false` when that was used up or cannot be shown, so the caller leaves the guest org instead.
/// A window that is showing or opening is another sign-in in progress: wait for it, the refusal comes again.
pub fn request_guest_sign_in(tenant_id: &str) -> bool {
    let (Some(browser), Some(url)) = (
        LINUX_BROWSER.get().cloned(),
        browser::tenant_start_url(tenant_id),
    ) else {
        return false;
    };
    if browser.is_visible_managed() || browser.is_busy() || login_launch() == LoginLaunch::Opening {
        return true;
    }
    // Taken, not read: one sign-in buys one more window, so a refused renewal cannot reopen it forever.
    let renew = GUEST_SIGNED_IN_AT.lock().ok().is_some_and(|mut slot| {
        slot.take_if(|signed_in| signed_in.elapsed() >= GUEST_RENEW_GAP)
            .is_some()
    });
    if renew {
        GUEST_SIGN_IN_TRIES.store(0, Ordering::SeqCst);
    }
    let tries = GUEST_SIGN_IN_TRIES.load(Ordering::SeqCst);
    if tries >= GUEST_SIGN_IN_MAX_TRIES {
        return false;
    }
    GUEST_SIGN_IN_TRIES.store(tries + 1, Ordering::SeqCst);
    let tenant = tenant_id.to_owned();
    LOGIN_LAUNCH.store(LoginLaunch::Opening as u8, Ordering::SeqCst);
    publish_sign_in(BrowserSignInState::Opening);
    app_base::runtime::handle().spawn(async move {
        match browser.login_window_at(vec![url]).await {
            Ok(()) => {
                GUEST_SIGN_IN_TRIES.store(GUEST_SIGN_IN_MAX_TRIES, Ordering::SeqCst);
                LOGIN_LAUNCH.store(LoginLaunch::Shown as u8, Ordering::SeqCst);
                set_guest_sign_in_tenant(Some(tenant));
                publish_sign_in(BrowserSignInState::AwaitingSignIn);
            }
            Err(error) => {
                log::warn!("guest org sign-in window failed: {error}");
                LOGIN_LAUNCH.store(LoginLaunch::Idle as u8, Ordering::SeqCst);
                publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            }
        }
    });
    true
}

fn set_guest_sign_in_tenant(tenant: Option<String>) {
    if let Ok(mut slot) = GUEST_SIGN_IN_TENANT.lock() {
        *slot = tenant;
    }
}

fn guest_sign_in_tenant() -> Option<String> {
    GUEST_SIGN_IN_TENANT
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

fn login_launch() -> LoginLaunch {
    match LOGIN_LAUNCH.load(Ordering::SeqCst) {
        value if value == LoginLaunch::Opening as u8 => LoginLaunch::Opening,
        value if value == LoginLaunch::Shown as u8 => LoginLaunch::Shown,
        _ => LoginLaunch::Idle,
    }
}

/// `Some(Opening)` means start the visible window. `None` means leave it alone.
fn login_launch_decision(
    current: LoginLaunch,
    visible: bool,
    busy: bool,
    force: bool,
) -> Option<LoginLaunch> {
    if visible || busy {
        return None;
    }
    match current {
        LoginLaunch::Opening => None,
        LoginLaunch::Shown if !force => None,
        LoginLaunch::Idle | LoginLaunch::Shown => Some(LoginLaunch::Opening),
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

    fn step(&mut self, status: Option<&browser::Status>, session_ready: bool) -> bool {
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
        if !teams_loaded || !session_ready {
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

/// Moves a managed visible Chrome window back to headless once Teams sign-in completes, matching WebView2 on Windows.
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
            set_guest_sign_in_tenant(None);
            continue;
        }
        let status = browser.status().await.ok();
        let guest = guest_sign_in_tenant();
        let teams_loaded = status.as_ref().is_some_and(|status| {
            status.running
                && status.mode == browser::Mode::Visible
                && status.login_state == browser::LoginState::NotRequired
                && status
                    .tabs
                    .iter()
                    .any(|tab| tab.app == Some(browser::App::Teams) && !tab.parked)
        });
        // During a guest sign-in only a fresh token of the guest tenant counts.
        let session_ready = teams_loaded && browser.teams_sign_in_ready(guest.as_deref()).await;
        if tracker.step(status.as_ref(), session_ready) {
            if guest.is_some()
                && let Ok(mut slot) = GUEST_SIGNED_IN_AT.lock()
            {
                *slot = Some(std::time::Instant::now());
            }
            log::info!("Teams sign-in finished: moving Chrome to the background");
            publish_sign_in(BrowserSignInState::Finishing);
            if let Err(error) = browser.headless().await {
                log::error!("Failed to switch Chrome to headless: {error}");
                publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            } else {
                headless_notify.notify_waiters();
                trigger_live_retry();
            }
        }
    }
}

pub fn start(endpoint: &str) -> Arc<dyn session::Transport> {
    let browser = match browser::Browser::detect() {
        Ok(browser) => browser,
        Err(error) => {
            log::warn!("Could not detect Chrome/Chromium on system: {error}");
            publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            return Arc::new(session::CdpTransport::new(endpoint));
        }
    };
    let _ = LINUX_BROWSER.set(browser.clone());
    let (init_done_tx, init_done_rx) = watch::channel(false);
    let headless_notify = Arc::new(Notify::new());
    let supervisor_browser = browser.clone();
    let supervisor_notify = headless_notify.clone();
    publish_sign_in(BrowserSignInState::Opening);
    app_base::runtime::handle().spawn(async move {
        let first_run = !Path::new(&supervisor_browser.config().profile).exists();
        let outcome = if first_run {
            publish_sign_in(BrowserSignInState::Opening);
            supervisor_browser.login_window().await
        } else {
            supervisor_browser.ensure_running().await
        };
        match &outcome {
            Ok(()) => {
                publish_sign_in(sign_in_after_browser_ready(
                    supervisor_browser.is_visible_managed(),
                ));
            }
            Err(error) => {
                log::warn!("browser launch failed: {error}");
                publish_sign_in(BrowserSignInState::Failed(error.to_string()));
            }
        }
        close_stale_guest_logins(&supervisor_browser);
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
                    close_stale_guest_logins(&supervisor_browser);
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
    fn login_launch_decision_opens_once_and_the_action_can_reopen() {
        assert_eq!(
            login_launch_decision(LoginLaunch::Idle, false, false, false),
            Some(LoginLaunch::Opening)
        );
        assert_eq!(
            login_launch_decision(LoginLaunch::Opening, false, false, true),
            None
        );
        assert_eq!(
            login_launch_decision(LoginLaunch::Shown, true, false, true),
            None
        );
        assert_eq!(
            login_launch_decision(LoginLaunch::Shown, false, false, false),
            None
        );
        assert_eq!(
            login_launch_decision(LoginLaunch::Shown, false, false, true),
            Some(LoginLaunch::Opening)
        );
        assert_eq!(
            login_launch_decision(LoginLaunch::Idle, false, true, true),
            None
        );
    }

    #[test]
    fn sign_in_after_browser_ready_is_idle_when_headless() {
        assert_eq!(sign_in_after_browser_ready(false), BrowserSignInState::Idle);
        assert_eq!(
            sign_in_after_browser_ready(true),
            BrowserSignInState::AwaitingSignIn
        );
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

    #[test]
    fn key_names_without_a_usable_token_keep_the_window_open() {
        let mut tracker = HideAfterLogin::default();
        let teams = status(
            browser::Mode::Visible,
            browser::LoginState::NotRequired,
            vec![tab(Some(browser::App::Teams), false, false)],
        );
        for _ in 0..HIDE_STABLE_CHECKS + 2 {
            assert!(!tracker.step(Some(&teams), false));
        }
    }

    #[test]
    fn stop_timeout_covers_browser_shutdown() {
        let timeouts = browser::Timeouts::default();
        let worst_case = timeouts.msal_key + timeouts.profile_release + Duration::from_secs(3);
        assert!(STOP_TIMEOUT >= worst_case);
    }
}
