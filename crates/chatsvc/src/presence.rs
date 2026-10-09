use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use serde_json::{Value, json};
use session::{Method, PRESENCE, Request, Scope, Session};
use uuid::Uuid;

use crate::cards::{CardTransport, SessionCardTransport};
use crate::error::{Error, Result};

const BASE_URL: &str = "https://presence.teams.microsoft.com/v1";
const PRESENCE_SCOPE: &str = "user_impersonation";
const CLIENT_VERSION: &str = "1415/26091712213";
const ORGID_PREFIX: &str = "8:orgid:";
const PINNED_NOTE_SUFFIX: &str = "<pinnednote></pinnednote>";
const NEVER_EXPIRES: &str = "9999-12-31T23:59:59.999Z";
const NEVER_YEAR: i32 = 9999;
const OFFLINE_ACTIVITY: &str = "OffWork";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedKind {
    Available,
    Busy,
    DoNotDisturb,
    BeRightBack,
    Away,
    Offline,
}

impl ForcedKind {
    pub fn code(self) -> &'static str {
        match self {
            ForcedKind::Available => "Available",
            ForcedKind::Busy => "Busy",
            ForcedKind::DoNotDisturb => "DoNotDisturb",
            ForcedKind::BeRightBack => "BeRightBack",
            ForcedKind::Away => "Away",
            ForcedKind::Offline => "Offline",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "Available" => Some(ForcedKind::Available),
            "Busy" => Some(ForcedKind::Busy),
            "DoNotDisturb" => Some(ForcedKind::DoNotDisturb),
            "BeRightBack" => Some(ForcedKind::BeRightBack),
            "Away" => Some(ForcedKind::Away),
            "Offline" => Some(ForcedKind::Offline),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForcedAvailability {
    pub kind: ForcedKind,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusNote {
    pub text: String,
    pub show_when_messaged: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkLocationKind {
    Office,
    Remote,
}

impl WorkLocationKind {
    fn code(self) -> u8 {
        match self {
            WorkLocationKind::Office => 1,
            WorkLocationKind::Remote => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkLocationSource {
    Set,
    Scheduled,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkLocation {
    pub kind: WorkLocationKind,
    pub source: WorkLocationSource,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresenceStatus {
    pub availability: String,
    pub activity: Option<String>,
    pub forced: Option<ForcedAvailability>,
    pub note: Option<StatusNote>,
    pub out_of_office: bool,
    pub out_of_office_note: Option<String>,
    pub work_location: Option<WorkLocation>,
}

pub struct PresenceService<T: CardTransport = SessionCardTransport> {
    transport: T,
}

impl PresenceService<SessionCardTransport> {
    pub fn new(session: &Session) -> Self {
        PresenceService::with_transport(SessionCardTransport::new(session))
    }
}

impl<T: CardTransport> PresenceService<T> {
    pub fn with_transport(transport: T) -> Self {
        PresenceService { transport }
    }

    pub async fn set_availability(&self, forced: Option<&ForcedAvailability>) -> Result<()> {
        let url = format!("{BASE_URL}/me/forceavailability/");
        let request = match forced {
            Some(forced) => Request::with_body(Method::Put, &url, availability_body(forced)),
            None => Request {
                method: Method::Put,
                ..Request::get(&url)
            },
        };
        self.send(request, &url).await.map(drop)
    }

    pub async fn set_note(&self, note: Option<&StatusNote>) -> Result<()> {
        let url = format!("{BASE_URL}/me/publishnote");
        let body = note.map_or_else(|| json!({"message": ""}), note_body);
        self.send(Request::with_body(Method::Put, &url, body), &url)
            .await
            .map(drop)
    }

    pub async fn set_work_location(
        &self,
        location: Option<(WorkLocationKind, DateTime<Utc>)>,
    ) -> Result<()> {
        let url = format!("{BASE_URL}/me/workLocation/");
        let body = match location {
            Some((kind, expires_at)) => json!({
                "location": kind.code(),
                "expirationTime": format_expiry(Some(expires_at)),
            }),
            None => json!({"location": 0}),
        };
        self.send(Request::with_body(Method::Put, &url, body), &url)
            .await
            .map(drop)
    }

    pub async fn status(&self, user_id: &str) -> Result<PresenceStatus> {
        let url = format!("{BASE_URL}/presence/getpresence/");
        let mri = format!("{ORGID_PREFIX}{user_id}");
        let body = json!([{ "mri": mri }]);
        let answer = self
            .send(Request::with_body(Method::Post, &url, body), &url)
            .await?;
        parse_status(&answer, &mri, Utc::now())
    }

    async fn send(&self, mut request: Request, url: &str) -> Result<Value> {
        request.headers = service_headers();
        let answer = self
            .transport
            .send(request, Scope::new(PRESENCE, PRESENCE_SCOPE))
            .await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                url,
                answer.body,
            )));
        }
        Ok(answer.body)
    }
}

fn service_headers() -> Vec<(String, String)> {
    vec![
        ("x-ms-client-user-agent".into(), "Teams-V2-Web".into()),
        ("x-ms-correlation-id".into(), Uuid::new_v4().to_string()),
        ("x-ms-client-version".into(), CLIENT_VERSION.into()),
    ]
}

pub fn format_expiry(expires_at: Option<DateTime<Utc>>) -> String {
    match expires_at {
        Some(expires_at) => expires_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        None => NEVER_EXPIRES.to_owned(),
    }
}

fn availability_body(forced: &ForcedAvailability) -> Value {
    let mut body = json!({"availability": forced.kind.code()});
    if forced.kind == ForcedKind::Offline {
        body["activity"] = json!(OFFLINE_ACTIVITY);
    }
    if let Some(expires_at) = forced.expires_at {
        body["desiredExpirationTime"] = json!(format_expiry(Some(expires_at)));
    }
    body
}

fn note_body(note: &StatusNote) -> Value {
    let suffix = if note.show_when_messaged {
        PINNED_NOTE_SUFFIX
    } else {
        ""
    };
    json!({
        "message": format!("{}{suffix}", escape_text(&note.text)),
        "expiry": format_expiry(note.expires_at),
    })
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn unescape_text(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn plain_text(message: &str) -> Option<String> {
    let mut text = String::with_capacity(message.len());
    let mut inside_tag = false;
    for character in message.chars() {
        match character {
            '<' => inside_tag = true,
            '>' if inside_tag => inside_tag = false,
            _ if !inside_tag => text.push(character),
            _ => {}
        }
    }
    let text = unescape_text(&text);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn split_pinned(message: &str) -> (String, bool) {
    let pinned = message
        .find(PINNED_NOTE_SUFFIX)
        .is_some_and(|index| index > 0);
    (message.replace(PINNED_NOTE_SUFFIX, ""), pinned)
}

fn parse_expiry(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    (parsed.year() < NEVER_YEAR).then_some(parsed)
}

fn parse_forced(forced: &Value) -> Option<ForcedAvailability> {
    Some(ForcedAvailability {
        kind: ForcedKind::from_code(forced.get("availability")?.as_str()?)?,
        expires_at: parse_expiry(forced.get("expiry")),
    })
}

fn parse_note(note: &Value, now: DateTime<Utc>) -> Option<StatusNote> {
    let (message, show_when_messaged) = split_pinned(note.get("message")?.as_str()?);
    let text = plain_text(&message)?;
    let expires_at = parse_expiry(note.get("expiry"));
    let expired = expires_at.is_some_and(|expires_at| expires_at <= now);
    (!expired).then_some(StatusNote {
        text,
        show_when_messaged,
        expires_at,
    })
}

fn parse_work_location(location: &Value, now: DateTime<Utc>) -> Option<WorkLocation> {
    let kind = match location.get("location")?.as_str()? {
        "Office" => WorkLocationKind::Office,
        "Remote" => WorkLocationKind::Remote,
        _ => return None,
    };
    let expired = parse_expiry(location.get("expiry")).is_some_and(|expires_at| expires_at <= now);
    (!expired).then(|| WorkLocation {
        kind,
        source: match location.get("locationSource").and_then(Value::as_str) {
            Some("ScheduledLocation") => WorkLocationSource::Scheduled,
            Some("VerifiedLocation") => WorkLocationSource::Verified,
            _ => WorkLocationSource::Set,
        },
    })
}

pub fn parse_status(body: &Value, mri: &str, now: DateTime<Utc>) -> Result<PresenceStatus> {
    let entries = body
        .as_array()
        .ok_or_else(|| Error::UnexpectedAnswer("no presence list".into()))?;
    let presence = entries
        .iter()
        .find(|entry| entry.get("mri").and_then(Value::as_str) == Some(mri))
        .or_else(|| entries.first())
        .and_then(|entry| entry.get("presence"))
        .ok_or_else(|| Error::UnexpectedAnswer("no own presence".into()))?;
    Ok(PresenceStatus {
        availability: presence
            .get("availability")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        activity: presence
            .get("activity")
            .and_then(Value::as_str)
            .map(str::to_owned),
        forced: presence.get("forcedAvailability").and_then(parse_forced),
        note: presence.get("note").and_then(|note| parse_note(note, now)),
        out_of_office: presence
            .pointer("/calendarData/isOutOfOffice")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        out_of_office_note: presence
            .pointer("/calendarData/outOfOfficeNote/message")
            .and_then(Value::as_str)
            .and_then(plain_text),
        work_location: presence
            .get("workLocation")
            .and_then(|location| parse_work_location(location, now)),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;
    use session::{ApiResponse, Method};

    use super::*;

    struct Recorder {
        status: u16,
        body: Value,
        sent: Mutex<Vec<(Request, Scope)>>,
    }

    impl CardTransport for Recorder {
        async fn send(&self, request: Request, scope: Scope) -> Result<ApiResponse> {
            self.sent.lock().unwrap().push((request, scope));
            Ok(ApiResponse {
                status: self.status,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn service(status: u16, body: Value) -> PresenceService<Recorder> {
        PresenceService::with_transport(Recorder {
            status,
            body,
            sent: Mutex::new(Vec::new()),
        })
    }

    fn only_request(service: &PresenceService<Recorder>) -> (Request, Scope) {
        let mut sent = service.transport.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        sent.remove(0)
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 9, hour, minute, 0).unwrap()
    }

    #[tokio::test]
    async fn busy_with_expiry_puts_availability_and_desired_expiration() {
        let service = service(200, Value::Null);
        let forced = ForcedAvailability {
            kind: ForcedKind::Busy,
            expires_at: Some(at(15, 30)),
        };
        service.set_availability(Some(&forced)).await.unwrap();
        let (request, scope) = only_request(&service);
        assert_eq!(request.method, Method::Put);
        assert_eq!(
            request.url,
            "https://presence.teams.microsoft.com/v1/me/forceavailability/"
        );
        assert_eq!(
            request.body,
            Some(json!({
                "availability": "Busy",
                "desiredExpirationTime": "2026-10-09T15:30:00.000Z",
            }))
        );
        assert_eq!(scope, Scope::new(PRESENCE, "user_impersonation"));
        let names: Vec<&str> = request
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "x-ms-client-user-agent",
                "x-ms-correlation-id",
                "x-ms-client-version"
            ]
        );
    }

    #[tokio::test]
    async fn offline_carries_the_off_work_activity_and_no_expiry_when_open() {
        let service = service(200, Value::Null);
        let forced = ForcedAvailability {
            kind: ForcedKind::Offline,
            expires_at: None,
        };
        service.set_availability(Some(&forced)).await.unwrap();
        let (request, _) = only_request(&service);
        assert_eq!(
            request.body,
            Some(json!({"availability": "Offline", "activity": "OffWork"}))
        );
    }

    #[tokio::test]
    async fn reset_is_a_put_without_body() {
        let service = service(200, Value::Null);
        service.set_availability(None).await.unwrap();
        let (request, _) = only_request(&service);
        assert_eq!(request.method, Method::Put);
        assert_eq!(request.body, None);
        assert!(request.url.ends_with("/v1/me/forceavailability/"));
    }

    #[tokio::test]
    async fn note_with_pin_marker_and_escaped_text() {
        let service = service(200, Value::Null);
        let note = StatusNote {
            text: "Out <b> & about".into(),
            show_when_messaged: true,
            expires_at: Some(at(18, 0)),
        };
        service.set_note(Some(&note)).await.unwrap();
        let (request, _) = only_request(&service);
        assert_eq!(request.method, Method::Put);
        assert_eq!(
            request.url,
            "https://presence.teams.microsoft.com/v1/me/publishnote"
        );
        assert_eq!(
            request.body,
            Some(json!({
                "message": "Out &lt;b&gt; &amp; about<pinnednote></pinnednote>",
                "expiry": "2026-10-09T18:00:00.000Z",
            }))
        );
    }

    #[tokio::test]
    async fn note_without_pin_and_never_expiring() {
        let service = service(200, Value::Null);
        let note = StatusNote {
            text: "Focus".into(),
            show_when_messaged: false,
            expires_at: None,
        };
        service.set_note(Some(&note)).await.unwrap();
        let (request, _) = only_request(&service);
        assert_eq!(
            request.body,
            Some(json!({"message": "Focus", "expiry": "9999-12-31T23:59:59.999Z"}))
        );
    }

    #[tokio::test]
    async fn clearing_the_note_sends_an_empty_message_only() {
        let service = service(200, Value::Null);
        service.set_note(None).await.unwrap();
        let (request, _) = only_request(&service);
        assert_eq!(request.body, Some(json!({"message": ""})));
    }

    #[tokio::test]
    async fn failures_surface_as_errors() {
        let service = service(400, json!({"message": "bad"}));
        assert!(service.set_note(None).await.is_err());
    }

    #[tokio::test]
    async fn reads_own_presence_by_mri() {
        let body = json!([{
            "mri": "8:orgid:me",
            "presence": {"availability": "Available", "activity": "Available"}
        }]);
        let service = service(200, body);
        let status = service.status("me").await.unwrap();
        assert_eq!(status.availability, "Available");
        let (request, _) = only_request(&service);
        assert_eq!(request.method, Method::Post);
        assert_eq!(
            request.url,
            "https://presence.teams.microsoft.com/v1/presence/getpresence/"
        );
        assert_eq!(request.body, Some(json!([{"mri": "8:orgid:me"}])));
    }

    #[test]
    fn parses_forced_note_and_out_of_office() {
        let body = json!([{
            "mri": "8:orgid:me",
            "presence": {
                "availability": "Busy",
                "activity": "Busy",
                "forcedAvailability": {
                    "availability": "Busy",
                    "expiry": "2026-10-09T15:30:00Z",
                    "publishTime": "2026-10-09T15:00:00Z"
                },
                "note": {
                    "message": "In &lt;focus&gt;<pinnednote></pinnednote>",
                    "expiry": "2026-10-09T18:00:00Z"
                },
                "calendarData": {"isOutOfOffice": true}
            }
        }]);
        let status = parse_status(&body, "8:orgid:me", at(12, 0)).unwrap();
        assert_eq!(status.availability, "Busy");
        assert_eq!(
            status.forced,
            Some(ForcedAvailability {
                kind: ForcedKind::Busy,
                expires_at: Some(at(15, 30)),
            })
        );
        assert_eq!(
            status.note,
            Some(StatusNote {
                text: "In <focus>".into(),
                show_when_messaged: true,
                expires_at: Some(at(18, 0)),
            })
        );
        assert!(status.out_of_office);
    }

    #[test]
    fn never_expiry_reads_back_as_none() {
        let body = json!([{
            "mri": "8:orgid:me",
            "presence": {
                "availability": "Available",
                "note": {"message": "Hi", "expiry": "9999-12-31T23:59:59.9999999Z"}
            }
        }]);
        let status = parse_status(&body, "8:orgid:me", at(12, 0)).unwrap();
        let note = status.note.unwrap();
        assert_eq!(note.expires_at, None);
        assert!(!note.show_when_messaged);
    }

    #[test]
    fn expired_and_empty_notes_are_dropped() {
        let expired = json!([{"mri": "m", "presence": {
            "availability": "Available",
            "note": {"message": "Old", "expiry": "2026-10-09T08:00:00Z"}
        }}]);
        let empty = json!([{"mri": "m", "presence": {
            "availability": "Available",
            "note": {"message": "", "expiry": "9999-12-31T23:59:59.9999999Z"}
        }}]);
        assert_eq!(parse_status(&expired, "m", at(12, 0)).unwrap().note, None);
        assert_eq!(parse_status(&empty, "m", at(12, 0)).unwrap().note, None);
    }

    #[test]
    fn pin_marker_at_the_start_does_not_count() {
        let (text, pinned) = split_pinned("<pinnednote></pinnednote>");
        assert_eq!(text, "");
        assert!(!pinned);
    }

    #[test]
    fn unknown_forced_availability_is_ignored() {
        let body = json!([{"mri": "m", "presence": {
            "availability": "Away",
            "forcedAvailability": {"availability": "Mystery"}
        }}]);
        assert_eq!(parse_status(&body, "m", at(12, 0)).unwrap().forced, None);
    }

    #[test]
    fn missing_presence_is_unexpected() {
        assert!(matches!(
            parse_status(&json!({}), "m", at(12, 0)),
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    #[tokio::test]
    async fn work_location_puts_numbers_and_end_of_day_expiry() {
        let service = service(200, Value::Null);
        service
            .set_work_location(Some((WorkLocationKind::Remote, at(21, 59))))
            .await
            .unwrap();
        let (request, scope) = only_request(&service);
        assert_eq!(request.method, Method::Put);
        assert_eq!(
            request.url,
            "https://presence.teams.microsoft.com/v1/me/workLocation/"
        );
        assert_eq!(
            request.body,
            Some(json!({"location": 2, "expirationTime": "2026-10-09T21:59:00.000Z"}))
        );
        assert_eq!(scope, Scope::new(PRESENCE, "user_impersonation"));
        service
            .set_work_location(Some((WorkLocationKind::Office, at(21, 59))))
            .await
            .unwrap();
        let (request, _) = only_request(&service);
        assert_eq!(request.body.unwrap()["location"], json!(1));
    }

    #[tokio::test]
    async fn clearing_the_work_location_sends_zero_without_expiry() {
        let service = service(200, Value::Null);
        service.set_work_location(None).await.unwrap();
        let (request, _) = only_request(&service);
        assert_eq!(request.body, Some(json!({"location": 0})));
    }

    #[test]
    fn parses_work_location_and_hides_unknown_or_expired() {
        let read = |location: Value| {
            let body = json!([{"mri": "m", "presence": {
                "availability": "Available", "workLocation": location
            }}]);
            parse_status(&body, "m", at(12, 0)).unwrap().work_location
        };
        assert_eq!(
            read(
                json!({"location": "Remote", "expiry": "2026-10-09T21:59:59.9999999Z",
                "isForced": true, "locationSource": "ForcedLocation"})
            ),
            Some(WorkLocation {
                kind: WorkLocationKind::Remote,
                source: WorkLocationSource::Set
            })
        );
        assert_eq!(
            read(
                json!({"location": "Office", "locationSource": "ScheduledLocation",
                "expiry": "2026-10-09T21:59:59Z"})
            ),
            Some(WorkLocation {
                kind: WorkLocationKind::Office,
                source: WorkLocationSource::Scheduled
            })
        );
        assert_eq!(
            read(json!({"location": "Office", "locationSource": "VerifiedLocation"}))
                .map(|found| found.source),
            Some(WorkLocationSource::Verified)
        );
        assert_eq!(read(json!({"location": "Unknown"})), None);
        assert_eq!(read(json!({"location": "TimeOff"})), None);
        assert_eq!(
            read(json!({"location": "Office", "expiry": "2026-10-09T08:00:00Z"})),
            None
        );
    }

    #[test]
    fn note_text_is_unescaped_and_the_pin_marker_stripped() {
        let body = json!([{"mri": "m", "presence": {
            "availability": "Busy",
            "note": {
                "message": "Back at 3 &amp; later<pinnednote></pinnednote>",
                "expiry": "9999-12-31T23:59:59.9999999Z"
            },
            "calendarData": {"isOutOfOffice": false}
        }}]);
        let status = parse_status(&body, "m", at(12, 0)).unwrap();
        assert_eq!(status.note.unwrap().text, "Back at 3 & later");
        assert!(!status.out_of_office);
        assert_eq!(status.out_of_office_note, None);
    }

    #[test]
    fn out_of_office_carries_its_plain_text() {
        let read = |calendar: Value| {
            let body = json!([{"mri": "m", "presence": {
                "availability": "Away", "calendarData": calendar
            }}]);
            parse_status(&body, "m", at(12, 0)).unwrap()
        };
        let with_text = read(
            json!({"isOutOfOffice": true, "outOfOfficeNote": {"message": "<p>On &amp; leave</p>"}}),
        );
        assert!(with_text.out_of_office);
        assert_eq!(with_text.out_of_office_note.as_deref(), Some("On & leave"));
        let bare = read(json!({"isOutOfOffice": true}));
        assert!(bare.out_of_office);
        assert_eq!(bare.out_of_office_note, None);
    }

    #[test]
    fn expiry_formats_to_milliseconds_utc() {
        assert_eq!(format_expiry(Some(at(15, 30))), "2026-10-09T15:30:00.000Z");
        assert_eq!(format_expiry(None), "9999-12-31T23:59:59.999Z");
    }
}
