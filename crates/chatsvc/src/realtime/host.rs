use serde_json::{Value, json};
use session::{App, IC3, Scope, Session};

use super::RealtimeConfig;

pub const DEFAULT_TROUTER_HOST: &str = "go-eu.trouter.teams.microsoft.com";
const DISCOVER_BODY: &str = include_str!("../../assets/discover_host.js");
const TROUTER_DOMAIN: &str = ".trouter.teams.microsoft.com";

/// The bearer is sent to this host, so only Microsoft Trouter names pass.
pub fn is_trouter_host(host: &str) -> bool {
    host.len() > TROUTER_DOMAIN.len()
        && host.ends_with(TROUTER_DOMAIN)
        && host.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
        && !host.starts_with('.')
        && !host.contains("..")
}

pub(super) fn ic3_scope() -> Scope {
    Scope::new(IC3, "Teams.AccessAsUser.All")
}

fn authz_scope() -> Scope {
    Scope::new("https://api.spaces.skype.com", "authorization.readwrite")
}

pub(super) async fn resolve(session: &Session, config: &RealtimeConfig) -> String {
    if let Some(host) = config.host.as_deref().filter(|host| is_trouter_host(host)) {
        return host.to_owned();
    }
    if config.discover_host
        && let Some(host) = discover(session).await
    {
        return host;
    }
    if is_trouter_host(&config.default_host) {
        config.default_host.clone()
    } else {
        DEFAULT_TROUTER_HOST.to_owned()
    }
}

async fn discover(session: &Session) -> Option<String> {
    let answer = session
        .run_with_token(App::Teams, &authz_scope(), DISCOVER_BODY, &json!({}), false)
        .await
        .ok()?;
    host_from_answer(&answer)
}

fn host_from_answer(answer: &Value) -> Option<String> {
    answer
        .get("host")?
        .as_str()
        .filter(|host| is_trouter_host(host))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_trouter_hosts() {
        assert!(is_trouter_host("go-eu.trouter.teams.microsoft.com"));
        assert!(is_trouter_host("go-eu2.trouter.teams.microsoft.com"));
    }

    #[test]
    fn rejects_foreign_hosts() {
        for host in [
            "trouter.teams.microsoft.com",
            ".trouter.teams.microsoft.com",
            "evil.com",
            "go-eu.trouter.teams.microsoft.com.evil.com",
            "go-eu.trouter.teams.microsoft.com:8443",
            "a@go-eu.trouter.teams.microsoft.com",
            "x/..trouter.teams.microsoft.com",
            "Go-EU.trouter.teams.microsoft.com",
            "",
        ] {
            assert!(!is_trouter_host(host), "{host}");
        }
    }

    #[test]
    fn answer_without_valid_host_is_none() {
        assert_eq!(
            host_from_answer(&json!({"host": "go-eu.trouter.teams.microsoft.com"})).as_deref(),
            Some("go-eu.trouter.teams.microsoft.com")
        );
        assert_eq!(host_from_answer(&json!({"error": "authz_http_401"})), None);
        assert_eq!(host_from_answer(&json!({"host": "evil.example"})), None);
    }
}
