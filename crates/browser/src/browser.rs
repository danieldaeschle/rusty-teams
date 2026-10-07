use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::error::BrowserError;
use crate::http;
use crate::launch::{LaunchRequest, Launcher, SystemLauncher};
use crate::targets::{self, APPS, App, LoginState, Mode, Tab, VersionInfo};

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const OPEN_TAB_TIMEOUT: Duration = Duration::from_secs(10);
const CDP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub running: bool,
    pub mode: Mode,
    pub tabs: Vec<Tab>,
    pub login_state: LoginState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererState {
    Ok,
    Hung,
    NoTab,
}

impl RendererState {
    pub fn label(self) -> &'static str {
        match self {
            RendererState::Ok => "ok",
            RendererState::Hung => "hung",
            RendererState::NoTab => "no tab",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Timeouts {
    pub start: Duration,
    pub stop: Duration,
    pub probe: Duration,
    pub poll: Duration,
    pub profile_release: Duration,
    pub jitter_min: Duration,
    pub jitter_max: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            start: Duration::from_secs(20),
            stop: Duration::from_secs(15),
            probe: Duration::from_secs(5),
            poll: Duration::from_millis(500),
            profile_release: Duration::from_secs(2),
            jitter_min: Duration::from_millis(500),
            jitter_max: Duration::from_secs(2),
        }
    }
}

struct Inner {
    config: Config,
    launcher: Arc<dyn Launcher>,
    timeouts: Timeouts,
    operation: Mutex<()>,
    wanted: AtomicBool,
}

#[derive(Clone)]
pub struct Browser {
    inner: Arc<Inner>,
}

pub(crate) fn start_urls_for(headless: bool) -> Vec<String> {
    let urls = APPS.iter().map(|app| app.start_url().to_owned());
    if headless { urls.take(1).collect() } else { urls.collect() }
}

pub(crate) fn jitter_between(min: Duration, max: Duration, random: u64) -> Duration {
    let span = max.saturating_sub(min).as_millis() as u64;
    if span == 0 { min } else { min + Duration::from_millis(random % (span + 1)) }
}

fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher, RandomState};
    RandomState::new().build_hasher().finish()
}

impl Browser {
    pub fn detect() -> Result<Browser, BrowserError> {
        Ok(Browser::new(Config::detect()?))
    }

    pub fn new(config: Config) -> Browser {
        let launcher = Arc::new(SystemLauncher::new(config.clone()));
        Browser::with_launcher(config, launcher, Timeouts::default())
    }

    pub fn with_launcher(config: Config, launcher: Arc<dyn Launcher>, timeouts: Timeouts) -> Browser {
        Browser {
            inner: Arc::new(Inner {
                config,
                launcher,
                timeouts,
                operation: Mutex::new(()),
                wanted: AtomicBool::new(false),
            }),
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub async fn status(&self) -> Result<Status, BrowserError> {
        let Some(_) = self.version().await else {
            return Ok(Status { running: false, mode: Mode::Visible, tabs: Vec::new(), login_state: LoginState::Unknown });
        };
        let tabs = self.tabs().await?;
        Ok(Status { running: true, mode: self.mode(), login_state: targets::login_state(&tabs), tabs })
    }

    pub async fn is_running(&self) -> bool {
        self.version().await.is_some()
    }

    pub async fn probe_renderer(&self) -> RendererState {
        let Ok(tabs) = self.tabs().await else { return RendererState::NoTab };
        let Some(tab) = targets::app_tab(&tabs, App::Teams) else { return RendererState::NoTab };
        let url = format!("ws://127.0.0.1:{}/devtools/page/{}", self.inner.config.port, tab.id);
        let params = json!({ "expression": "1+1" });
        match cdp_call(&url, "Runtime.evaluate", params, self.inner.timeouts.probe).await {
            Ok(_) => RendererState::Ok,
            Err(_) => RendererState::Hung,
        }
    }

    pub(crate) async fn restart_hung(&self) -> Result<(), BrowserError> {
        let _guard = self.inner.operation.lock().await;
        let headless = self.mode() == Mode::Headless;
        self.stop_locked(true).await?;
        self.launch_locked(headless).await
    }

    pub async fn tabs(&self) -> Result<Vec<Tab>, BrowserError> {
        let response = http::request(self.inner.config.port, "GET", "/json/list", HTTP_TIMEOUT).await?;
        targets::parse_tabs(&response.body)
    }

    pub async fn ensure_running(&self) -> Result<(), BrowserError> {
        let _guard = self.inner.operation.lock().await;
        self.inner.wanted.store(true, Ordering::SeqCst);
        if self.is_running().await {
            return self.ensure_tabs_locked().await;
        }
        match self.launch_locked(true).await {
            Ok(()) => {}
            Err(BrowserError::PortBusy(_)) => self.ensure_tabs_locked().await?,
            Err(error) => return Err(error),
        }
        self.dedupe_app_tabs_locked().await;
        Ok(())
    }

    pub async fn wake(&self, app: App) -> Result<(), BrowserError> {
        let _guard = self.inner.operation.lock().await;
        let tabs = self.tabs().await?;
        let candidates = tabs.iter().filter(|tab| tab.app == Some(app));
        let Some(tab) = candidates.clone().find(|tab| tab.parked).or_else(|| candidates.clone().next()) else {
            return self.open_tab(app).await;
        };
        let url = format!("ws://127.0.0.1:{}/devtools/page/{}", self.inner.config.port, tab.id);
        cdp_call(&url, "Page.navigate", json!({ "url": app.start_url() }), CDP_TIMEOUT).await.map(|_| ())
    }

    pub async fn ensure_tabs(&self) -> Result<(), BrowserError> {
        let _guard = self.inner.operation.lock().await;
        self.ensure_tabs_locked().await
    }

    pub async fn login_window(&self) -> Result<(), BrowserError> {
        self.relaunch(false).await
    }

    pub async fn headless(&self) -> Result<(), BrowserError> {
        self.relaunch(true).await
    }

    pub async fn stop(&self) -> Result<(), BrowserError> {
        let _guard = self.inner.operation.lock().await;
        self.inner.wanted.store(false, Ordering::SeqCst);
        self.stop_locked(false).await
    }

    pub(crate) fn is_wanted(&self) -> bool {
        self.inner.wanted.load(Ordering::SeqCst)
    }

    pub(crate) fn set_wanted(&self, wanted: bool) {
        self.inner.wanted.store(wanted, Ordering::SeqCst);
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.inner.operation.try_lock().is_err()
    }

    async fn relaunch(&self, headless: bool) -> Result<(), BrowserError> {
        let _guard = self.inner.operation.lock().await;
        self.inner.wanted.store(true, Ordering::SeqCst);
        self.stop_locked(false).await?;
        self.launch_locked(headless).await
    }

    async fn version(&self) -> Option<VersionInfo> {
        let response = http::request(self.inner.config.port, "GET", "/json/version", Duration::from_secs(2))
            .await
            .ok()?;
        targets::parse_version(&response.body).ok()
    }

    fn mode(&self) -> Mode {
        std::fs::read_to_string(&self.inner.config.mode_file)
            .map(|content| Mode::from_mode_file(&content))
            .unwrap_or(Mode::Visible)
    }

    async fn ensure_tabs_locked(&self) -> Result<(), BrowserError> {
        let tabs = self.tabs().await?;
        let login_open = tabs.iter().any(|tab| tab.login);
        for app in APPS {
            if targets::app_tab(&tabs, app).is_none() && !login_open {
                self.open_tab(app).await?;
            }
        }
        Ok(())
    }

    pub(crate) fn jitter(&self) -> Duration {
        let timeouts = &self.inner.timeouts;
        jitter_between(timeouts.jitter_min, timeouts.jitter_max, random_u64())
    }

    async fn dedupe_app_tabs_locked(&self) {
        let _ = self.try_dedupe_app_tabs().await;
    }

    async fn try_dedupe_app_tabs(&self) -> Result<(), BrowserError> {
        let tabs = self.tabs().await?;
        let info = self.version().await.ok_or_else(|| BrowserError::NoBrowser(self.inner.config.port.to_string()))?;
        let result = cdp_call(&info.websocket_url, "Target.getTargets", json!({}), CDP_TIMEOUT).await?;
        let attached = targets::attached_target_ids(&result);
        for id in targets::duplicate_app_tab_ids(&tabs, &attached) {
            let path = format!("/json/close/{id}");
            http::request(self.inner.config.port, "GET", &path, HTTP_TIMEOUT).await?;
        }
        Ok(())
    }

    async fn open_tab(&self, app: App) -> Result<(), BrowserError> {
        let path = format!("/json/new?{}", app.start_url());
        let response = http::request(self.inner.config.port, "PUT", &path, OPEN_TAB_TIMEOUT).await?;
        if response.status != 200 {
            return Err(BrowserError::Http(format!("PUT /json/new returned {}", response.status)));
        }
        Ok(())
    }

    async fn launch_locked(&self, headless: bool) -> Result<(), BrowserError> {
        let config = &self.inner.config;
        tokio::time::sleep(self.jitter()).await;
        if self.is_running().await {
            return Err(BrowserError::PortBusy(config.port));
        }
        let request = LaunchRequest { headless, start_urls: start_urls_for(headless) };
        self.inner.launcher.spawn(&request)?;
        let mode = if headless { Mode::Headless } else { Mode::Visible };
        if let Some(parent) = config.mode_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config.mode_file, mode.file_content())?;
        self.wait_until_running().await?;
        if headless {
            for app in APPS.into_iter().skip(1) {
                self.open_tab(app).await?;
            }
        }
        Ok(())
    }

    async fn wait_until_stopped(&self) -> bool {
        let timeouts = &self.inner.timeouts;
        let deadline = tokio::time::Instant::now() + timeouts.stop;
        while tokio::time::Instant::now() < deadline && self.is_running().await {
            tokio::time::sleep(timeouts.poll).await;
        }
        !self.is_running().await
    }

    async fn wait_until_running(&self) -> Result<(), BrowserError> {
        let timeouts = &self.inner.timeouts;
        let deadline = tokio::time::Instant::now() + timeouts.start;
        while tokio::time::Instant::now() < deadline {
            if self.is_running().await {
                return Ok(());
            }
            tokio::time::sleep(timeouts.poll).await;
        }
        Err(BrowserError::StartTimeout { port: self.inner.config.port, seconds: timeouts.start.as_secs() })
    }

    async fn stop_locked(&self, kill_fallback: bool) -> Result<(), BrowserError> {
        let Some(info) = self.version().await else { return Ok(()) };
        close_over_cdp(&info.websocket_url).await;
        let timeouts = &self.inner.timeouts;
        if !self.wait_until_stopped().await && kill_fallback {
            let launcher = self.inner.launcher.clone();
            let port = self.inner.config.port;
            let _ = tokio::task::spawn_blocking(move || launcher.kill_listener(port)).await;
            self.wait_until_stopped().await;
        }
        if self.is_running().await {
            return Err(BrowserError::StopTimeout {
                port: self.inner.config.port,
                seconds: timeouts.stop.as_secs(),
            });
        }
        // the profile lock is released a moment after the debugging port closes
        tokio::time::sleep(timeouts.profile_release).await;
        Ok(())
    }
}

async fn cdp_call(
    websocket_url: &str,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, BrowserError> {
    let exchange = async {
        let (mut socket, _) = tokio_tungstenite::connect_async(websocket_url)
            .await
            .map_err(|error| BrowserError::Cdp(error.to_string()))?;
        let request = json!({ "id": 1, "method": method, "params": params });
        socket
            .send(Message::text(request.to_string()))
            .await
            .map_err(|error| BrowserError::Cdp(error.to_string()))?;
        while let Some(message) = socket.next().await {
            let Message::Text(text) = message.map_err(|error| BrowserError::Cdp(error.to_string()))? else {
                continue;
            };
            let Ok(reply) = serde_json::from_str::<Value>(&text) else { continue };
            if reply["id"] != 1 {
                continue;
            }
            if let Some(error) = reply.get("error") {
                return Err(BrowserError::Cdp(format!("{method}: {}", error["message"].as_str().unwrap_or("error"))));
            }
            return Ok(reply["result"].clone());
        }
        Err(BrowserError::Cdp(format!("{method}: connection closed")))
    };
    tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| BrowserError::Cdp(format!("{method} timed out")))?
}

async fn close_over_cdp(websocket_url: &str) {
    let Ok(Ok((mut socket, _))) =
        tokio::time::timeout(Duration::from_secs(3), tokio_tungstenite::connect_async(websocket_url)).await
    else {
        return;
    };
    let _ = socket.send(Message::text(r#"{"id":1,"method":"Browser.close"}"#)).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), futures_util::StreamExt::next(&mut socket)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_in_range() {
        let (min, max) = (Duration::from_millis(500), Duration::from_secs(2));
        assert_eq!(jitter_between(min, max, 0), min);
        assert_eq!(jitter_between(min, max, 1500), max);
        assert_eq!(jitter_between(min, max, 1501), Duration::from_millis(500));
        assert_eq!(jitter_between(min, min, 99), min);
        assert!((0..50).all(|seed| (min..=max).contains(&jitter_between(min, max, random_u64().wrapping_add(seed)))));
    }

    #[test]
    fn headless_gets_one_start_url_visible_gets_both() {
        assert_eq!(start_urls_for(true), ["https://teams.cloud.microsoft/"]);
        assert_eq!(
            start_urls_for(false),
            ["https://teams.cloud.microsoft/", "https://outlook.office.com/mail/"]
        );
    }
}
