//! Linux with a Chromium browser: parks the hidden Teams tab on a static page of its own
//! origin, like the WebView2 host does on Windows. The running Teams web app holds close to a
//! gigabyte; the parked page keeps the MSAL cache reachable for token refreshes at a fraction
//! of that. The session wakes the tab when it needs the running app again
//! (`Transport::wake`), and this parks it once more.
//!
//! The quiet period counts from the last time the session opened or woke the tab, not from
//! the last command: commands go over a connection the session keeps, which this transport
//! does not see. Windows counts commands in flight (`webview::park`); here a page-run call
//! that overlaps the navigation loses its page connection (a read retries, a write fails).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use session::Transport as _;

const PARK_DELAY: Duration = Duration::from_secs(60);
const PARK_POLL: Duration = Duration::from_secs(15);

/// When the session last opened or woke the tab, and who needs it to stay loaded.
pub struct ParkClock {
    holds: AtomicUsize,
    last_use: Mutex<Instant>,
}

impl ParkClock {
    pub fn new() -> Arc<Self> {
        Arc::new(ParkClock {
            holds: AtomicUsize::new(0),
            last_use: Mutex::new(Instant::now()),
        })
    }

    /// The tab was just opened or woken: leave it running for the quiet period.
    pub fn touch(&self) {
        if let Ok(mut last_use) = self.last_use.lock() {
            *last_use = Instant::now();
        }
    }

    /// A call holds the tab: a navigation would cut its page sockets.
    pub fn hold(&self, held: bool) {
        if held {
            self.holds.fetch_add(1, Ordering::SeqCst);
        } else {
            let _ = self
                .holds
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |holds| {
                    Some(holds.saturating_sub(1))
                });
        }
    }

    fn due(&self, now: Instant) -> bool {
        self.holds.load(Ordering::SeqCst) == 0
            && self
                .last_use
                .lock()
                .is_ok_and(|last_use| now.duration_since(*last_use) >= PARK_DELAY)
    }
}

pub fn spawn(clock: Arc<ParkClock>, browser: browser::Browser, endpoint: &str) {
    let transport = session::CdpTransport::new(endpoint);
    app_base::runtime::handle().spawn(async move {
        loop {
            tokio::time::sleep(PARK_POLL).await;
            if !clock.due(Instant::now()) || browser.is_busy() || browser.is_visible_managed() {
                continue;
            }
            // False for a parked tab, a login page and a Teams that has not signed in yet.
            if !browser.teams_has_msal_cache().await {
                continue;
            }
            match park(&transport).await {
                Ok(true) => log::info!("Teams tab parked"),
                Ok(false) => {}
                Err(error) => log::warn!("parking the Teams tab failed: {error}"),
            }
        }
    });
}

async fn park(transport: &session::CdpTransport) -> session::Result<bool> {
    let app = session::App::Teams;
    let Some((control, _)) = transport.open(app, false).await? else {
        return Ok(false);
    };
    control
        .call("Page.navigate", json!({ "url": app.park_url() }))
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock_used(ago: Duration) -> Arc<ParkClock> {
        let clock = ParkClock::new();
        *clock.last_use.lock().unwrap() = Instant::now() - ago;
        clock
    }

    #[test]
    fn a_tab_parks_only_after_the_quiet_period() {
        assert!(!clock_used(Duration::from_secs(5)).due(Instant::now()));
        assert!(clock_used(PARK_DELAY).due(Instant::now()));
    }

    #[test]
    fn a_wake_restarts_the_quiet_period() {
        let clock = clock_used(PARK_DELAY);
        clock.touch();
        assert!(!clock.due(Instant::now()));
    }

    #[test]
    fn a_held_tab_never_parks_and_releases_are_counted() {
        let clock = clock_used(PARK_DELAY);
        clock.hold(true);
        clock.hold(true);
        clock.hold(false);
        assert!(!clock.due(Instant::now()));
        clock.hold(false);
        assert!(clock.due(Instant::now()));
        clock.hold(false);
        assert!(clock.due(Instant::now()));
    }
}
