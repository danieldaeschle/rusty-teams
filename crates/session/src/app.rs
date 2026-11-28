use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

/// Defender for Cloud Apps (Conditional Access App Control) proxies the app on `<host>.mcas.ms`.
pub const MCAS_SUFFIX: &str = ".mcas.ms";

// The MSAL cache lives on the origin the page ran on. Behind MCAS thats the proxied
// origin, so headless restarts and parking have to go there too, else the cache is empty.
static BEHIND_MCAS: AtomicBool = AtomicBool::new(false);

/// Sends start and park navigation through the MCAS proxy from now on.
pub fn set_behind_mcas(enabled: bool) {
    BEHIND_MCAS.store(enabled, Ordering::SeqCst);
}

/// True when start and park navigation go through the MCAS proxy.
pub fn behind_mcas() -> bool {
    BEHIND_MCAS.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum App {
    Teams,
}

impl App {
    pub const ALL: [App; 1] = [App::Teams];

    pub fn origins(self) -> &'static [&'static str] {
        match self {
            App::Teams => &[
                "https://teams.cloud.microsoft",
                "https://teams.microsoft.com",
            ],
        }
    }

    pub fn start_url(self) -> &'static str {
        self.start_url_via(behind_mcas())
    }

    /// The start URL on the app's own host, never through the MCAS proxy.
    pub fn direct_start_url(self) -> &'static str {
        self.start_url_via(false)
    }

    fn start_url_via(self, mcas: bool) -> &'static str {
        match (self, mcas) {
            (App::Teams, false) => "https://teams.cloud.microsoft/",
            (App::Teams, true) => "https://teams.cloud.microsoft.mcas.ms/",
        }
    }

    /// A static page on the app origin: keeps the MSAL cache reachable without running the app.
    pub fn park_url(self) -> &'static str {
        self.park_url_via(behind_mcas())
    }

    fn park_url_via(self, mcas: bool) -> &'static str {
        match (self, mcas) {
            (App::Teams, false) => "https://teams.cloud.microsoft/robots.txt",
            (App::Teams, true) => "https://teams.cloud.microsoft.mcas.ms/robots.txt",
        }
    }

    pub fn is_park_url(self, url: &str) -> bool {
        // both origins count, a tab parked before the switch must still be seen as parked
        [self.park_url_via(false), self.park_url_via(true)]
            .iter()
            .any(|park| url.starts_with(park))
    }

    /// True when `url` is an https page on one of the app hosts, directly or through the
    /// MCAS proxy. The host must match exactly, a look-alike host is not the app.
    pub fn is_app_url(self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        let Some(host) = parsed.host_str().filter(|_| parsed.scheme() == "https") else {
            return false;
        };
        let host = host.strip_suffix(MCAS_SUFFIX).unwrap_or(host);
        self.origins()
            .iter()
            .any(|origin| origin.strip_prefix("https://") == Some(host))
    }
}

impl std::fmt::Display for App {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            App::Teams => "teams",
        })
    }
}

const LOGIN_ORIGINS: [&str; 2] = [
    "https://login.microsoftonline.com",
    "https://login.live.com",
];

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
        target.kind == "page" && target.websocket_url.is_some() && app.is_app_url(&target.url)
    })
}

pub fn is_login_url(url: &str) -> bool {
    LOGIN_ORIGINS.iter().any(|origin| url.starts_with(origin))
}

pub fn has_login_tab(targets: &[Target]) -> bool {
    targets
        .iter()
        .any(|target| target.kind == "page" && is_login_url(&target.url))
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
        assert_eq!(
            find_app_tab(&targets, App::Teams)
                .unwrap()
                .websocket_url
                .as_deref(),
            Some("ws://x/3")
        );
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
        assert!(!App::Teams.is_park_url("https://teams.cloud.microsoft/v2/"));
    }

    #[test]
    fn mcas_urls_go_through_the_proxy_and_both_park_urls_count() {
        assert_eq!(
            App::Teams.start_url_via(true),
            "https://teams.cloud.microsoft.mcas.ms/"
        );
        assert_eq!(
            App::Teams.park_url_via(true),
            "https://teams.cloud.microsoft.mcas.ms/robots.txt"
        );
        assert_eq!(
            App::Teams.start_url_via(false),
            "https://teams.cloud.microsoft/"
        );
        assert_eq!(
            App::Teams.direct_start_url(),
            App::Teams.start_url_via(false)
        );
        assert!(App::Teams.is_park_url("https://teams.cloud.microsoft.mcas.ms/robots.txt"));
        assert!(App::Teams.is_park_url("https://teams.cloud.microsoft/robots.txt"));
        assert!(!App::Teams.is_park_url("https://teams.cloud.microsoft.mcas.ms/v2/"));
    }

    #[test]
    fn app_urls_need_an_exact_host() {
        for url in [
            "https://teams.cloud.microsoft/v2/",
            "https://teams.microsoft.com/v2/",
            "https://teams.cloud.microsoft.mcas.ms/",
            "https://teams.cloud.microsoft.mcas.ms/robots.txt",
        ] {
            assert!(App::Teams.is_app_url(url), "{url}");
        }
        for url in [
            "https://teams.cloud.microsoft.evil.example/",
            "https://teams.cloud.microsoftevil.example/",
            "https://evil.mcas.ms/",
            "http://teams.cloud.microsoft/",
            "https://login.microsoftonline.com/",
            "about:blank",
        ] {
            assert!(!App::Teams.is_app_url(url), "{url}");
        }
        let look_alike: Vec<Target> = serde_json::from_str(
            r#"[{"type":"page","url":"https://teams.cloud.microsoft.evil.example/","webSocketDebuggerUrl":"ws://x/1"}]"#,
        )
        .unwrap();
        assert!(find_app_tab(&look_alike, App::Teams).is_none());
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
