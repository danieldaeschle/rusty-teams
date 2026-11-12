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
}

impl CalendarEvent {
    pub fn outlook_defaults() -> (CalendarSource, Option<String>, Option<String>, Vec<CalendarSource>) {
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
        }
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
        ical_uid: text(entry, "iCalUid").or_else(|| text(entry, "iCalUId")),
        response: response_type(entry),
        show_as: show_as(entry),
        dial_in: dial_in(entry),
        source,
        account_id,
        calendar_id,
        sources,
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
        assert_eq!(dial_in(&serde_json::json!({"onlineMeetingTollNumber": ""})), None);
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
            [ShowAs::Free, ShowAs::Tentative, ShowAs::OutOfOffice, ShowAs::Busy]
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
