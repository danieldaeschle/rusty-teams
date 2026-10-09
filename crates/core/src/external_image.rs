use url::Url;

const PROXY_PATH: &str = "/urlp/v1/url/content";
const PROXY_HOST_SUFFIX: &str = ".asyncgw.teams.microsoft.com";
const GIPHY_HOST: &str = "giphy.com";
const TEAMS_CDN_HOST: &str = "statics.teams.cdn.office.net";

/// The direct URL of a GIF or sticker image, `None` when `src` is not one of the known public hosts.
pub fn external_image_url(src: &str) -> Option<String> {
    let url = Url::parse(src).ok()?;
    if is_proxy(&url) {
        let target = url
            .query_pairs()
            .find(|(name, _)| name == "url")
            .map(|(_, value)| value.into_owned())?;
        return direct_url(&Url::parse(&target).ok()?);
    }
    direct_url(&url)
}

fn is_proxy(url: &Url) -> bool {
    url.scheme() == "https"
        && url.path() == PROXY_PATH
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(PROXY_HOST_SUFFIX))
}

fn direct_url(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let allowed = host == TEAMS_CDN_HOST
        || host == GIPHY_HOST
        || host
            .strip_suffix(GIPHY_HOST)
            .is_some_and(|prefix| prefix.ends_with('.'));
    (url.scheme() == "https" && allowed).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn giphy_and_teams_cdn_pass() {
        let giphy = "https://media0.giphy.com/media/abc/giphy.gif";
        assert_eq!(external_image_url(giphy).as_deref(), Some(giphy));
        let sticker = "https://statics.teams.cdn.office.net/evergreen-assets/stickerassets/clippy-250x250/Clippy_HelloThere.gif?v=5";
        assert_eq!(external_image_url(sticker).as_deref(), Some(sticker));
        assert!(external_image_url("https://giphy.com/x.gif").is_some());
    }

    #[test]
    fn proxy_urls_are_unwrapped() {
        let wrapped = "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/content?url=https%3a%2f%2fstatics.teams.cdn.office.net%2fevergreen-assets%2fx.png%3fv%3d5";
        assert_eq!(
            external_image_url(wrapped).as_deref(),
            Some("https://statics.teams.cdn.office.net/evergreen-assets/x.png?v=5")
        );
    }

    #[test]
    fn proxy_of_a_foreign_host_is_rejected() {
        let wrapped = "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/content?url=https%3a%2f%2fevil.example%2fx.png";
        assert_eq!(external_image_url(wrapped), None);
    }

    #[test]
    fn insecure_and_lookalike_hosts_are_rejected() {
        assert_eq!(external_image_url("http://media0.giphy.com/a.gif"), None);
        assert_eq!(
            external_image_url("https://giphy.com.evil.example/a.gif"),
            None
        );
        assert_eq!(external_image_url("https://evilgiphy.com/a.gif"), None);
        assert_eq!(
            external_image_url(
                "https://graph.microsoft.com/v1.0/chats/1/messages/2/hostedContents/3/$value"
            ),
            None
        );
        assert_eq!(external_image_url("not a url"), None);
    }
}
