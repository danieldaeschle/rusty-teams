use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value;
use session::{ApiResponse, Method, Request, Session};

use crate::cards::{CardTransport, DEFAULT_MT_REGION, SessionCardTransport, spaces_scope};
use crate::error::{Error, Result};

mod availability;
mod reminders;
mod schedule;

pub use availability::{BusyTime, PersonSchedule, parse_schedules};

pub use reminders::{Reminder, parse_reminders};
pub use schedule::{
    Attendee, CreatedEvent, NewEvent, Repeat, Reply, new_event_body, new_event_body_with_join,
    parse_created_event,
};

const QUERY_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";
const NAIVE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.f";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResponseType {
    Organizer,
    Accepted,
    Tentative,
    Declined,
    #[default]
    NotResponded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShowAs {
    Free,
    Tentative,
    #[default]
    Busy,
    OutOfOffice,
    WorkingElsewhere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarSource {
    #[default]
    Outlook,
    Google,
}

impl CalendarSource {
    pub fn label(self) -> &'static str {
        match self {
            CalendarSource::Outlook => "Outlook",
            CalendarSource::Google => "Google",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AttendeeResponseCounts {
    pub accepted: u32,
    pub declined: u32,
    pub tentative: u32,
    pub not_responded: u32,
}

impl AttendeeResponseCounts {
    pub fn is_empty(self) -> bool {
        self.accepted == 0
            && self.declined == 0
            && self.tentative == 0
            && self.not_responded == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAttendee {
    pub name: String,
    pub address: Option<String>,
    pub response: ResponseType,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub object_id: String,
    pub subject: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub cancelled: bool,
    pub online: bool,
    pub join_url: Option<String>,
    pub thread_id: Option<String>,
    pub organizer_name: Option<String>,
    pub organizer_address: Option<String>,
    pub ical_uid: Option<String>,
    pub response: ResponseType,
    pub show_as: ShowAs,
    pub dial_in: Option<DialIn>,
    pub source: CalendarSource,
    pub account_id: Option<String>,
    pub calendar_id: Option<String>,
    /// When a merge collapsed duplicates, every contributing source.
    pub sources: Vec<CalendarSource>,
    pub location: Option<String>,
    pub body_preview: Option<String>,
    /// Full event body (HTML). `body_preview` from list payloads is server-truncated.
    pub body_html: Option<String>,
    pub series: bool,
    pub series_master_id: Option<String>,
    pub response_counts: Option<AttendeeResponseCounts>,
    pub attendees: Vec<EventAttendee>,
    /// Minutes before start (Google reminders). Empty when unknown.
    pub reminders: Vec<u32>,
    /// Google `source` title + URL (View source).
    pub source_link: Option<(String, String)>,
    /// Google `htmlLink` — open in Google Calendar.
    pub web_link: Option<String>,
    /// Display name of the calendar that owns the event (Google).
    pub calendar_name: Option<String>,
}

impl CalendarEvent {
    pub fn outlook_defaults() -> (
        CalendarSource,
        Option<String>,
        Option<String>,
        Vec<CalendarSource>,
    ) {
        (
            CalendarSource::Outlook,
            None,
            None,
            vec![CalendarSource::Outlook],
        )
    }
}

impl Default for CalendarEvent {
    fn default() -> Self {
        let (source, account_id, calendar_id, sources) = Self::outlook_defaults();
        CalendarEvent {
            object_id: String::new(),
            subject: String::new(),
            start: DateTime::<Utc>::UNIX_EPOCH,
            end: DateTime::<Utc>::UNIX_EPOCH,
            all_day: false,
            cancelled: false,
            online: false,
            join_url: None,
            thread_id: None,
            organizer_name: None,
            organizer_address: None,
            ical_uid: None,
            response: ResponseType::default(),
            show_as: ShowAs::default(),
            dial_in: None,
            source,
            account_id,
            calendar_id,
            sources,
            location: None,
            body_preview: None,
            body_html: None,
            series: false,
            series_master_id: None,
            response_counts: None,
            attendees: Vec::new(),
            reminders: Vec::new(),
            source_link: None,
            web_link: None,
            calendar_name: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialIn {
    pub toll_number: String,
    pub conference_id: String,
}

const JOIN_INFO_TITLE: &str = "Microsoft Teams meeting";

/// The plain text Teams puts on the clipboard for "Copy join info".
pub fn join_info_text(join_url: &str, dial_in: Option<&DialIn>) -> String {
    let mut lines = vec![JOIN_INFO_TITLE.to_owned(), format!("Join: {join_url}")];
    if let Some(dial_in) = dial_in {
        lines.push(String::new());
        lines.push("Dial in by phone".to_owned());
        lines.push(format!(
            "{},,{}#",
            dial_in.toll_number, dial_in.conference_id
        ));
        lines.push(format!("Phone conference ID: {}#", dial_in.conference_id));
    }
    lines.join("\n")
}

fn dial_in(entry: &Value) -> Option<DialIn> {
    Some(DialIn {
        toll_number: text(entry, "onlineMeetingTollNumber").filter(|value| !value.is_empty())?,
        conference_id: text(entry, "onlineMeetingConferenceId")
            .filter(|value| !value.is_empty())?,
    })
}

pub struct Calendar<T: CardTransport = SessionCardTransport> {
    transport: T,
    url: String,
    create_url: String,
    mt_region: String,
}

impl Calendar<SessionCardTransport> {
    pub fn new(session: &Session) -> Self {
        Calendar::with_transport(SessionCardTransport::new(session), DEFAULT_MT_REGION)
    }
}

impl<T: CardTransport> Calendar<T> {
    pub fn with_transport(transport: T, mt_region: &str) -> Self {
        Calendar {
            transport,
            url: format!(
                "https://teams.cloud.microsoft/api/mt/{mt_region}/v2.1/me/calendars/calendarView"
            ),
            create_url: format!(
                "https://teams.cloud.microsoft/api/mt/{mt_region}/beta/me/calendarEvents/privateMeeting/schedulingService/create"
            ),
            mt_region: mt_region.to_owned(),
        }
    }

    pub fn teams_event_url(&self, event_id: &str) -> String {
        format!(
            "https://teams.cloud.microsoft/api/mt/{}/beta/me/calendarEvents/{}",
            self.mt_region,
            crate::messages::encode(event_id)
        )
    }

    pub async fn calendar_view(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        let answer = self.view_response(start, end).await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                &self.url,
                answer.body,
            )));
        }
        Ok(parse_calendar_events(&answer.body))
    }

    pub async fn view_response(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<ApiResponse> {
        let url = format!(
            "{}?startDate={}&endDate={}",
            self.url,
            start.format(QUERY_TIME_FORMAT),
            end.format(QUERY_TIME_FORMAT)
        );
        self.transport.send(Request::get(url), spaces_scope()).await
    }
}

impl<T: CardTransport> Calendar<T> {
    pub async fn create_meet_now(&self, subject: &str) -> Result<String> {
        let body = serde_json::json!({
            "meetingType": "MeetNow",
            "isStreamEnabled": false,
            "subject": subject,
            "unhideChatThread": true,
        });
        let answer = self
            .transport
            .send(
                Request::with_body(Method::Post, &self.create_url, body),
                spaces_scope(),
            )
            .await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                &self.create_url,
                answer.body,
            )));
        }
        meet_now_join_url(&answer.body).ok_or_else(|| {
            Error::Session(session::Error::api(
                answer.status,
                &self.create_url,
                answer.body,
            ))
        })
    }
}

pub fn meet_now_join_url(body: &Value) -> Option<String> {
    let value = body.get("value")?;
    value
        .pointer("/links/join")
        .or_else(|| value.get("meetingUrl"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn parse_calendar_events(body: &Value) -> Vec<CalendarEvent> {
    let entries = body.get("value").unwrap_or(body);
    let mut events: Vec<CalendarEvent> = entries
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(parse_event)
        .collect();
    events.sort_by_key(|event| (event.start, event.end));
    events
}

fn text(entry: &Value, key: &str) -> Option<String> {
    entry
        .get(key)?
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn flag(entry: &Value, key: &str) -> bool {
    entry.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn time(entry: &Value, key: &str) -> Option<DateTime<Utc>> {
    let text = entry.get(key)?.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .map(|time| time.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(text, NAIVE_TIME_FORMAT).map(|time| time.and_utc())
        })
        .ok()
}

fn thread_id(entry: &Value) -> Option<String> {
    text(entry, "threadId").or_else(|| {
        let data: Value = serde_json::from_str(entry.get("skypeTeamsData")?.as_str()?).ok()?;
        text(&data, "cid")
    })
}

fn response_type(entry: &Value) -> ResponseType {
    if flag(entry, "isOrganizer") {
        return ResponseType::Organizer;
    }
    let label = text(entry, "myResponseType").or_else(|| {
        let status = entry.get("responseStatus")?;
        status
            .as_str()
            .map(str::to_owned)
            .or_else(|| text(status, "response"))
    });
    match label.as_deref() {
        Some("Organizer") => ResponseType::Organizer,
        Some("Accepted") => ResponseType::Accepted,
        Some("TentativelyAccepted" | "Tentative") => ResponseType::Tentative,
        Some("Declined") => ResponseType::Declined,
        _ => ResponseType::NotResponded,
    }
}

fn show_as(entry: &Value) -> ShowAs {
    match text(entry, "showAs").as_deref() {
        Some("Free") => ShowAs::Free,
        Some("Tentative") => ShowAs::Tentative,
        Some("Oof") => ShowAs::OutOfOffice,
        Some("WorkingElsewhere") => ShowAs::WorkingElsewhere,
        _ => ShowAs::Busy,
    }
}

fn location_text(entry: &Value) -> Option<String> {
    text(entry, "locationDisplayName")
        .or_else(|| text(entry, "location"))
        .or_else(|| {
            entry
                .get("location")
                .and_then(|location| text(location, "displayName"))
        })
        .filter(|value| !value.is_empty())
}

fn body_preview_text(entry: &Value) -> Option<String> {
    text(entry, "bodyPreview")
        .or_else(|| text(entry, "bodyContent").map(|content| strip_html(&content)))
        .or_else(|| {
            entry
                .get("body")
                .and_then(|body| text(body, "content"))
                .map(|content| strip_html(&content))
        })
        .map(|preview| normalize_preview_whitespace(&preview))
        .map(|preview| truncate_preview(&preview))
        .filter(|value| !value.is_empty())
}

/// Full body as HTML: Teams MT `bodyContent` or Graph `body.content`.
/// Plain-text bodies are escaped and keep their line breaks.
fn body_html_text(entry: &Value) -> Option<String> {
    let (content, kind) = match text(entry, "bodyContent") {
        Some(content) => (content, text(entry, "bodyContentType")),
        None => {
            let body = entry.get("body")?;
            (text(body, "content")?, text(body, "contentType"))
        }
    };
    if content.trim().is_empty() {
        return None;
    }
    if kind.is_some_and(|kind| kind.eq_ignore_ascii_case("text")) {
        let escaped = content
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "<br>");
        return Some(escaped);
    }
    // Drop non-http(s)/mailto hrefs so the agenda renderer cannot open them.
    Some(sanitize_html_hrefs(&content))
}

fn sanitize_html_hrefs(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("href=") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let quote = rest.as_bytes().get(5).copied();
        let (quote_char, after_quote) = match quote {
            Some(b'"') | Some(b'\'') => (quote.unwrap() as char, &rest[6..]),
            _ => {
                out.push_str("href=");
                rest = &rest[5..];
                continue;
            }
        };
        let Some(end) = after_quote.find(quote_char) else {
            out.push_str(rest);
            return out;
        };
        let url = &after_quote[..end];
        let safe = url_is_safe_href(url);
        out.push_str("href=");
        out.push(quote_char);
        if safe {
            out.push_str(url);
        }
        out.push(quote_char);
        rest = &after_quote[end + 1..];
    }
    out.push_str(rest);
    out
}

fn url_is_safe_href(url: &str) -> bool {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
}

fn attendee_response(label: &str) -> ResponseType {
    match label {
        "accepted" | "Accepted" => ResponseType::Accepted,
        "organizer" | "Organizer" => ResponseType::Organizer,
        "declined" | "Declined" => ResponseType::Declined,
        "tentativelyAccepted" | "TentativelyAccepted" | "tentative" | "Tentative" => {
            ResponseType::Tentative
        }
        _ => ResponseType::NotResponded,
    }
}

/// Attendee roster from Graph (`emailAddress{name,address}`) or Teams MT (`name`, `address`).
pub fn attendees_from_entry(entry: &Value) -> Vec<EventAttendee> {
    let Some(list) = entry.get("attendees").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|attendee| {
            let address = attendee
                .pointer("/emailAddress/address")
                .and_then(Value::as_str)
                .or_else(|| attendee.get("address").and_then(Value::as_str))
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let name = attendee
                .pointer("/emailAddress/name")
                .and_then(Value::as_str)
                .or_else(|| attendee.get("name").and_then(Value::as_str))
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| address.clone())?;
            let required = !attendee
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| !kind.eq_ignore_ascii_case("required"));
            Some(EventAttendee {
                name,
                address,
                response: attendee_response(attendee_response_label(attendee).unwrap_or("none")),
                required,
            })
        })
        .collect()
}

fn normalize_preview_whitespace(preview: &str) -> String {
    preview
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

pub fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for character in input.chars() {
        match character {
            '<' => {
                in_tag = true;
                // Keep a word break when tags sit flush against text (`</p><p>`).
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    html_entities(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn html_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

pub fn truncate_preview(preview: &str) -> String {
    // Long enough for a Teams peek agenda; UI scrolls instead of hard-clipping.
    const MAX_CHARS: usize = 4_000;
    let trimmed = preview.trim();
    if trimmed.chars().count() <= MAX_CHARS {
        return trimmed.to_owned();
    }
    let mut cut: String = trimmed.chars().take(MAX_CHARS).collect();
    cut.push('…');
    cut
}

fn series_flag(entry: &Value) -> bool {
    if flag(entry, "isRecurring") || flag(entry, "isSeriesMaster") {
        return true;
    }
    match text(entry, "eventType")
        .or_else(|| text(entry, "type"))
        .as_deref()
    {
        Some("occurrence" | "exception" | "seriesMaster") => true,
        _ => entry.get("seriesMasterId").and_then(Value::as_str).is_some()
            || entry.get("recurrence").is_some(),
    }
}

fn series_master_id(entry: &Value) -> Option<String> {
    text(entry, "seriesMasterId").or_else(|| text(entry, "seriesMasterObjectId"))
}

fn attendee_response_label(attendee: &Value) -> Option<&str> {
    attendee
        .pointer("/status/response")
        .and_then(Value::as_str)
        .or_else(|| attendee.get("responseStatus").and_then(Value::as_str))
}

pub fn response_counts_from_attendees(entry: &Value) -> Option<AttendeeResponseCounts> {
    let attendees = entry.get("attendees")?.as_array()?;
    if attendees.is_empty() {
        return None;
    }
    let mut counts = AttendeeResponseCounts::default();
    for attendee in attendees {
        match attendee_response_label(attendee).unwrap_or("none") {
            "accepted" | "Accepted" | "organizer" | "Organizer" => counts.accepted += 1,
            "declined" | "Declined" => counts.declined += 1,
            "tentativelyAccepted" | "TentativelyAccepted" | "tentative" | "Tentative" => {
                counts.tentative += 1
            }
            _ => counts.not_responded += 1,
        }
    }
    Some(counts)
}

/// Merge Graph / detail fields into a calendar event without wiping list data.
pub fn apply_event_details(event: &mut CalendarEvent, entry: &Value) {
    if let Some(location) = location_text(entry) {
        event.location = Some(location);
    }
    if let Some(preview) = body_preview_text(entry) {
        event.body_preview = Some(preview);
    }
    if let Some(html) = body_html_text(entry) {
        event.body_html = Some(html);
    }
    if series_flag(entry) {
        event.series = true;
    }
    if let Some(master) = series_master_id(entry) {
        event.series_master_id = Some(master);
        event.series = true;
    }
    if let Some(counts) = response_counts_from_attendees(entry) {
        event.response_counts = Some(counts);
    }
    let attendees = attendees_from_entry(entry);
    if !attendees.is_empty() {
        event.attendees = attendees;
    }
    if let Some(name) = entry
        .pointer("/organizer/emailAddress/name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        event.organizer_name = Some(name.to_owned());
    }
    if let Some(address) = entry
        .pointer("/organizer/emailAddress/address")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        event.organizer_address = Some(address.to_owned());
    }
    if event.join_url.is_none() {
        if let Some(url) = entry
            .pointer("/onlineMeeting/joinUrl")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| text(entry, "skypeTeamsMeetingUrl"))
            .or_else(|| text(entry, "joinUrl"))
        {
            event.join_url = Some(url);
            event.online = true;
        }
    }
}

fn parse_event(entry: &Value) -> Option<CalendarEvent> {
    let join_url = text(entry, "skypeTeamsMeetingUrl").or_else(|| text(entry, "joinUrl"));
    let (source, account_id, calendar_id, sources) = CalendarEvent::outlook_defaults();
    Some(CalendarEvent {
        object_id: text(entry, "objectId").or_else(|| text(entry, "id"))?,
        subject: text(entry, "subject").unwrap_or_default(),
        start: time(entry, "startTime")?,
        end: time(entry, "endTime")?,
        all_day: flag(entry, "isAllDayEvent") || flag(entry, "isAllDay"),
        cancelled: flag(entry, "isCancelled"),
        online: flag(entry, "isOnlineMeeting") || join_url.is_some(),
        join_url,
        thread_id: thread_id(entry),
        organizer_name: text(entry, "organizerName"),
        organizer_address: text(entry, "organizerAddress"),
        ical_uid: text(entry, "iCalUid")
            .or_else(|| text(entry, "iCalUId"))
            .or_else(|| text(entry, "iCalUID")),
        response: response_type(entry),
        show_as: show_as(entry),
        dial_in: dial_in(entry),
        source,
        account_id,
        calendar_id,
        sources,
        location: location_text(entry),
        body_preview: body_preview_text(entry),
        body_html: body_html_text(entry),
        series: series_flag(entry),
        series_master_id: series_master_id(entry),
        response_counts: response_counts_from_attendees(entry),
        attendees: attendees_from_entry(entry),
        reminders: Vec::new(),
        source_link: None,
        web_link: None,
        calendar_name: None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn join_info_carries_the_phone_dial_in_like_teams() {
        let entry = serde_json::json!({
            "onlineMeetingTollNumber": "+49 30 123456",
            "onlineMeetingConferenceId": "987654321",
        });
        let parsed = dial_in(&entry).expect("dial in");
        assert_eq!(
            join_info_text("https://teams.microsoft.com/l/meetup-join/x", Some(&parsed)),
            "Microsoft Teams meeting\nJoin: https://teams.microsoft.com/l/meetup-join/x\n\nDial in by phone\n+49 30 123456,,987654321#\nPhone conference ID: 987654321#"
        );
        assert_eq!(
            dial_in(&serde_json::json!({"onlineMeetingTollNumber": ""})),
            None
        );
        assert_eq!(
            join_info_text("https://x", None),
            "Microsoft Teams meeting\nJoin: https://x"
        );
    }

    #[test]
    fn meet_now_join_url_prefers_the_join_link() {
        let body = serde_json::json!({"value": {
            "links": {"join": "https://teams.microsoft.com/l/meetup-join/19%3ameeting_a%40thread.v2/0"},
            "meetingUrl": "https://example.invalid/other",
        }});
        assert_eq!(
            meet_now_join_url(&body).as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/19%3ameeting_a%40thread.v2/0")
        );
        assert_eq!(meet_now_join_url(&serde_json::json!({"value": {}})), None);
    }

    use std::sync::Mutex;

    use chrono::{Duration, TimeZone};
    use session::Scope;

    use super::*;

    struct Canned {
        body: Value,
        sent: Mutex<Vec<(Request, Scope)>>,
    }

    impl CardTransport for Canned {
        async fn send(&self, request: Request, scope: Scope) -> Result<ApiResponse> {
            self.sent.lock().unwrap().push((request, scope));
            Ok(ApiResponse {
                status: 200,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/calendar_view.json")).unwrap()
    }

    #[test]
    fn events_are_parsed_and_sorted_by_start() {
        let events = parse_calendar_events(&fixture());
        assert_eq!(events.len(), 4);
        let offsite = &events[0];
        assert!(offsite.all_day);
        assert!(!offsite.online);
        assert_eq!(offsite.join_url, None);
        assert_eq!(offsite.thread_id, None);
        assert_eq!(offsite.response, ResponseType::NotResponded);
        let standup = &events[1];
        assert_eq!(standup.subject, "Team standup");
        assert_eq!(
            standup.start,
            Utc.with_ymd_and_hms(2026, 10, 12, 8, 30, 0).unwrap()
        );
        assert_eq!(standup.organizer_name.as_deref(), Some("Mara Lindqvist"));
        assert_eq!(
            standup.join_url.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/19%3ameeting_demo1%40thread.v2/0")
        );
        assert!(standup.online);
        assert_eq!(standup.response, ResponseType::Accepted);
        assert_eq!(
            standup.thread_id.as_deref(),
            Some("19:meeting_demo1@thread.v2")
        );
        assert_eq!(
            standup.location.as_deref(),
            Some("Microsoft Teams Meeting")
        );
        assert_eq!(
            standup.body_preview.as_deref(),
            Some("Daily agenda and blockers")
        );
        assert!(standup.series);
        assert_eq!(
            standup.series_master_id.as_deref(),
            Some("evt-standup-master")
        );
        let counts = standup.response_counts.expect("counts");
        assert_eq!(counts.accepted, 2);
        assert_eq!(counts.declined, 1);
        assert_eq!(counts.not_responded, 2);
    }

    #[test]
    fn strip_html_flattens_markup_for_peek_preview() {
        assert_eq!(
            strip_html("<p>Hello&nbsp;<b>team</b></p>"),
            "Hello team"
        );
        assert_eq!(
            strip_html("<p>NEUE</p><p>Agenda</p>"),
            "NEUE Agenda"
        );
        assert_eq!(truncate_preview(&"x".repeat(4_100)).chars().count(), 4_001);
    }

    #[test]
    fn apply_event_details_fills_missing_peek_fields() {
        let mut event = CalendarEvent {
            object_id: "a".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 12, 8, 30, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 12, 9, 0, 0).unwrap(),
            ..Default::default()
        };
        apply_event_details(
            &mut event,
            &serde_json::json!({
                "bodyPreview": "Notes",
                "location": {"displayName": "Room A"},
                "type": "seriesMaster",
                "seriesMasterId": "master",
                "attendees": [
                    {"status": {"response": "accepted"}},
                    {"status": {"response": "tentativelyAccepted"}},
                ],
                "onlineMeeting": {"joinUrl": "https://teams.microsoft.com/l/meetup-join/z"}
            }),
        );
        assert_eq!(event.body_preview.as_deref(), Some("Notes"));
        assert_eq!(event.location.as_deref(), Some("Room A"));
        assert!(event.series);
        assert_eq!(event.series_master_id.as_deref(), Some("master"));
        assert_eq!(event.join_url.as_deref(), Some("https://teams.microsoft.com/l/meetup-join/z"));
        let counts = event.response_counts.expect("counts");
        assert_eq!(counts.accepted, 1);
        assert_eq!(counts.tentative, 1);
    }

    #[test]
    fn parses_teams_ical_uid_and_body_content() {
        let events = parse_calendar_events(&serde_json::json!({
            "value": [{
                "objectId": "evt-1",
                "subject": "Standup",
                "startTime": "2026-11-09T11:00:00Z",
                "endTime": "2026-11-09T11:15:00Z",
                "iCalUID": "uid-from-teams",
                "bodyContent": "<p>NEUE TERMINE</p><p>Agenda</p>",
                "location": "Microsoft Teams-Besprechung"
            }]
        }));
        assert_eq!(events[0].ical_uid.as_deref(), Some("uid-from-teams"));
        assert_eq!(
            events[0].body_preview.as_deref(),
            Some("NEUE TERMINE Agenda")
        );
        assert_eq!(
            events[0].location.as_deref(),
            Some("Microsoft Teams-Besprechung")
        );
    }

    #[test]
    fn text_body_normalizes_lone_carriage_returns() {
        let mut event = CalendarEvent {
            object_id: "cr".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 12, 8, 30, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 12, 9, 0, 0).unwrap(),
            ..Default::default()
        };
        apply_event_details(
            &mut event,
            &serde_json::json!({
                "body": {"contentType": "text", "content": "Line1\rLine2\r\nLine3"}
            }),
        );
        assert_eq!(
            event.body_html.as_deref(),
            Some("Line1<br>Line2<br>Line3")
        );
    }

    #[test]
    fn html_body_drops_javascript_hrefs() {
        let mut event = CalendarEvent {
            object_id: "js".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 12, 8, 30, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 12, 9, 0, 0).unwrap(),
            ..Default::default()
        };
        apply_event_details(
            &mut event,
            &serde_json::json!({
                "body": {
                    "contentType": "html",
                    "content": r#"<p><a href="javascript:alert(1)">x</a> <a href='https://ok.example'>y</a></p>"#
                }
            }),
        );
        let html = event.body_html.as_deref().expect("html");
        assert!(!html.to_ascii_lowercase().contains("javascript:"));
        assert!(html.contains(r#"href="""#) || html.contains("href=''"));
        assert!(html.contains(r#"href='https://ok.example'"#));
    }

    #[test]
    fn apply_event_details_uses_body_content_when_preview_missing() {
        let mut event = CalendarEvent {
            object_id: "b".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 12, 8, 30, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 12, 9, 0, 0).unwrap(),
            ..Default::default()
        };
        apply_event_details(
            &mut event,
            &serde_json::json!({
                "body": {"contentType": "html", "content": "<p>NEUE TERMINE!!!!!</p><p>Agenda</p>"},
                "attendees": [{"status": {"response": "accepted"}}]
            }),
        );
        assert_eq!(
            event.body_preview.as_deref(),
            Some("NEUE TERMINE!!!!! Agenda")
        );
        assert_eq!(
            event.body_html.as_deref(),
            Some("<p>NEUE TERMINE!!!!!</p><p>Agenda</p>")
        );
    }

    #[test]
    fn attendees_parse_graph_and_teams_shapes() {
        let mut event = CalendarEvent::default();
        apply_event_details(
            &mut event,
            &serde_json::json!({
                "attendees": [
                    {"emailAddress": {"name": "Ada", "address": "ada@x.ch"},
                     "status": {"response": "accepted"}, "type": "required"},
                    {"name": "Bob", "address": "bob@x.ch",
                     "status": {"response": "NotResponded"}, "type": "Optional"},
                    {"address": "eve@x.ch", "status": {"response": "Declined"}}
                ]
            }),
        );
        let roster: Vec<_> = event
            .attendees
            .iter()
            .map(|a| (a.name.as_str(), a.response, a.required))
            .collect();
        assert_eq!(
            roster,
            vec![
                ("Ada", ResponseType::Accepted, true),
                ("Bob", ResponseType::NotResponded, false),
                ("eve@x.ch", ResponseType::Declined, true),
            ]
        );
    }

    #[test]
    fn plain_text_body_becomes_escaped_html() {
        let mut event = CalendarEvent::default();
        apply_event_details(
            &mut event,
            &serde_json::json!({"body": {"contentType": "text", "content": "a < b\r\nnext"}}),
        );
        assert_eq!(event.body_html.as_deref(), Some("a &lt; b<br>next"));
    }

    #[test]
    fn zone_less_times_and_embedded_thread_data_are_understood() {
        let events = parse_calendar_events(&fixture());
        let review = &events[2];
        assert_eq!(review.response, ResponseType::Tentative);
        assert_eq!(
            review.thread_id.as_deref(),
            Some("19:meeting_demo2@thread.v2")
        );
        assert_eq!(
            review.start,
            Utc.with_ymd_and_hms(2026, 10, 12, 13, 0, 0).unwrap()
        );
        assert!(events[3].cancelled);
    }

    #[test]
    fn the_organizer_flag_and_calendar_uid_are_read() {
        let body = serde_json::json!({"value": [{
            "objectId": "a",
            "startTime": "2026-10-12T08:30:00Z",
            "endTime": "2026-10-12T09:00:00Z",
            "isOrganizer": true,
            "myResponseType": "Accepted",
            "iCalUid": "uid-a"
        }]});
        let events = parse_calendar_events(&body);
        assert_eq!(events[0].response, ResponseType::Organizer);
        assert_eq!(events[0].ical_uid.as_deref(), Some("uid-a"));
    }

    #[test]
    fn the_free_busy_status_is_read_and_defaults_to_busy() {
        let entry = |show_as: &str| {
            serde_json::json!({
                "objectId": show_as,
                "startTime": "2026-10-12T08:30:00Z",
                "endTime": "2026-10-12T09:00:00Z",
                "showAs": show_as
            })
        };
        let body = serde_json::json!({"value": [entry("Free"), entry("Tentative"), entry("Oof"), entry("Unknown")]});
        let statuses: Vec<ShowAs> = parse_calendar_events(&body)
            .iter()
            .map(|event| event.show_as)
            .collect();
        assert_eq!(
            statuses,
            [
                ShowAs::Free,
                ShowAs::Tentative,
                ShowAs::OutOfOffice,
                ShowAs::Busy
            ]
        );
    }

    #[test]
    fn entries_without_an_id_or_times_are_skipped() {
        let body =
            serde_json::json!({"value": [{"subject": "x"}, {"objectId": "a", "subject": "y"}]});
        assert!(parse_calendar_events(&body).is_empty());
    }

    #[tokio::test]
    async fn calendar_view_gets_the_range_with_the_spaces_scope() {
        let calendar = Calendar::with_transport(
            Canned {
                body: fixture(),
                sent: Mutex::new(Vec::new()),
            },
            "emea",
        );
        let start = Utc.with_ymd_and_hms(2026, 10, 12, 0, 0, 0).unwrap();
        let events = calendar
            .calendar_view(start, start + Duration::days(7))
            .await
            .unwrap();
        assert_eq!(events.len(), 4);
        let sent = calendar.transport.sent.lock().unwrap();
        assert_eq!(
            sent[0].0.url,
            "https://teams.cloud.microsoft/api/mt/emea/v2.1/me/calendars/calendarView?startDate=2026-10-12T00:00:00Z&endDate=2026-10-19T00:00:00Z"
        );
        assert_eq!(sent[0].1, spaces_scope());
    }
}
