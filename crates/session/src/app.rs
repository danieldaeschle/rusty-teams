use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum App {
    Teams,
    Outlook,
}

impl App {
    pub const ALL: [App; 2] = [App::Teams, App::Outlook];

    pub fn origins(self) -> &'static [&'static str] {
        match self {
            App::Teams => &["https://teams.cloud.microsoft", "https://teams.microsoft.com"],
            App::Outlook => &["https://outlook.office.com", "https://outlook.cloud.microsoft"],
        }
    }

    pub fn start_url(self) -> &'static str {
        match self {
            App::Teams => "https://teams.cloud.microsoft/",
            App::Outlook => "https://outlook.cloud.microsoft/mail/",
        }
    }

    /// A static page on the app origin: keeps the MSAL cache reachable without running the app.
    pub fn park_url(self) -> &'static str {
        match self {
            App::Teams => "https://teams.cloud.microsoft/robots.txt",
            App::Outlook => "https://outlook.cloud.microsoft/owa/favicon.ico",
        }
    }

    pub fn is_park_url(self, url: &str) -> bool {
        url.starts_with(self.park_url())
    }
}

impl std::fmt::Display for App {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            App::Teams => "teams",
            App::Outlook => "outlook",
        })
    }
}

const LOGIN_ORIGINS: [&str; 2] = ["https://login.microsoftonline.com", "https://login.live.com"];

#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub websocket_url: Option<String>,
}

pub fn find_app_tab(targets: &[Target], app: App) -> Option<&Target> {
    targets.iter().find(|target| {
        target.kind == "page"
            && target.websocket_url.is_some()
            && app.origins().iter().any(|origin| target.url.starts_with(origin))
    })
}

pub fn is_login_url(url: &str) -> bool {
    LOGIN_ORIGINS.iter().any(|origin| url.starts_with(origin))
}

pub fn has_login_tab(targets: &[Target]) -> bool {
    targets.iter().any(|target| target.kind == "page" && is_login_url(&target.url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<Target> {
        serde_json::from_str(
            r#"[
              {"type":"service_worker","url":"https://teams.cloud.microsoft/sw.js","webSocketDebuggerUrl":"ws://x/1"},
              {"type":"page","url":"https://outlook.cloud.microsoft/mail/","webSocketDebuggerUrl":"ws://x/2"},
              {"type":"page","url":"https://teams.cloud.microsoft/v2/","webSocketDebuggerUrl":"ws://x/3"},
              {"type":"page","url":"about:blank"}
            ]"#,
        )
        .unwrap()
    }

    #[test]
    fn finds_page_tabs_per_app() {
        let targets = targets();
        assert_eq!(find_app_tab(&targets, App::Teams).unwrap().websocket_url.as_deref(), Some("ws://x/3"));
        assert_eq!(find_app_tab(&targets, App::Outlook).unwrap().websocket_url.as_deref(), Some("ws://x/2"));
    }

    #[test]
    fn ignores_non_page_targets_and_missing_tabs() {
        let only_worker = &targets()[..1];
        assert!(find_app_tab(only_worker, App::Teams).is_none());
    }

    #[test]
    fn parked_tabs_are_found_by_origin_and_recognised() {
        let parked: Vec<Target> = serde_json::from_str(
            r#"[{"type":"page","url":"https://teams.cloud.microsoft/robots.txt","webSocketDebuggerUrl":"ws://x/9"},
                {"type":"page","url":"https://outlook.cloud.microsoft/owa/favicon.ico","webSocketDebuggerUrl":"ws://x/8"}]"#,
        )
        .unwrap();
        let teams = find_app_tab(&parked, App::Teams).unwrap();
        assert!(App::Teams.is_park_url(&teams.url));
        assert!(App::Outlook.is_park_url(&find_app_tab(&parked, App::Outlook).unwrap().url));
        assert!(!App::Teams.is_park_url("https://teams.cloud.microsoft/v2/"));
    }

    #[test]
    fn detects_login_tab() {
        let mut targets = targets();
        assert!(!has_login_tab(&targets));
        targets.push(Target {
            kind: "page".into(),
            url: "https://login.microsoftonline.com/common/oauth2".into(),
            websocket_url: None,
        });
        assert!(has_login_tab(&targets));
    }
}
