use url::Url;

const POSTER_FILE: &str = "maxresdefault.jpg";
const FALLBACK_POSTER_FILE: &str = "hqdefault.jpg";

pub fn video_id(link: &str) -> Option<String> {
    let parsed = Url::parse(link).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .unwrap_or(&host);
    let mut segments = parsed.path_segments()?;
    let candidate = match host {
        "youtu.be" => segments.next().map(str::to_owned),
        "youtube.com" | "youtube-nocookie.com" => match segments.next()? {
            "watch" => parsed
                .query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned()),
            "shorts" | "embed" | "live" | "v" => segments.next().map(str::to_owned),
            _ => None,
        },
        _ => None,
    }?;
    let valid = !candidate.is_empty()
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    valid.then_some(candidate)
}

pub fn poster_url(link: &str) -> Option<String> {
    video_id(link).map(|id| format!("https://img.youtube.com/vi/{id}/{POSTER_FILE}"))
}

pub fn fallback_poster_url(poster: &str) -> Option<String> {
    poster
        .strip_suffix(POSTER_FILE)
        .filter(|base| base.starts_with("https://img.youtube.com/vi/"))
        .map(|base| format!("{base}{FALLBACK_POSTER_FILE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_video_id_from_every_link_form() {
        for link in [
            "https://www.youtube.com/watch?v=abc_DEF-123&t=42",
            "https://youtube.com/watch?feature=share&v=abc_DEF-123",
            "https://m.youtube.com/watch?v=abc_DEF-123",
            "https://youtu.be/abc_DEF-123?t=7",
            "https://www.youtube.com/shorts/abc_DEF-123",
            "https://www.youtube.com/embed/abc_DEF-123",
        ] {
            assert_eq!(video_id(link).as_deref(), Some("abc_DEF-123"), "{link}");
        }
    }

    #[test]
    fn other_links_have_no_video() {
        for link in [
            "https://example.com/watch?v=abc",
            "https://www.youtube.com/channel/xyz",
            "https://www.youtube.com/watch?v=bad%20id",
            "not a url",
        ] {
            assert_eq!(video_id(link), None, "{link}");
        }
    }

    #[test]
    fn the_poster_falls_back_to_the_smaller_still() {
        let poster = poster_url("https://youtu.be/abc").unwrap();
        assert_eq!(poster, "https://img.youtube.com/vi/abc/maxresdefault.jpg");
        assert_eq!(
            fallback_poster_url(&poster).as_deref(),
            Some("https://img.youtube.com/vi/abc/hqdefault.jpg")
        );
        assert_eq!(
            fallback_poster_url("https://example.com/maxresdefault.jpg"),
            None
        );
    }
}
