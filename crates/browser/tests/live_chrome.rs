use std::time::Duration;

use browser::{Browser, Config, LoginState, Mode};

const TEST_PORT: u16 = 9444;

#[tokio::test]
#[ignore = "starts a real Chrome on port 9444 with a throwaway profile"]
async fn start_status_stop_on_a_separate_port_and_profile() {
    let profile_name = format!("rusty-teams-chrome-test-{}", std::process::id());
    let config = Config::detect().unwrap().with_port(TEST_PORT).with_profile_name(&profile_name);
    let mode_file = config.mode_file.clone();
    let profile_directory = mode_file.with_extension("");
    let browser = Browser::new(config);
    assert!(!browser.is_running().await, "port {TEST_PORT} is in use");

    let outcome = async {
        browser.ensure_running().await.unwrap();
        let status = browser.status().await.unwrap();
        assert!(status.running);
        assert_eq!(status.mode, Mode::Headless);
        assert_eq!(status.tabs.len(), 2);
        tokio::time::sleep(Duration::from_secs(5)).await;
        let status = browser.status().await.unwrap();
        println!("login state after load: {:?}", status.login_state);
        assert_ne!(status.login_state, LoginState::Unknown);
        browser.stop().await.unwrap();
        assert!(!browser.is_running().await);
    }
    .await;

    let _ = browser.stop().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _ = std::fs::remove_dir_all(&profile_directory);
    let _ = std::fs::remove_file(&mode_file);
    outcome
}
