use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::browser::{Browser, RendererState};
use crate::targets::LoginState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserEvent {
    Started,
    Died,
    Restarted,
    LoginRequired,
    Hung,
}

#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    pub poll_interval: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub died_wait: Duration,
    pub probe_interval: Duration,
    pub probe_failures: u32,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        WatchdogConfig {
            poll_interval: Duration::from_secs(5),
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(60),
            died_wait: Duration::from_secs(2),
            probe_interval: Duration::from_secs(30),
            probe_failures: 2,
        }
    }
}

pub struct Watchdog {
    pub events: mpsc::Receiver<BrowserEvent>,
    task: JoinHandle<()>,
}

impl Watchdog {
    pub fn spawn(browser: Browser, config: WatchdogConfig) -> Watchdog {
        let (sender, events) = mpsc::channel(32);
        browser.set_wanted(true);
        let task = tokio::spawn(run(browser, config, sender));
        Watchdog { events, task }
    }

    pub fn stop(self) {
        self.task.abort();
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) fn next_backoff(current: Duration, max: Duration) -> Duration {
    (current * 2).min(max)
}

async fn run(browser: Browser, config: WatchdogConfig, sender: mpsc::Sender<BrowserEvent>) {
    let mut backoff = config.initial_backoff;
    let mut alive = false;
    let mut died = false;
    let mut login_announced = false;
    let mut probe_failures = 0;
    let mut last_probe = tokio::time::Instant::now();
    loop {
        if browser.is_busy() {
            tokio::time::sleep(config.poll_interval.min(Duration::from_millis(250))).await;
            continue;
        }
        if browser.is_running().await {
            backoff = config.initial_backoff;
            if !alive {
                alive = true;
                let event = if std::mem::take(&mut died) { BrowserEvent::Restarted } else { BrowserEvent::Started };
                if sender.send(event).await.is_err() {
                    return;
                }
            }
            let required = browser
                .status()
                .await
                .is_ok_and(|status| status.login_state == LoginState::Required);
            if required && !login_announced && sender.send(BrowserEvent::LoginRequired).await.is_err() {
                return;
            }
            login_announced = required;
            if last_probe.elapsed() >= config.probe_interval {
                match browser.probe_renderer().await {
                    RendererState::Hung => probe_failures += 1,
                    _ => probe_failures = 0,
                }
                last_probe = tokio::time::Instant::now();
                if probe_failures >= config.probe_failures {
                    probe_failures = 0;
                    if sender.send(BrowserEvent::Hung).await.is_err() {
                        return;
                    }
                    if browser.restart_hung().await.is_ok() {
                        alive = false;
                        died = true;
                        login_announced = false;
                    }
                    continue;
                }
            }
            tokio::time::sleep(config.poll_interval).await;
            continue;
        }
        if alive {
            alive = false;
            login_announced = false;
            if browser.is_wanted() {
                died = true;
                if sender.send(BrowserEvent::Died).await.is_err() {
                    return;
                }
                tokio::time::sleep(config.died_wait).await;
                continue;
            }
        }
        if !browser.is_wanted() {
            tokio::time::sleep(config.poll_interval).await;
            continue;
        }
        if browser.ensure_running().await.is_err() {
            tokio::time::sleep(backoff).await;
            backoff = next_backoff(backoff, config.max_backoff);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let max = Duration::from_secs(60);
        let mut value = Duration::from_secs(2);
        let mut seen = Vec::new();
        for _ in 0..7 {
            seen.push(value.as_secs());
            value = next_backoff(value, max);
        }
        assert_eq!(seen, [2, 4, 8, 16, 32, 60, 60]);
    }
}
