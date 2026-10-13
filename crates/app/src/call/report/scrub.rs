use std::collections::HashMap;
use std::sync::LazyLock;

use regex::{Captures, Regex, escape};

pub const SELF_PLACEHOLDER: &str = "self";
pub const ECHO_PLACEHOLDER: &str = "echo-bot";
const ORGID_PREFIX: &str = "8:orgid:";
const MIN_NAME_PART: usize = 3;
const ID_SEGMENT_MIN: usize = 8;
const REMOVED: &str = "[removed]";

fn compiled(pattern: &str) -> Regex {
    Regex::new(pattern).expect("scrub pattern compiles")
}

static URL: LazyLock<Regex> = LazyLock::new(|| compiled(r#"(?i)\b[a-z][a-z0-9+.-]*://[^\s"'<>)\]]+"#));
static JWT: LazyLock<Regex> = LazyLock::new(|| compiled(r"\beyJ[\w-]+\.[\w-]+\.[\w-]*"));
static COOKIE: LazyLock<Regex> = LazyLock::new(|| compiled(r"(?i)\b(set-cookie|cookie)\s*[:=][^\n]*"));
static BEARER: LazyLock<Regex> = LazyLock::new(|| compiled(r"(?i)\b(bearer|basic)\s+[\w\-.~+/=]+"));
static CREDENTIAL: LazyLock<Regex> = LazyLock::new(|| {
    compiled(
        r"(?i)\b(access_token|refresh_token|id_token|skypetoken|authtoken|token|authorization|password|passwd|secret|signature|sig)\b\s*[:=]\s*[^\s&;,]+",
    )
});
static ICE_CREDENTIAL: LazyLock<Regex> =
    LazyLock::new(|| compiled(r"(?i)\b(ice-ufrag|ice-pwd|ufrag|pwd)\b[:=\s]+\S+"));
static FINGERPRINT: LazyLock<Regex> = LazyLock::new(|| compiled(r"(?i)\b(?:[0-9a-f]{2}:){15,}[0-9a-f]{2}\b"));
static THREAD: LazyLock<Regex> = LazyLock::new(|| compiled(r#"\b19:[^\s,;"'<>()]+@[A-Za-z0-9.\-]+"#));
static ENCODED_THREAD: LazyLock<Regex> = LazyLock::new(|| compiled(r"(?i)\b19%3a\S+?%40[A-Za-z0-9.\-]+"));
static MRI: LazyLock<Regex> = LazyLock::new(|| {
    compiled(r#"\b(?:8:(?:orgid|live|acs|teamsvisitor|spo|gid):[A-Za-z0-9_.+\-=]+|28:[A-Za-z0-9_.\-]{8,}|4:\+?\d{5,})"#)
});
static EMAIL: LazyLock<Regex> = LazyLock::new(|| compiled(r"[A-Za-z0-9._%+'\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)+"));
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    compiled(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
});
static IPV4: LazyLock<Regex> = LazyLock::new(|| compiled(r"\b(?:\d{1,3}\.){3}\d{1,3}\b"));
static IPV6: LazyLock<Regex> = LazyLock::new(|| {
    compiled(
        r"(?i)\b(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}\b|(?:\b[0-9a-f]{1,4}:){1,7}:(?:[0-9a-f]{1,4}(?::[0-9a-f]{1,4}){0,6})?|::(?:[0-9a-f]{1,4}:){0,6}[0-9a-f]{1,4}\b",
    )
});
static PHONE: LazyLock<Regex> = LazyLock::new(|| compiled(r"\+\d[\d ()\-/]{6,}\d"));
static OPAQUE: LazyLock<Regex> = LazyLock::new(|| compiled(r"[A-Za-z0-9+/_\-]{40,}={0,2}"));

#[derive(Debug, Clone, Default)]
pub struct Scrubber {
    people: HashMap<String, String>,
    names: Vec<(String, String)>,
    meetings: HashMap<String, String>,
    identifiers: HashMap<String, String>,
    person_count: usize,
}

impl Scrubber {
    pub fn new() -> Self {
        Scrubber::default()
    }

    pub fn register_self(&mut self, mri: &str, display_name: &str, email: &str) {
        for key in [mri, email].into_iter().filter(|key| !key.trim().is_empty()) {
            self.alias(key, SELF_PLACEHOLDER);
        }
        self.add_name(display_name, SELF_PLACEHOLDER);
    }

    pub fn register_person(&mut self, mri: &str, display_name: &str) {
        let placeholder = if mri == calling::signaling::ECHO_BOT_MRI {
            self.alias(mri, ECHO_PLACEHOLDER);
            if !display_name.trim().is_empty() {
                self.names.push((display_name.to_owned(), ECHO_PLACEHOLDER.to_owned()));
            }
            return;
        } else if mri.is_empty() {
            self.person(display_name)
        } else {
            self.person(mri)
        };
        self.add_name(display_name, &placeholder);
    }

    pub fn person(&mut self, key: &str) -> String {
        let normalized = normalize(key);
        if let Some(found) = self.people.get(&normalized) {
            return found.clone();
        }
        self.person_count += 1;
        let placeholder = format!("person-{}", self.person_count);
        self.alias(key, &placeholder);
        placeholder
    }

    fn alias(&mut self, key: &str, placeholder: &str) {
        let normalized = normalize(key);
        if let Some(user_id) = normalized.strip_prefix(ORGID_PREFIX) {
            self.people.insert(user_id.to_owned(), placeholder.to_owned());
        }
        self.people.insert(normalized, placeholder.to_owned());
    }

    fn add_name(&mut self, name: &str, placeholder: &str) {
        if name.trim().is_empty() {
            return;
        }
        let mut candidates = vec![name.to_owned()];
        candidates.extend(
            name.split(|character: char| !character.is_alphanumeric())
                .filter(|part| part.chars().count() >= MIN_NAME_PART)
                .map(str::to_owned),
        );
        for candidate in candidates {
            if !self.names.iter().any(|(known, _)| known.eq_ignore_ascii_case(&candidate)) {
                self.names.push((candidate, placeholder.to_owned()));
            }
        }
    }

    pub fn scrub(&mut self, text: &str) -> String {
        let mut text = URL.replace_all(text, |captures: &Captures| scrub_url(&captures[0])).into_owned();
        text = JWT.replace_all(&text, REMOVED).into_owned();
        text = COOKIE.replace_all(&text, "$1: [removed]").into_owned();
        text = BEARER.replace_all(&text, "$1 [removed]").into_owned();
        text = CREDENTIAL.replace_all(&text, "$1 [removed]").into_owned();
        text = ICE_CREDENTIAL.replace_all(&text, "$1 [removed]").into_owned();
        text = FINGERPRINT.replace_all(&text, "[fingerprint]").into_owned();
        text = THREAD.replace_all(&text, |captures: &Captures| self.meeting(&captures[0])).into_owned();
        text = ENCODED_THREAD.replace_all(&text, |captures: &Captures| self.meeting(&captures[0])).into_owned();
        text = MRI.replace_all(&text, |captures: &Captures| self.person(&captures[0])).into_owned();
        text = EMAIL.replace_all(&text, |captures: &Captures| self.person(&captures[0])).into_owned();
        text = PHONE.replace_all(&text, |captures: &Captures| self.person(&captures[0])).into_owned();
        text = UUID.replace_all(&text, |captures: &Captures| self.identifier(&captures[0])).into_owned();
        text = IPV6.replace_all(&text, "[ip]").into_owned();
        text = IPV4.replace_all(&text, "[ip]").into_owned();
        text = self.replace_names(&text);
        OPAQUE.replace_all(&text, REMOVED).into_owned()
    }

    fn meeting(&mut self, key: &str) -> String {
        let next = self.meetings.len() + 1;
        self.meetings.entry(normalize(key)).or_insert_with(|| format!("meeting-{next}")).clone()
    }

    fn identifier(&mut self, key: &str) -> String {
        let normalized = normalize(key);
        if let Some(found) = self.people.get(&normalized) {
            return found.clone();
        }
        let next = self.identifiers.len() + 1;
        self.identifiers.entry(normalized).or_insert_with(|| format!("id-{next}")).clone()
    }

    fn replace_names(&self, text: &str) -> String {
        if self.names.is_empty() {
            return text.to_owned();
        }
        let mut ordered: Vec<&(String, String)> = self.names.iter().collect();
        ordered.sort_by_key(|(name, _)| std::cmp::Reverse(name.chars().count()));
        let alternatives: Vec<String> = ordered.iter().map(|(name, _)| escape(name)).collect();
        let pattern = format!(r"(?i)\b(?:{})\b", alternatives.join("|"));
        let Ok(names) = Regex::new(&pattern) else {
            return text.to_owned();
        };
        names
            .replace_all(text, |captures: &Captures| {
                let matched = captures[0].to_lowercase();
                ordered
                    .iter()
                    .find(|(name, _)| name.to_lowercase() == matched)
                    .map_or_else(|| REMOVED.to_owned(), |(_, placeholder)| placeholder.clone())
            })
            .into_owned()
    }
}

fn normalize(key: &str) -> String {
    key.trim().to_lowercase()
}

pub fn scrub_url(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let (authority_and_path, has_query) = match rest.find(['?', '#']) {
        Some(position) => (&rest[..position], true),
        None => (rest, false),
    };
    let (authority, path) = match authority_and_path.find('/') {
        Some(position) => authority_and_path.split_at(position),
        None => (authority_and_path, ""),
    };
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host_name = host.split(':').next().unwrap_or(host);
    let host = if IPV4.is_match(host_name) || host.starts_with('[') {
        "[ip]".to_owned()
    } else {
        host.to_lowercase()
    };
    let path: Vec<String> = path
        .split('/')
        .map(|segment| if is_identifier_segment(segment) { "{id}".to_owned() } else { segment.to_owned() })
        .collect();
    let query = if has_query { "?..." } else { "" };
    format!("{scheme}://{host}{}{query}", path.join("/"))
}

fn is_identifier_segment(segment: &str) -> bool {
    let has_digit = segment.chars().any(|character| character.is_ascii_digit());
    let encoded = segment.contains(['@', ':', '%', '=']);
    encoded || (segment.chars().count() >= ID_SEGMENT_MIN && has_digit) || UUID.is_match(segment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber() -> Scrubber {
        let mut scrubber = Scrubber::new();
        scrubber.register_self("8:orgid:11111111-2222-3333-4444-555555555555", "Daniel Daeschle", "daniel@example.com");
        scrubber.register_person("8:orgid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "Mara Lindqvist");
        scrubber
    }

    const FIXTURE: &str = "\
Mara Lindqvist called 8:orgid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee from daniel@example.com; \
Daniel Daeschle answered. Colleague jonas.ortega@contoso.com and +49 170 1234567 joined, also 8:orgid:99999999-8888-7777-6666-555555555555 and 4:+4917012345678. \
Thread 19:meeting_ZmFrZWlkMTIzNDU2Nzg5@thread.v2 and 19:abc123_def456@unq.gbl.spaces. \
GET https://emea.ng.msg.teams.microsoft.com/v1/users/ME/conversations/19%3Aabc%40thread.v2/messages?token=abc123&skypetoken=zzz#frag \
Authorization: Bearer abcdef.ghijkl.mnopqr and eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJl. \
Cookie: session=abc; other=def\n\
Retry with access_token=hunter2 failed. \
a=ice-ufrag:Xy12 a=ice-pwd:supersecretpasswordvalue12 candidate 1 UDP 2122 203.0.113.9 50000 typ host ufrag Xy12 and 2001:db8:85a3::8a2e:370:7334 \
a=fingerprint:sha-256 AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89 \
opaque QWxhZGRpbjpvcGVuIHNlc2FtZVF1ZXJ5U3RyaW5nVGhhdElzTG9uZ0Vub3VnaEZvckFUb2tlbg \
meeting id 12345678-aaaa-bbbb-cccc-1234567890ab at 12:30:45.";

    #[test]
    fn nothing_personal_survives_the_fixture() {
        let scrubbed = scrubber().scrub(FIXTURE);
        for leaked in [
            "Mara", "Lindqvist", "Daniel", "Daeschle", "daniel@", "example.com", "jonas", "contoso", "+49", "1234567",
            "aaaaaaaa", "99999999", "11111111", "ZmFrZW", "abc123", "def456", "skypetoken=zzz", "token=abc", "abcdef.ghijkl",
            "eyJ", "session=abc", "access_token=hunter2", "Xy12", "supersecret", "203.0.113.9", "2001:db8", "AB:CD:EF", "QWxhZGRp", "12345678-aaaa",
        ] {
            assert!(!scrubbed.contains(leaked), "{leaked} leaked in: {scrubbed}");
        }
    }

    #[test]
    fn people_get_stable_placeholders_across_forms() {
        let mut scrubber = scrubber();
        let scrubbed = scrubber.scrub("Mara Lindqvist 8:orgid:AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee mara");
        assert_eq!(scrubbed, "person-1 person-1 person-1 person-1");
        assert_eq!(scrubber.scrub("daniel@example.com Daniel"), "self self");
        assert_eq!(scrubber.scrub("8:orgid:99999999-8888-7777-6666-555555555555"), "person-2");
        assert_eq!(scrubber.scrub("jonas.ortega@contoso.com 8:orgid:99999999-8888-7777-6666-555555555555"), "person-3 person-2");
    }

    #[test]
    fn threads_become_numbered_meetings() {
        let mut scrubber = Scrubber::new();
        let scrubbed = scrubber.scrub("19:abc@thread.v2 then 19:def@thread.v2 then 19:abc@thread.v2");
        assert_eq!(scrubbed, "meeting-1 then meeting-2 then meeting-1");
    }

    #[test]
    fn urls_keep_host_and_path_shape() {
        assert_eq!(
            scrub_url("https://Host.Example.com/v1/users/ME/conversations/19%3Aabc%40thread.v2/messages/1699999999999?x=1"),
            "https://host.example.com/v1/users/ME/conversations/{id}/messages/{id}?..."
        );
        assert_eq!(scrub_url("https://203.0.113.9:3478/turn"), "https://[ip]/turn");
        assert_eq!(scrub_url("https://user:pass@host.example.com/a"), "https://host.example.com/a");
    }

    #[test]
    fn plain_diagnostics_stay_readable() {
        let mut scrubber = scrubber();
        let text = "ICE state Connected at 12:30:45, in packets=1200 level=0.031 mixer reports 4 (rtt 31 ms)";
        assert_eq!(scrubber.scrub(text), text);
    }

    #[test]
    fn the_echo_bot_is_named_not_numbered() {
        let mut scrubber = Scrubber::new();
        scrubber.register_person(calling::signaling::ECHO_BOT_MRI, "Teams echo");
        assert_eq!(scrubber.scrub(&format!("{} Teams echo", calling::signaling::ECHO_BOT_MRI)), "echo-bot echo-bot");
    }
}
