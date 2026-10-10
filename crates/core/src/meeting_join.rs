use std::sync::LazyLock;

use percent_encoding::percent_decode_str;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{Value, json};
use url::Url;

const MEETING_HOSTS: [&str; 3] = [
    "teams.microsoft.com",
    "teams.cloud.microsoft",
    "teams.live.com",
];
const WORK_DOMAIN: &str = "teams.microsoft.com";
const LIVE_DOMAIN: &str = "teams.live.com";
const MEETUP_SEGMENT: &str = "meetup-join";
const MEET_SEGMENT: &str = "meet";
const THREAD_PREFIX: &str = "19:";
const PASSCODE_KEY: &str = "p";
const CONTEXT_KEY: &str = "context";

static URL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https://[^\s<>"'\])]+"#).expect("url pattern"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMeeting {
    pub thread_id: String,
    pub tenant_id: String,
    pub organizer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingCode {
    pub code: String,
    pub passcode: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeetingLink {
    Thread(ThreadMeeting),
    Code(MeetingCode),
}

impl MeetingCode {
    pub fn meeting_data(&self) -> Value {
        json!({
            "meetingCode": self.code,
            "passcode": self.passcode,
            "meetingUrl": self.url,
        })
    }
}

fn thread_meeting(url: &Url, segments: &[&str]) -> Option<ThreadMeeting> {
    let thread_id = percent_decode_str(segments.get(2)?).decode_utf8().ok()?;
    if !thread_id.starts_with(THREAD_PREFIX) {
        return None;
    }
    let context = url.query_pairs().find(|(key, _)| key == CONTEXT_KEY)?.1;
    let context: Value = serde_json::from_str(&context).ok()?;
    let text = |key: &str| {
        context
            .get(key)?
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    Some(ThreadMeeting {
        thread_id: thread_id.into_owned(),
        tenant_id: text("Tid")?,
        organizer_id: text("Oid")?,
    })
}

fn meeting_code(url: &Url, segments: &[&str]) -> Option<MeetingCode> {
    let code = *segments.get(1)?;
    if code.is_empty() || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let passcode = url
        .query_pairs()
        .find(|(key, _)| key == PASSCODE_KEY)
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty());
    Some(MeetingCode {
        code: code.to_owned(),
        passcode,
        url: url.to_string(),
    })
}

pub fn parse_meeting_link(text: &str) -> Option<MeetingLink> {
    let url = Url::parse(text.trim()).ok()?;
    if url.scheme() != "https" || !MEETING_HOSTS.contains(&url.host_str()?) {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.collect();
    match segments.as_slice() {
        ["l", MEETUP_SEGMENT, ..] => thread_meeting(&url, &segments).map(MeetingLink::Thread),
        [MEET_SEGMENT, ..] => meeting_code(&url, &segments).map(MeetingLink::Code),
        _ => None,
    }
}

pub fn find_meeting_link(text: &str) -> Option<(String, MeetingLink)> {
    URL_PATTERN.find_iter(text).find_map(|candidate| {
        let url = candidate
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?']);
        parse_meeting_link(url).map(|link| (url.to_owned(), link))
    })
}

pub fn meeting_link_in_html(html: &str) -> Option<String> {
    let anchors = Selector::parse("a[href]").ok()?;
    let fragment = Html::parse_fragment(html);
    let linked = fragment
        .select(&anchors)
        .filter_map(|anchor| anchor.value().attr("href"))
        .find(|href| parse_meeting_link(href).is_some());
    match linked {
        Some(href) => Some(href.to_owned()),
        None => {
            let text = fragment.root_element().text().collect::<Vec<_>>().join(" ");
            find_meeting_link(&text).map(|(url, _)| url)
        }
    }
}

pub fn meeting_code_from_id(meeting_id: &str, passcode: &str) -> Option<MeetingCode> {
    let digits: String = meeting_id
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if digits.len() < 2 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let domain = match (
        digits[..1].parse::<u8>().ok()?,
        digits[..2].parse::<u8>().ok()?,
    ) {
        (2..=4, _) | (_, 90..=92) => WORK_DOMAIN,
        (_, 93..=95) => LIVE_DOMAIN,
        _ => return None,
    };
    let passcode: String = passcode.chars().filter(|c| !c.is_whitespace()).collect();
    let mut url = Url::parse(&format!("https://{domain}/{MEET_SEGMENT}/{digits}")).ok()?;
    if !passcode.is_empty() {
        url.query_pairs_mut().append_pair(PASSCODE_KEY, &passcode);
    }
    Some(MeetingCode {
        code: digits,
        passcode: (!passcode.is_empty()).then_some(passcode),
        url: url.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "19:meeting_ZDk5@thread.v2";
    const JOIN_LINK: &str = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_ZDk5%40thread.v2/0?context=%7b%22Tid%22%3a%22tenant-1%22%2c%22Oid%22%3a%22organizer-1%22%7d";

    fn thread(link: Option<MeetingLink>) -> ThreadMeeting {
        match link {
            Some(MeetingLink::Thread(meeting)) => meeting,
            other => panic!("not a thread link: {other:?}"),
        }
    }

    #[test]
    fn a_meetup_join_link_gives_thread_tenant_and_organizer() {
        let meeting = thread(parse_meeting_link(JOIN_LINK));
        assert_eq!(meeting.thread_id, THREAD);
        assert_eq!(meeting.tenant_id, "tenant-1");
        assert_eq!(meeting.organizer_id, "organizer-1");
    }

    #[test]
    fn a_meetup_join_link_on_the_new_host_parses_too() {
        let link = JOIN_LINK.replace("teams.microsoft.com", "teams.cloud.microsoft");
        assert_eq!(thread(parse_meeting_link(&link)).thread_id, THREAD);
    }

    #[test]
    fn a_meetup_join_link_without_a_usable_context_is_invalid() {
        let bare = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_ZDk5%40thread.v2/0";
        assert_eq!(parse_meeting_link(bare), None);
        let broken = format!("{bare}?context=not-json");
        assert_eq!(parse_meeting_link(&broken), None);
        let no_organizer = format!("{bare}?context=%7b%22Tid%22%3a%22t%22%7d");
        assert_eq!(parse_meeting_link(&no_organizer), None);
        let not_a_thread = JOIN_LINK.replace("19%3ameeting", "20%3ameeting");
        assert_eq!(parse_meeting_link(&not_a_thread), None);
    }

    #[test]
    fn a_meet_link_gives_code_and_passcode() {
        let link = parse_meeting_link("https://teams.cloud.microsoft/meet/123456789012?p=AbC12xyz");
        let Some(MeetingLink::Code(code)) = link else {
            panic!("not a code link");
        };
        assert_eq!(code.code, "123456789012");
        assert_eq!(code.passcode.as_deref(), Some("AbC12xyz"));
        let without = parse_meeting_link("https://teams.microsoft.com/meet/9876543210");
        let Some(MeetingLink::Code(code)) = without else {
            panic!("not a code link");
        };
        assert_eq!(code.passcode, None);
    }

    #[test]
    fn other_links_are_no_meeting_links() {
        for text in [
            "",
            "hello",
            "https://example.com/meet/123456789012?p=x",
            "https://teams.microsoft.com/l/message/19:abc/1",
            "https://teams.microsoft.com/meet/",
            "https://teams.microsoft.com/meet/bad id",
            "http://teams.microsoft.com/meet/123456789012",
        ] {
            assert_eq!(parse_meeting_link(text), None, "{text}");
        }
    }

    #[test]
    fn a_link_is_found_inside_a_sentence_and_inside_markdown() {
        let sentence = format!("Join here {JOIN_LINK}, see you.");
        let (url, link) = find_meeting_link(&sentence).unwrap();
        assert_eq!(url, JOIN_LINK);
        assert!(matches!(link, MeetingLink::Thread(_)));
        let markdown = "[our call](https://teams.microsoft.com/meet/123456789012?p=abc)";
        let (url, _) = find_meeting_link(markdown).unwrap();
        assert_eq!(url, "https://teams.microsoft.com/meet/123456789012?p=abc");
        assert_eq!(find_meeting_link("see https://example.com/x"), None);
    }

    #[test]
    fn a_meeting_link_is_found_in_an_anchor_or_in_plain_html_text() {
        let anchor = format!("<p>Join: <a href=\"{JOIN_LINK}\">Click here</a></p>");
        assert_eq!(meeting_link_in_html(&anchor).as_deref(), Some(JOIN_LINK));
        let plain = format!("<p>Join {JOIN_LINK}</p>");
        assert_eq!(meeting_link_in_html(&plain).as_deref(), Some(JOIN_LINK));
        assert_eq!(
            meeting_link_in_html("<p>see <a href=\"https://example.com\">x</a></p>"),
            None
        );
    }

    #[test]
    fn a_meeting_id_with_spaces_and_a_passcode_becomes_a_meet_link() {
        let code = meeting_code_from_id("234 567 890 123", " Ab Cd12 ").unwrap();
        assert_eq!(code.code, "234567890123");
        assert_eq!(code.passcode.as_deref(), Some("AbCd12"));
        assert_eq!(
            code.url,
            "https://teams.microsoft.com/meet/234567890123?p=AbCd12"
        );
        assert_eq!(
            meeting_code_from_id("93-456-789-012", "").unwrap().url,
            "https://teams.live.com/meet/93456789012"
        );
        assert_eq!(
            meeting_code_from_id("91 234 567 890", "x").unwrap().code,
            "91234567890"
        );
    }

    #[test]
    fn a_meeting_id_needs_digits_from_a_known_cloud() {
        assert_eq!(meeting_code_from_id("", "x"), None);
        assert_eq!(meeting_code_from_id("12abc", "x"), None);
        assert_eq!(meeting_code_from_id("1234567890", "x"), None);
        assert_eq!(meeting_code_from_id("96 1234 5678", "x"), None);
    }

    #[test]
    fn the_request_body_carries_code_passcode_and_url() {
        let code = meeting_code_from_id("234567890123", "pw").unwrap();
        assert_eq!(
            code.meeting_data(),
            json!({
                "meetingCode": "234567890123",
                "passcode": "pw",
                "meetingUrl": "https://teams.microsoft.com/meet/234567890123?p=pw",
            })
        );
    }
}
