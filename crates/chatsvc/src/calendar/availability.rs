use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{Value, json};
use session::{GRAPH, Method, Request, Scope};

use super::Calendar;
use super::schedule::CALENDARS_SCOPE;
use crate::cards::CardTransport;
use crate::error::{Error, Result};

const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const READ_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.f";
const TIME_ZONE: &str = "UTC";
const VIEW_INTERVAL_MINUTES: u32 = 30;
const FREE: &str = "free";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusyTime {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonSchedule {
    pub email: String,
    pub busy: Vec<BusyTime>,
    pub error: Option<String>,
}

fn schedule_url() -> String {
    format!("{GRAPH}/v1.0/me/calendar/getSchedule")
}

pub fn schedule_request_body(emails: &[String], start: DateTime<Utc>, end: DateTime<Utc>) -> Value {
    let stamp = |moment: DateTime<Utc>| json!({"dateTime": moment.format(TIME_FORMAT).to_string(), "timeZone": TIME_ZONE});
    json!({
        "schedules": emails,
        "startTime": stamp(start),
        "endTime": stamp(end),
        "availabilityViewInterval": VIEW_INTERVAL_MINUTES,
    })
}

fn utc_time(entry: &Value) -> Option<DateTime<Utc>> {
    let text = entry.get("dateTime")?.as_str()?;
    NaiveDateTime::parse_from_str(text, READ_FORMAT)
        .ok()
        .map(|time| time.and_utc())
}

fn busy_time(item: &Value) -> Option<BusyTime> {
    let status = item.get("status")?.as_str()?.to_owned();
    if status.eq_ignore_ascii_case(FREE) {
        return None;
    }
    Some(BusyTime {
        start: utc_time(item.get("start")?)?,
        end: utc_time(item.get("end")?)?,
        status,
    })
}

pub fn parse_schedules(body: &Value) -> Vec<PersonSchedule> {
    body.get("value")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some(PersonSchedule {
                email: entry.get("scheduleId")?.as_str()?.to_owned(),
                busy: entry
                    .get("scheduleItems")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(busy_time)
                    .collect(),
                error: entry
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

impl<T: CardTransport> Calendar<T> {
    pub async fn schedules(
        &self,
        emails: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<PersonSchedule>> {
        let url = schedule_url();
        let mut request = Request::with_body(Method::Post, &url, schedule_request_body(emails, start, end));
        request
            .headers
            .push(("Prefer".to_owned(), format!("outlook.timezone=\"{TIME_ZONE}\"")));
        let answer = self
            .transport
            .send(request, Scope::graph(CALENDARS_SCOPE))
            .await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(answer.status, &url, answer.body)));
        }
        Ok(parse_schedules(&answer.body))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::{parse_schedules, schedule_request_body};

    #[test]
    fn the_request_asks_in_utc() {
        let start = Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap();
        let body = schedule_request_body(&["mara@example.com".to_owned()], start, start + chrono::Duration::hours(10));
        assert_eq!(body["startTime"]["dateTime"], "2026-11-20T08:00:00");
        assert_eq!(body["endTime"]["timeZone"], "UTC");
        assert_eq!(body["schedules"][0], "mara@example.com");
    }

    #[test]
    fn busy_slots_and_errors_are_read_free_ones_dropped() {
        let body = json!({"value": [
            {"scheduleId": "mara@example.com", "scheduleItems": [
                {"status": "busy", "start": {"dateTime": "2026-11-20T09:00:00.0000000", "timeZone": "UTC"}, "end": {"dateTime": "2026-11-20T10:00:00.0000000", "timeZone": "UTC"}},
                {"status": "free", "start": {"dateTime": "2026-11-20T11:00:00.0000000", "timeZone": "UTC"}, "end": {"dateTime": "2026-11-20T12:00:00.0000000", "timeZone": "UTC"}}
            ]},
            {"scheduleId": "guest@other.com", "scheduleItems": [], "error": {"message": "not found"}}
        ]});
        let schedules = parse_schedules(&body);
        assert_eq!(schedules[0].busy.len(), 1);
        assert_eq!(schedules[0].busy[0].start, Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap());
        assert_eq!(schedules[1].error.as_deref(), Some("not found"));
    }
}
