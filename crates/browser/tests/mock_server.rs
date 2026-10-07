mod common;

use std::time::Duration;

use browser::{App, BrowserEvent, LoginState, Mode, RendererState, Watchdog, WatchdogConfig};
use common::harness;
use tokio::sync::mpsc::Receiver;

const TEAMS: &str = "https://teams.cloud.microsoft/";
const OUTLOOK: &str = "https://outlook.office.com/mail/";
const PARKED_TEAMS: &str = "https://teams.cloud.microsoft/robots.txt";
const PARKED_OUTLOOK: &str = "https://outlook.cloud.microsoft/owa/favicon.ico";
const LOGIN: &str = "https://login.microsoftonline.com/common/oauth2/authorize?client_id=x";

async fn next_event(events: &mut Receiver<BrowserEvent>) -> BrowserEvent {
    tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .expect("no event within 10s")
        .expect("channel closed")
}

fn fast_watchdog() -> WatchdogConfig {
    WatchdogConfig {
        poll_interval: Duration::from_millis(50),
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(200),
        died_wait: Duration::from_millis(30),
        probe_interval: Duration::from_millis(50),
        probe_failures: 2,
    }
}

#[tokio::test]
async fn status_of_a_down_browser() {
    let harness = harness().await;
    let status = harness.browser.status().await.unwrap();
    assert!(!status.running);
    assert!(status.tabs.is_empty());
    assert_eq!(status.login_state, LoginState::Unknown);
}

#[tokio::test]
async fn status_of_a_running_browser_reads_mode_file_and_tabs() {
    let harness = harness().await;
    harness.mock.start(&[TEAMS, OUTLOOK]).await;
    std::fs::write(&harness.mode_file, "headless").unwrap();
    let status = harness.browser.status().await.unwrap();
    assert!(status.running);
    assert_eq!(status.mode, Mode::Headless);
    assert_eq!(status.tabs.len(), 2);
    assert_eq!(status.tabs[0].app, Some(App::Teams));
    assert_eq!(status.login_state, LoginState::NotRequired);
}

#[tokio::test]
async fn status_reports_login_required() {
    let harness = harness().await;
    harness.mock.start(&[TEAMS, LOGIN]).await;
    let status = harness.browser.status().await.unwrap();
    assert_eq!(status.login_state, LoginState::Required);
    assert_eq!(status.mode, Mode::Visible);
}

#[tokio::test]
async fn ensure_running_starts_headless_with_one_url_and_opens_the_second_tab() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let launches = harness.launcher.requests.lock().unwrap().clone();
    assert_eq!(launches.len(), 1);
    assert!(launches[0].headless);
    assert_eq!(launches[0].start_urls, [TEAMS]);
    assert_eq!(harness.mock.urls(), [TEAMS, OUTLOOK]);
    assert_eq!(std::fs::read_to_string(&harness.mode_file).unwrap(), "headless");
    assert!(harness.mock.requests().contains(&"PUT /json/new".to_owned()));
}

#[tokio::test]
async fn ensure_running_on_a_running_browser_opens_only_missing_tabs() {
    let harness = harness().await;
    harness.mock.start(&[TEAMS]).await;
    harness.browser.ensure_running().await.unwrap();
    assert!(harness.launcher.requests.lock().unwrap().is_empty());
    assert_eq!(harness.mock.urls(), [TEAMS, OUTLOOK]);
    harness.browser.ensure_tabs().await.unwrap();
    assert_eq!(harness.mock.urls().len(), 2);
}

#[tokio::test]
async fn ensure_tabs_does_not_open_tabs_while_a_login_page_is_open() {
    let harness = harness().await;
    harness.mock.start(&[LOGIN]).await;
    harness.browser.ensure_tabs().await.unwrap();
    assert_eq!(harness.mock.urls(), [LOGIN]);
}

#[tokio::test]
async fn launch_failure_is_reported() {
    let harness = harness().await;
    *harness.launcher.fail.lock().unwrap() = true;
    assert!(harness.browser.ensure_running().await.is_err());
}

#[tokio::test]
async fn login_window_restarts_visible_and_headless_goes_back() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    harness.browser.login_window().await.unwrap();
    assert_eq!(harness.mock.urls(), [TEAMS, OUTLOOK]);
    let status = harness.browser.status().await.unwrap();
    assert_eq!(status.mode, Mode::Visible);
    assert!(!harness.launcher.requests.lock().unwrap().last().unwrap().headless);

    harness.browser.headless().await.unwrap();
    assert_eq!(harness.browser.status().await.unwrap().mode, Mode::Headless);
    assert_eq!(harness.launcher.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn stop_closes_the_browser_over_cdp() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    harness.browser.stop().await.unwrap();
    assert!(!harness.browser.is_running().await);
    harness.browser.stop().await.unwrap();
}

#[tokio::test]
async fn watchdog_reports_start_death_and_restart() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    harness.mock.kill().await;
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Died);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Restarted);
    assert!(harness.browser.is_running().await);
    assert!(harness.launcher.requests.lock().unwrap().last().unwrap().headless);
}

#[tokio::test]
async fn watchdog_retries_with_backoff_until_launch_works() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    *harness.launcher.fail.lock().unwrap() = true;
    harness.mock.kill().await;
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Died);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(harness.launcher.requests.lock().unwrap().len() >= 3);
    *harness.launcher.fail.lock().unwrap() = false;
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Restarted);
}

#[tokio::test]
async fn watchdog_announces_login_once_per_occurrence() {
    let harness = harness().await;
    harness.mock.start(&[TEAMS, OUTLOOK]).await;
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    harness.mock.set_urls(&[LOGIN, OUTLOOK]);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::LoginRequired);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(watchdog.events.try_recv().is_err());

    harness.mock.set_urls(&[TEAMS, OUTLOOK]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    harness.mock.set_urls(&[LOGIN]);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::LoginRequired);
}

#[tokio::test]
async fn watchdog_does_not_restart_after_an_explicit_stop() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    harness.browser.stop().await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(watchdog.events.try_recv().is_err());
    assert_eq!(harness.launcher.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn watchdog_ignores_the_gap_during_a_login_window_restart() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    harness.browser.login_window().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    while let Ok(event) = watchdog.events.try_recv() {
        assert_ne!(event, BrowserEvent::Died);
    }
    assert!(!harness.launcher.requests.lock().unwrap().iter().skip(1).any(|request| request.headless));
}

#[tokio::test]
async fn parked_tabs_are_the_app_tab_not_missing_and_not_login() {
    let harness = harness().await;
    harness.mock.start(&[PARKED_TEAMS, PARKED_OUTLOOK]).await;
    let status = harness.browser.status().await.unwrap();
    assert!(status.tabs.iter().all(|tab| tab.parked && tab.app.is_some()));
    assert_eq!(status.login_state, LoginState::NotRequired);
    harness.browser.ensure_running().await.unwrap();
    assert_eq!(harness.mock.urls(), [PARKED_TEAMS, PARKED_OUTLOOK]);
}

#[tokio::test]
async fn wake_navigates_the_parked_app_tab_to_its_start_url() {
    let harness = harness().await;
    harness.mock.start(&[PARKED_TEAMS, PARKED_OUTLOOK]).await;
    harness.browser.wake(App::Teams).await.unwrap();
    assert_eq!(harness.mock.urls(), [TEAMS, PARKED_OUTLOOK]);
    assert!(harness.mock.requests().contains(&"CDP Page.navigate".to_owned()));
    harness.browser.wake(App::Outlook).await.unwrap();
    assert_eq!(harness.mock.urls(), [TEAMS, OUTLOOK]);
    let status = harness.browser.status().await.unwrap();
    assert!(status.tabs.iter().all(|tab| !tab.parked));
}

#[tokio::test]
async fn wake_opens_the_tab_when_the_app_has_none() {
    let harness = harness().await;
    harness.mock.start(&[TEAMS]).await;
    harness.browser.wake(App::Outlook).await.unwrap();
    assert_eq!(harness.mock.urls(), [TEAMS, OUTLOOK]);
}

#[tokio::test]
async fn jittered_recheck_adopts_a_chrome_another_launcher_started() {
    let harness = harness().await;
    let mock = harness.mock.clone();
    let browser = harness.browser.clone();
    let ensure = tokio::spawn(async move { browser.ensure_running().await });
    tokio::time::sleep(Duration::from_millis(2)).await;
    mock.start(&[TEAMS, OUTLOOK]).await;
    ensure.await.unwrap().unwrap();
    assert!(harness.launcher.requests.lock().unwrap().is_empty());
    assert_eq!(harness.mock.urls(), [TEAMS, OUTLOOK]);
}

#[tokio::test]
async fn a_foreign_chrome_taking_the_port_during_launch_is_adopted_and_deduped() {
    let harness = harness().await;
    harness.mock.set_attached(&["P0"]);
    *harness.launcher.foreign.lock().unwrap() = Some(vec![TEAMS, TEAMS, OUTLOOK]);
    harness.browser.ensure_running().await.unwrap();
    assert_eq!(harness.mock.tab_ids(), ["P0", "P2"]);
    assert_eq!(harness.mock.urls(), [TEAMS, OUTLOOK]);
}

#[tokio::test]
async fn dedupe_closes_unattached_duplicates_after_a_launch() {
    let harness = harness().await;
    *harness.launcher.foreign.lock().unwrap() = Some(vec![TEAMS, TEAMS, OUTLOOK, OUTLOOK]);
    harness.mock.set_attached(&["P1"]);
    harness.browser.ensure_running().await.unwrap();
    assert_eq!(harness.mock.tab_ids(), ["P1", "P2"]);
}

#[tokio::test]
async fn dedupe_never_closes_login_or_foreign_tabs() {
    let harness = harness().await;
    *harness.launcher.foreign.lock().unwrap() = Some(vec![TEAMS, LOGIN, "about:blank"]);
    harness.browser.ensure_running().await.unwrap();
    assert_eq!(harness.mock.urls(), [TEAMS, LOGIN, "about:blank", OUTLOOK]);
}

#[tokio::test]
async fn watchdog_waits_after_died_and_adopts_a_chrome_that_came_up_meanwhile() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let config = WatchdogConfig { died_wait: Duration::from_millis(400), ..fast_watchdog() };
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), config);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    harness.mock.kill().await;
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Died);
    harness.mock.start(&[TEAMS, OUTLOOK]).await;
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Restarted);
    assert_eq!(harness.launcher.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn probe_reports_ok_hung_and_no_tab() {
    let harness = harness().await;
    harness.mock.start(&[OUTLOOK]).await;
    assert_eq!(harness.browser.probe_renderer().await, RendererState::NoTab);
    harness.mock.set_urls(&[TEAMS, OUTLOOK]);
    assert_eq!(harness.browser.probe_renderer().await, RendererState::Ok);
    harness.mock.stop_answering(false);
    assert_eq!(harness.browser.probe_renderer().await, RendererState::Hung);
    harness.mock.answer_again();
    assert_eq!(harness.browser.probe_renderer().await, RendererState::Ok);
}

#[tokio::test]
async fn watchdog_leaves_a_responsive_renderer_alone() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(watchdog.events.try_recv().is_err());
    assert_eq!(harness.launcher.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn watchdog_restarts_a_hung_renderer_with_a_graceful_close() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    harness.mock.stop_answering(false);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Hung);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Restarted);
    assert_eq!(harness.launcher.requests.lock().unwrap().len(), 2);
    assert_eq!(*harness.launcher.kills.lock().unwrap(), 0);
    assert_eq!(harness.browser.probe_renderer().await, RendererState::Ok);
}

#[tokio::test]
async fn hung_stop_falls_back_to_kill_when_close_is_ignored() {
    let harness = harness().await;
    harness.browser.ensure_running().await.unwrap();
    let mut watchdog = Watchdog::spawn(harness.browser.clone(), fast_watchdog());
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Started);

    harness.mock.stop_answering(true);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Hung);
    assert_eq!(next_event(&mut watchdog.events).await, BrowserEvent::Restarted);
    assert_eq!(*harness.launcher.kills.lock().unwrap(), 1);
    assert_eq!(harness.launcher.requests.lock().unwrap().len(), 2);
}
