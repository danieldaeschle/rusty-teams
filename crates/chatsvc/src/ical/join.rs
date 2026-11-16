/// Extract a Teams join URL from iCalendar fields (priority order from the CalDAV plan).
pub fn teams_join_url(
    skype_url: Option<&str>,
    url: Option<&str>,
    location: Option<&str>,
    description: Option<&str>,
) -> Option<String> {
    skype_url
        .filter(|value| looks_like_teams(value))
        .map(str::to_owned)
        .or_else(|| url.filter(|value| looks_like_teams(value)).map(str::to_owned))
        .or_else(|| {
            location
                .filter(|value| looks_like_teams(value))
                .map(str::to_owned)
        })
        .or_else(|| description.and_then(find_in_text))
}

fn looks_like_teams(value: &str) -> bool {
    value.contains("teams.microsoft.com") || value.contains("teams.live.com")
}

fn find_in_text(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|token| looks_like_teams(token))
        .map(|token| {
            token
                .trim_end_matches(|c: char| matches!(c, '.' | ',' | ')' | ']' | '>' | '"'))
                .to_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_skype_property() {
        let url = teams_join_url(
            Some("https://teams.microsoft.com/l/meetup-join/a"),
            Some("https://example.com"),
            None,
            Some("https://teams.microsoft.com/l/meetup-join/b"),
        );
        assert_eq!(
            url.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/a")
        );
    }

    #[test]
    fn finds_link_in_description() {
        let url = teams_join_url(
            None,
            None,
            None,
            Some("Join: https://teams.microsoft.com/l/meetup-join/x?a=1."),
        );
        assert_eq!(
            url.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/x?a=1")
        );
    }
}
