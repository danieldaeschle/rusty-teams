use chrono::{DateTime, Datelike, NaiveDateTime, Utc, Weekday};
use serde_json::{Value, json};
use session::{GRAPH, Method, Request, Scope};

use super::Calendar;
use crate::cards::CardTransport;
use crate::error::{Error, Result};
use crate::messages::encode;

pub(super) const CALENDARS_SCOPE: &str = "Calendars.ReadWrite";
const GRAPH_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const GRAPH_DATE_FORMAT: &str = "%Y-%m-%d";
const TIME_ZONE: &str = "UTC";
const ONLINE_PROVIDER: &str = "teamsForBusiness";
const NOT_FOUND: u16 = 404;
const WORK_WEEK: [Weekday; 5] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Never,
    Daily,
    Weekdays,
    Weekly,
}

impl Repeat {
    pub const ALL: [Repeat; 4] = [
        Repeat::Never,
        Repeat::Daily,
        Repeat::Weekdays,
        Repeat::Weekly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Repeat::Never => "Does not repeat",
            Repeat::Daily => "Daily",
            Repeat::Weekdays => "Weekdays",
            Repeat::Weekly => "Weekly",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendee {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEvent {
    pub subject: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub local_start: NaiveDateTime,
    pub local_end: NaiveDateTime,
    pub time_zone_name: Option<String>,
    pub attendees: Vec<Attendee>,
    pub repeat: Repeat,
    pub details: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedEvent {
    pub id: String,
    pub join_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Accept,
    Tentative,
    Decline,
}

impl Reply {
    fn action(self) -> &'static str {
        match self {
            Reply::Accept => "accept",
            Reply::Tentative => "tentativelyAccept",
            Reply::Decline => "decline",
        }
    }
}

fn day_name(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "monday",
        Weekday::Tue => "tuesday",
        Weekday::Wed => "wednesday",
        Weekday::Thu => "thursday",
        Weekday::Fri => "friday",
        Weekday::Sat => "saturday",
        Weekday::Sun => "sunday",
    }
}

fn graph_time(event: &NewEvent, local: NaiveDateTime, utc: DateTime<Utc>) -> Value {
    match &event.time_zone_name {
        Some(time_zone_name) => json!({
            "dateTime": local.format(GRAPH_TIME_FORMAT).to_string(),
            "timeZone": time_zone_name,
        }),
        None => json!({
            "dateTime": utc.format(GRAPH_TIME_FORMAT).to_string(),
            "timeZone": TIME_ZONE,
        }),
    }
}

fn recurrence(repeat: Repeat, start: NaiveDateTime) -> Option<Value> {
    let (pattern_type, days): (&str, Vec<Weekday>) = match repeat {
        Repeat::Never => return None,
        Repeat::Daily => ("daily", Vec::new()),
        Repeat::Weekdays => ("weekly", WORK_WEEK.to_vec()),
        Repeat::Weekly => ("weekly", vec![start.weekday()]),
    };
    let mut pattern = json!({"type": pattern_type, "interval": 1});
    if !days.is_empty() {
        pattern["daysOfWeek"] = days.into_iter().map(day_name).collect();
        pattern["firstDayOfWeek"] = json!("monday");
    }
    Some(json!({
        "pattern": pattern,
        "range": {
            "type": "noEnd",
            "startDate": start.format(GRAPH_DATE_FORMAT).to_string(),
        },
    }))
}

pub fn new_event_body(event: &NewEvent) -> Value {
    let mut body = json!({
        "subject": event.subject,
        "start": graph_time(event, event.local_start, event.start),
        "end": graph_time(event, event.local_end, event.end),
        "attendees": event.attendees.iter().map(|attendee| json!({
            "type": "required",
            "emailAddress": {"address": attendee.address, "name": attendee.name},
        })).collect::<Vec<_>>(),
        "isOnlineMeeting": true,
        "onlineMeetingProvider": ONLINE_PROVIDER,
    });
    if !event.details.trim().is_empty() {
        body["body"] = json!({"contentType": "text", "content": event.details});
    }
    if let Some(recurrence) = recurrence(event.repeat, event.local_start) {
        body["recurrence"] = recurrence;
    }
    body
}

pub fn parse_created_event(body: &Value) -> Option<CreatedEvent> {
    Some(CreatedEvent {
        id: body.get("id")?.as_str()?.to_owned(),
        join_url: body
            .pointer("/onlineMeeting/joinUrl")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn events_url() -> String {
    format!("{GRAPH}/v1.0/me/events")
}

fn reply_url(event_id: &str, reply: Reply) -> String {
    format!("{}/{}/{}", events_url(), encode(event_id), reply.action())
}

fn lookup_url(ical_uid: &str) -> String {
    format!(
        "{}?$filter={}&$select=id&$top=1",
        events_url(),
        encode(&format!("iCalUId eq '{}'", ical_uid.replace('\'', "''")))
    )
}

fn api_error(answer: session::ApiResponse, url: &str) -> Error {
    Error::Session(session::Error::api(answer.status, url, answer.body))
}

impl<T: CardTransport> Calendar<T> {
    pub async fn create_event(&self, event: &NewEvent) -> Result<CreatedEvent> {
        let url = events_url();
        let request = Request::with_body(Method::Post, &url, new_event_body(event));
        let answer = self
            .transport
            .send(request, Scope::graph(CALENDARS_SCOPE))
            .await?;
        if !answer.is_success() {
            return Err(api_error(answer, &url));
        }
        parse_created_event(&answer.body).ok_or_else(|| api_error(answer, &url))
    }

    pub async fn respond(
        &self,
        event_id: &str,
        ical_uid: Option<&str>,
        reply: Reply,
        send_response: bool,
    ) -> Result<()> {
        let answer = self.send_reply(event_id, reply, send_response).await?;
        if answer.is_success() {
            return Ok(());
        }
        let Some(ical_uid) = ical_uid.filter(|_| answer.status == NOT_FOUND) else {
            return Err(api_error(answer, &reply_url(event_id, reply)));
        };
        let graph_id = self.graph_event_id(ical_uid).await?;
        let retried = self.send_reply(&graph_id, reply, send_response).await?;
        if retried.is_success() {
            return Ok(());
        }
        Err(api_error(retried, &reply_url(&graph_id, reply)))
    }

    async fn send_reply(
        &self,
        event_id: &str,
        reply: Reply,
        send_response: bool,
    ) -> Result<session::ApiResponse> {
        let request = Request::with_body(
            Method::Post,
            reply_url(event_id, reply),
            json!({"comment": "", "sendResponse": send_response}),
        );
        self.transport
            .send(request, Scope::graph(CALENDARS_SCOPE))
            .await
    }

    async fn graph_event_id(&self, ical_uid: &str) -> Result<String> {
        let url = lookup_url(ical_uid);
        let answer = self
            .transport
            .send(Request::get(&url), Scope::graph(CALENDARS_SCOPE))
            .await?;
        if !answer.is_success() {
            return Err(api_error(answer, &url));
        }
        answer
            .body
            .pointer("/value/0/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| api_error(answer, &url))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::*;
    use crate::calendar::Calendar;

    struct Scripted {
        answers: Mutex<Vec<(u16, Value)>>,
        sent: Mutex<Vec<(Method, String, Option<Value>)>>,
    }

    impl Scripted {
        fn new(answers: Vec<(u16, Value)>) -> Self {
            Scripted {
                answers: Mutex::new(answers),
                sent: Mutex::new(Vec::new()),
            }
        }
    }

    impl CardTransport for Scripted {
        async fn send(&self, request: Request, scope: Scope) -> Result<session::ApiResponse> {
            assert_eq!(scope, Scope::graph(CALENDARS_SCOPE));
            self.sent
                .lock()
                .unwrap()
                .push((request.method, request.url, request.body));
            let (status, body) = self.answers.lock().unwrap().remove(0);
            Ok(session::ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    fn local(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    fn zoned(repeat: Repeat, local_start: NaiveDateTime, offset_hours: i64) -> NewEvent {
        let utc_start = local_start.and_utc() - chrono::Duration::hours(offset_hours);
        NewEvent {
            start: utc_start,
            end: utc_start + chrono::Duration::minutes(30),
            local_start,
            local_end: local_start + chrono::Duration::minutes(30),
            time_zone_name: Some("W. Europe Standard Time".into()),
            ..event(repeat)
        }
    }

    fn event(repeat: Repeat) -> NewEvent {
        NewEvent {
            subject: "Planning".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 14, 9, 30, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 14, 10, 0, 0).unwrap(),
            local_start: local(2026, 10, 14, 11, 30),
            local_end: local(2026, 10, 14, 12, 0),
            time_zone_name: None,
            attendees: vec![Attendee {
                name: "Mara Lindqvist".into(),
                address: "mara@example.invalid".into(),
            }],
            repeat,
            details: String::new(),
        }
    }

    #[test]
    fn the_body_asks_for_a_teams_meeting_with_utc_times() {
        let body = new_event_body(&event(Repeat::Never));
        assert_eq!(body["subject"], "Planning");
        assert_eq!(body["isOnlineMeeting"], true);
        assert_eq!(body["onlineMeetingProvider"], "teamsForBusiness");
        assert_eq!(body["start"]["dateTime"], "2026-10-14T09:30:00");
        assert_eq!(body["start"]["timeZone"], "UTC");
        assert_eq!(body["end"]["dateTime"], "2026-10-14T10:00:00");
        assert_eq!(body["attendees"][0]["type"], "required");
        assert_eq!(
            body["attendees"][0]["emailAddress"]["address"],
            "mara@example.invalid"
        );
        assert!(body.get("recurrence").is_none());
        assert!(body.get("body").is_none());
    }

    #[test]
    fn summer_and_winter_series_keep_the_same_local_time() {
        let summer = new_event_body(&zoned(Repeat::Weekly, local(2026, 7, 15, 9, 0), 2));
        let winter = new_event_body(&zoned(Repeat::Weekly, local(2026, 12, 16, 9, 0), 1));
        assert_eq!(summer["start"]["dateTime"], "2026-07-15T09:00:00");
        assert_eq!(winter["start"]["dateTime"], "2026-12-16T09:00:00");
        for body in [&summer, &winter] {
            assert_eq!(body["start"]["timeZone"], "W. Europe Standard Time");
            assert_eq!(body["end"]["dateTime"].as_str().unwrap()[11..], *"09:30:00");
            assert_eq!(body["end"]["timeZone"], "W. Europe Standard Time");
        }
    }

    #[test]
    fn a_local_half_past_midnight_keeps_its_local_weekday_and_start_date() {
        let body = new_event_body(&zoned(Repeat::Weekly, local(2026, 10, 14, 0, 30), 2));
        assert_eq!(body["start"]["dateTime"], "2026-10-14T00:30:00");
        assert_eq!(body["recurrence"]["range"]["startDate"], "2026-10-14");
        assert_eq!(
            body["recurrence"]["pattern"]["daysOfWeek"],
            json!(["wednesday"])
        );
    }

    #[test]
    fn without_a_zone_name_times_stay_utc_but_the_date_is_local() {
        let event = NewEvent {
            start: Utc.with_ymd_and_hms(2026, 10, 13, 22, 30, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 13, 23, 0, 0).unwrap(),
            local_start: local(2026, 10, 14, 0, 30),
            local_end: local(2026, 10, 14, 1, 0),
            ..event(Repeat::Weekly)
        };
        let body = new_event_body(&event);
        assert_eq!(body["start"]["dateTime"], "2026-10-13T22:30:00");
        assert_eq!(body["start"]["timeZone"], "UTC");
        assert_eq!(body["recurrence"]["range"]["startDate"], "2026-10-14");
        assert_eq!(
            body["recurrence"]["pattern"]["daysOfWeek"],
            json!(["wednesday"])
        );
    }

    #[test]
    fn details_become_a_plain_text_body() {
        let body = new_event_body(&NewEvent {
            details: "Agenda: roadmap".into(),
            ..event(Repeat::Never)
        });
        assert_eq!(body["body"]["contentType"], "text");
        assert_eq!(body["body"]["content"], "Agenda: roadmap");
    }

    #[test]
    fn recurrence_maps_to_graph_patterns() {
        let daily = new_event_body(&event(Repeat::Daily));
        assert_eq!(daily["recurrence"]["pattern"]["type"], "daily");
        assert_eq!(daily["recurrence"]["pattern"]["interval"], 1);
        assert_eq!(daily["recurrence"]["range"]["type"], "noEnd");
        assert_eq!(daily["recurrence"]["range"]["startDate"], "2026-10-14");
        let weekdays = new_event_body(&event(Repeat::Weekdays));
        assert_eq!(weekdays["recurrence"]["pattern"]["type"], "weekly");
        assert_eq!(
            weekdays["recurrence"]["pattern"]["daysOfWeek"],
            json!(["monday", "tuesday", "wednesday", "thursday", "friday"])
        );
        let weekly = new_event_body(&event(Repeat::Weekly));
        assert_eq!(
            weekly["recurrence"]["pattern"]["daysOfWeek"],
            json!(["wednesday"])
        );
    }

    #[test]
    fn the_created_event_carries_its_id_and_join_link() {
        let body =
            json!({"id": "AAMk", "onlineMeeting": {"joinUrl": "https://example.invalid/join"}});
        assert_eq!(
            parse_created_event(&body),
            Some(CreatedEvent {
                id: "AAMk".into(),
                join_url: Some("https://example.invalid/join".into()),
            })
        );
        assert_eq!(parse_created_event(&json!({})), None);
    }

    #[tokio::test]
    async fn create_event_posts_to_graph_events() {
        let calendar =
            Calendar::with_transport(Scripted::new(vec![(201, json!({"id": "AAMk"}))]), "emea");
        let created = calendar.create_event(&event(Repeat::Never)).await.unwrap();
        assert_eq!(created.id, "AAMk");
        let sent = calendar.transport.sent.lock().unwrap();
        assert_eq!(sent[0].0, Method::Post);
        assert_eq!(sent[0].1, "https://graph.microsoft.com/v1.0/me/events");
    }

    #[tokio::test]
    async fn create_event_failure_carries_the_service_message() {
        let calendar = Calendar::with_transport(
            Scripted::new(vec![(403, json!({"error": {"message": "Access denied"}}))]),
            "emea",
        );
        let error = calendar
            .create_event(&event(Repeat::Never))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Access denied"));
    }

    #[tokio::test]
    async fn respond_posts_the_reply_with_the_send_flag() {
        let calendar = Calendar::with_transport(Scripted::new(vec![(202, Value::Null)]), "emea");
        calendar
            .respond("AAMk=", None, Reply::Tentative, false)
            .await
            .unwrap();
        let sent = calendar.transport.sent.lock().unwrap();
        assert_eq!(
            sent[0].1,
            "https://graph.microsoft.com/v1.0/me/events/AAMk%3D/tentativelyAccept"
        );
        assert_eq!(
            sent[0].2,
            Some(json!({"comment": "", "sendResponse": false}))
        );
    }

    #[tokio::test]
    async fn an_unknown_id_is_looked_up_by_its_ical_uid_once() {
        let calendar = Calendar::with_transport(
            Scripted::new(vec![
                (404, json!({"error": {"message": "not found"}})),
                (200, json!({"value": [{"id": "graph-id"}]})),
                (202, Value::Null),
            ]),
            "emea",
        );
        calendar
            .respond("mt-id", Some("uid-1"), Reply::Decline, true)
            .await
            .unwrap();
        let sent = calendar.transport.sent.lock().unwrap();
        assert!(sent[1].1.contains("iCalUId%20eq%20'uid-1'"));
        assert!(sent[2].1.ends_with("/graph-id/decline"));
    }

    #[tokio::test]
    async fn a_missing_event_without_an_ical_uid_fails() {
        let calendar = Calendar::with_transport(
            Scripted::new(vec![(404, json!({"error": {"message": "not found"}}))]),
            "emea",
        );
        assert!(
            calendar
                .respond("mt-id", None, Reply::Accept, true)
                .await
                .is_err()
        );
    }
}
