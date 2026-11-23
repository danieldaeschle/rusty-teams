//! Host checks for URLs that may receive a token.
//!
//! A check must read a URL exactly as the HTTP client will: `reqwest` and the browser follow the
//! WHATWG URL standard, where `https://evil.com\@x.teams.microsoft.com/` and
//! `https://evil.com?.asm.skype.com/` both connect to `evil.com`. Splitting the string by hand
//! reads another host, so every check parses with [`url::Url`].

use url::Url;

/// Registrable domains of the Microsoft services that take the account's tokens.
/// A token never goes to a host outside them, whatever URL a message or server answer carries.
pub const TOKEN_DOMAINS: [&str; 8] = [
    "microsoft.com",
    "cloud.microsoft",
    "skype.com",
    "office.com",
    "office.net",
    "sharepoint.com",
    "microsoftonline.com",
    "live.com",
];

/// Whether a request to `url` may carry an account token: `https`, no user info, a Microsoft host.
pub fn may_carry_token(url: &str) -> bool {
    https_url(url)
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .is_some_and(|host| {
            TOKEN_DOMAINS
                .iter()
                .any(|domain| host_in_domain(&host, domain))
        })
}

/// `url` parsed as an `https` URL without user info, or `None`.
///
/// The host of the result is lower case; a default port reads as `None`.
pub fn https_url(url: &str) -> Option<Url> {
    let parsed = Url::parse(url).ok()?;
    let has_user_info = !parsed.username().is_empty() || parsed.password().is_some();
    (parsed.scheme() == "https" && !has_user_info && parsed.host_str().is_some()).then_some(parsed)
}

/// Whether `host` is `domain` itself or one of its subdomains.
pub fn host_in_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(url: &str) -> Option<String> {
        https_url(url).and_then(|parsed| parsed.host_str().map(str::to_owned))
    }

    #[test]
    fn reads_the_host_the_http_client_connects_to() {
        assert_eq!(
            host("https://evil.com\\@x.asyncgw.teams.microsoft.com/a.png").as_deref(),
            Some("evil.com")
        );
        assert_eq!(
            host("https://evil.com?.asm.skype.com/v1/objects/x").as_deref(),
            Some("evil.com")
        );
        assert_eq!(
            host("https://evil.com#.teams.microsoft.com/x").as_deref(),
            Some("evil.com")
        );
    }

    #[test]
    fn rejects_other_schemes_user_info_and_garbage() {
        assert_eq!(host("http://graph.microsoft.com/v1.0"), None);
        assert_eq!(host("https://user@graph.microsoft.com/v1.0"), None);
        assert_eq!(host("https://user:pass@graph.microsoft.com/v1.0"), None);
        assert_eq!(host("graph.microsoft.com/v1.0"), None);
        assert_eq!(host("https://evil.com%2f.skype.com/x"), None);
    }

    #[test]
    fn lowercases_the_host_and_keeps_explicit_ports() {
        assert_eq!(
            host("https://Graph.Microsoft.COM/v1.0").as_deref(),
            Some("graph.microsoft.com")
        );
        assert_eq!(
            https_url("https://graph.microsoft.com:443/")
                .unwrap()
                .port(),
            None
        );
        assert_eq!(
            https_url("https://graph.microsoft.com:8443/")
                .unwrap()
                .port(),
            Some(8443)
        );
    }

    #[test]
    fn tokens_go_to_microsoft_hosts_only() {
        for url in [
            "https://graph.microsoft.com/v1.0/me",
            "https://teams.cloud.microsoft/api/mt/emea/beta/users",
            "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/image",
            "https://eu-api.asm.skype.com/v1/objects/x",
            "https://substrate.office.com/search/api/v2/query",
            "https://contoso-my.sharepoint.com/personal/a/_api/web",
            "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            "https://teams.live.com/api/x",
            "https://statics.teams.cdn.office.net/x",
        ] {
            assert!(may_carry_token(url), "{url}");
        }
        for url in [
            "https://evil.com\\@x.asyncgw.teams.microsoft.com/urlp/v1/url/image",
            "https://evil.com?.asm.skype.com/v1/objects/x",
            "https://graph.microsoft.com.evil.example/v1.0/me",
            "https://evilmicrosoft.com/x",
            "https://media0.giphy.com/media/a/giphy.gif",
            "http://graph.microsoft.com/v1.0/me",
            "https://user@graph.microsoft.com/v1.0/me",
        ] {
            assert!(!may_carry_token(url), "{url}");
        }
    }

    #[test]
    fn a_domain_matches_itself_and_its_subdomains_only() {
        assert!(host_in_domain("skype.com", "skype.com"));
        assert!(host_in_domain("eu-api.asm.skype.com", "skype.com"));
        assert!(!host_in_domain("evilskype.com", "skype.com"));
        assert!(!host_in_domain("skype.com.evil.example", "skype.com"));
    }
}
