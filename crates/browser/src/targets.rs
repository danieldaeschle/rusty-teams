use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::BrowserError;

const LOGIN_HOSTS: [&str; 2] = ["login.microsoftonline.com", "login.live.com"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum App {
    Teams,
    Outlook,
}

pub const APPS: [App; 2] = [App::Teams, App::Outlook];

impl App {
    pub fn start_url(self) -> &'static str {
        match self {
            App::Teams => "https://teams.cloud.microsoft/",
            App::Outlook => "https://outlook.office.com/mail/",
        }
    }

    fn park_path(self) -> &'static str {
        match self {
            App::Teams => "/robots.txt",
            App::Outlook => "/owa/favicon.ico",
        }
    }

    fn hosts(self) -> &'static [&'static str] {
        match self {
            App::Teams => &["teams.cloud.microsoft", "teams.microsoft.com"],
            App::Outlook => &["outlook.office.com", "outlook.cloud.microsoft"],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            App::Teams => "teams",
            App::Outlook => "outlook",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Headless,
    Visible,
}

impl Mode {
    pub(crate) fn from_mode_file(content: &str) -> Mode {
        if content.trim() == "headless" { Mode::Headless } else { Mode::Visible }
    }

    pub(crate) fn file_content(self) -> &'static str {
        match self {
            Mode::Headless => "headless",
            Mode::Visible => "visible",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    Unknown,
    Required,
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tab {
    pub id: String,
    pub app: Option<App>,
    pub host: Option<String>,
    pub login: bool,
    pub parked: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct VersionInfo {
    #[serde(rename = "webSocketDebuggerUrl", default)]
    pub websocket_url: String,
}

#[derive(Debug, Deserialize)]
struct RawTarget {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    url: String,
}

pub(crate) fn parse_version(body: &[u8]) -> Result<VersionInfo, BrowserError> {
    serde_json::from_slice(body).map_err(|error| BrowserError::Parse(format!("/json/version: {error}")))
}

pub(crate) fn parse_tabs(body: &[u8]) -> Result<Vec<Tab>, BrowserError> {
    let targets: Vec<RawTarget> = serde_json::from_slice(body)
        .map_err(|error| BrowserError::Parse(format!("/json/list: {error}")))?;
    Ok(targets
        .into_iter()
        .filter(|target| target.kind == "page")
        .map(|target| tab_from_url(target.id, &target.url))
        .collect())
}

fn https_host(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    (parsed.scheme() == "https").then(|| parsed.host_str().map(str::to_owned))?
}

fn tab_from_url(id: String, url: &str) -> Tab {
    let host = Url::parse(url).ok().and_then(|parsed| parsed.host_str().map(str::to_owned));
    let secure = https_host(url);
    let app = secure
        .as_deref()
        .and_then(|host| APPS.into_iter().find(|app| app.hosts().contains(&host)));
    let login = secure.as_deref().is_some_and(|host| LOGIN_HOSTS.contains(&host));
    let parked = app.is_some_and(|app| Url::parse(url).is_ok_and(|parsed| parsed.path() == app.park_path()));
    Tab { id, app, host, login, parked }
}

pub(crate) fn login_state(tabs: &[Tab]) -> LoginState {
    if tabs.iter().any(|tab| tab.login) { LoginState::Required } else { LoginState::NotRequired }
}

pub(crate) fn app_tab(tabs: &[Tab], app: App) -> Option<&Tab> {
    tabs.iter().find(|tab| tab.app == Some(app))
}

pub(crate) fn attached_target_ids(result: &serde_json::Value) -> HashSet<String> {
    result["targetInfos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|info| info["attached"].as_bool() == Some(true))
        .filter_map(|info| info["targetId"].as_str().map(str::to_owned))
        .collect()
}

pub(crate) fn duplicate_app_tab_ids(tabs: &[Tab], attached: &HashSet<String>) -> Vec<String> {
    let mut duplicates = Vec::new();
    for app in APPS {
        let mut candidates: Vec<&Tab> = tabs.iter().filter(|tab| tab.app == Some(app)).collect();
        candidates.sort_by_key(|tab| (!attached.contains(&tab.id), tab.parked));
        let Some((keep, rest)) = candidates.split_first() else { continue };
        duplicates.extend(
            rest.iter().filter(|tab| !attached.contains(&tab.id) && tab.id != keep.id).map(|tab| tab.id.clone()),
        );
    }
    duplicates
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERSION: &str = r#"{"Browser":"Chrome/154.0.8037.98","Protocol-Version":"1.3","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/browser/abc"}"#;
    const LIST: &str = r#"[
      {"id":"A","type":"page","url":"https://outlook.cloud.microsoft/mail/inbox?x=secret","title":"Mail"},
      {"id":"B","type":"page","url":"https://teams.cloud.microsoft/v2/"},
      {"id":"C","type":"browser_ui","url":"chrome://omnibox-popup.top-chrome/"},
      {"id":"D","type":"service_worker","url":"https://teams.cloud.microsoft/sw.js"}
    ]"#;

    #[test]
    fn parses_version() {
        let info = parse_version(VERSION.as_bytes()).unwrap();
        assert!(info.websocket_url.ends_with("/devtools/browser/abc"));
    }

    #[test]
    fn keeps_pages_only_and_hosts_only() {
        let tabs = parse_tabs(LIST.as_bytes()).unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].app, Some(App::Outlook));
        assert_eq!(tabs[0].host.as_deref(), Some("outlook.cloud.microsoft"));
        assert_eq!(tabs[1].app, Some(App::Teams));
        assert_eq!(login_state(&tabs), LoginState::NotRequired);
        assert!(!format!("{tabs:?}").contains("secret"));
    }

    #[test]
    fn detects_login_tabs() {
        let list = r#"[{"id":"L","type":"page","url":"https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id=x"}]"#;
        let tabs = parse_tabs(list.as_bytes()).unwrap();
        assert!(tabs[0].login);
        assert_eq!(login_state(&tabs), LoginState::Required);
    }

    #[test]
    fn login_live_counts() {
        let tabs = parse_tabs(br#"[{"id":"L","type":"page","url":"https://login.live.com/"}]"#).unwrap();
        assert_eq!(login_state(&tabs), LoginState::Required);
    }

    #[test]
    fn lookalike_host_is_not_login_or_app() {
        let list = r#"[
          {"id":"X","type":"page","url":"https://login.microsoftonline.com.evil.example/"},
          {"id":"Y","type":"page","url":"http://teams.cloud.microsoft/"},
          {"id":"Z","type":"page","url":"about:blank"}
        ]"#;
        let tabs = parse_tabs(list.as_bytes()).unwrap();
        assert!(tabs.iter().all(|tab| !tab.login && tab.app.is_none()));
        assert_eq!(tabs[2].host, None);
    }

    #[test]
    fn legacy_teams_host_is_teams() {
        let tabs = parse_tabs(br#"[{"id":"T","type":"page","url":"https://teams.microsoft.com/v2/"}]"#).unwrap();
        assert_eq!(app_tab(&tabs, App::Teams).map(|tab| tab.id.as_str()), Some("T"));
        assert!(app_tab(&tabs, App::Outlook).is_none());
    }

    fn page(id: &str, url: &str) -> Tab {
        tab_from_url(id.to_owned(), url)
    }

    #[test]
    fn parked_tabs_are_still_the_app_tab() {
        let tabs = parse_tabs(
            br#"[
              {"id":"T","type":"page","url":"https://teams.cloud.microsoft/robots.txt"},
              {"id":"O","type":"page","url":"https://outlook.cloud.microsoft/owa/favicon.ico?x=1"},
              {"id":"L","type":"page","url":"https://teams.cloud.microsoft/v2/"},
              {"id":"X","type":"page","url":"https://evil.example/robots.txt"}
            ]"#,
        )
        .unwrap();
        assert!(tabs[0].parked && tabs[0].app == Some(App::Teams));
        assert!(tabs[1].parked && tabs[1].app == Some(App::Outlook));
        assert!(!tabs[2].parked && tabs[2].app == Some(App::Teams));
        assert!(!tabs[3].parked && tabs[3].app.is_none());
        assert!(tabs[..2].iter().all(|tab| !tab.login));
        assert_eq!(login_state(&tabs[..2]), LoginState::NotRequired);
        assert_eq!(app_tab(&tabs[..1], App::Teams).map(|tab| tab.id.as_str()), Some("T"));
    }

    #[test]
    fn dedupe_keeps_the_attached_tab_and_never_closes_attached_ones() {
        let tabs = [
            page("a", "https://teams.cloud.microsoft/v2/"),
            page("b", "https://teams.cloud.microsoft/v2/"),
            page("c", "https://teams.cloud.microsoft/robots.txt"),
            page("o", "https://outlook.office.com/mail/"),
            page("l", "https://login.microsoftonline.com/"),
        ];
        let attached: HashSet<String> = ["b".to_owned()].into();
        assert_eq!(duplicate_app_tab_ids(&tabs, &attached), ["a", "c"]);
        let both: HashSet<String> = ["a".to_owned(), "b".to_owned()].into();
        assert_eq!(duplicate_app_tab_ids(&tabs, &both), ["c"]);
    }

    #[test]
    fn dedupe_without_attached_keeps_the_first_loaded_tab() {
        let tabs = [
            page("p", "https://teams.cloud.microsoft/robots.txt"),
            page("a", "https://teams.cloud.microsoft/v2/"),
            page("b", "https://teams.cloud.microsoft/v2/"),
        ];
        assert_eq!(duplicate_app_tab_ids(&tabs, &HashSet::new()), ["b", "p"]);
    }

    #[test]
    fn reads_attached_ids() {
        let result = serde_json::json!({"targetInfos":[
            {"targetId":"a","attached":true},{"targetId":"b","attached":false},{"targetId":"c"}]});
        assert_eq!(attached_target_ids(&result), HashSet::from(["a".to_owned()]));
        assert!(attached_target_ids(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn mode_file_content() {
        assert_eq!(Mode::from_mode_file("headless\n"), Mode::Headless);
        assert_eq!(Mode::from_mode_file("visible"), Mode::Visible);
        assert_eq!(Mode::from_mode_file(""), Mode::Visible);
    }

    #[test]
    fn rejects_non_json() {
        assert!(parse_version(b"<html>").is_err());
        assert!(parse_tabs(b"{}").is_err());
    }
}
