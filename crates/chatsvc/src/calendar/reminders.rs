use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{Value, json};
use session::{GRAPH, Method, Request, Scope};

use super::Calendar;
use super::schedule::CALENDARS_SCOPE;
use crate::cards::CardTransport;
use crate::error::{Error, Result};
use crate::messages::encode;

const QUERY_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";
const GRAPH_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const GRAPH_READ_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.f";
const UTC_PREFERENCE: &str = "outlook.timezone=\"UTC\"";
const MAX_PAGES: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub event_id: String,
    pub fire_time: DateTime<Utc>,
}

fn reminder_view_url(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    format!(
        "{GRAPH}/v1.0/me/reminderView(startDateTime='{}',endDateTime='{}')",
        start.format(QUERY_TIME_FORMAT),
        end.format(QUERY_TIME_FORMAT)
    )
}

fn reminder_action_url(event_id: &str, action: &str) -> String {
    format!("{GRAPH}/v1.0/me/events/{}/{action}", encode(event_id))
}

fn utc_time(value: &Value) -> Option<DateTime<Utc>> {
    let text = value.get("dateTime")?.as_str()?;
    let zone = value
        .get("timeZone")
        .and_then(Value::as_str)
        .unwrap_or("UTC");
    if !zone.eq_ignore_ascii_case("UTC") {
        return None;
    }
    NaiveDateTime::parse_from_str(text, GRAPH_READ_FORMAT)
        .ok()
        .map(|time| time.and_utc())
}

pub fn parse_reminders(body: &Value) -> Vec<Reminder> {
    body.get("value")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some(Reminder {
                event_id: entry.get("eventId")?.as_str()?.to_owned(),
                fire_time: utc_time(entry.get("reminderFireTime")?)?,
            })
        })
        .collect()
}

impl<T: CardTransport> Calendar<T> {
    pub async fn reminder_view(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Reminder>> {
        let mut url = reminder_view_url(start, end);
        let mut reminders = Vec::new();
        for _ in 0..MAX_PAGES {
            let mut request = Request::get(&url);
            request
                .headers
                .push(("Prefer".to_owned(), UTC_PREFERENCE.to_owned()));
            let answer = self
                .transport
                .send(request, Scope::graph(CALENDARS_SCOPE))
                .await?;
            if !answer.is_success() {
                return Err(Error::Session(session::Error::api(
                    answer.status,
                    &url,
                    answer.body,
                )));
            }
            reminders.extend(parse_reminders(&answer.body));
            match answer.body.get("@odata.nextLink").and_then(Value::as_str) {
                Some(next) => url = next.to_owned(),
                None => break,
            }
        }
        Ok(reminders)
    }

    pub async fn snooze_reminder(&self, event_id: &str, until: DateTime<Utc>) -> Result<()> {
        let body = json!({"newReminderTime": {
            "dateTime": until.format(GRAPH_TIME_FORMAT).to_string(),
            "timeZone": "UTC",
        }});
        self.reminder_action(event_id, "snoozeReminder", Some(body))
            .await
    }

    pub async fn dismiss_reminder(&self, event_id: &str) -> Result<()> {
        self.reminder_action(event_id, "dismissReminder", None)
            .await
    }

    async fn reminder_action(
        &self,
        event_id: &str,
        action: &str,
        body: Option<Value>,
    ) -> Result<()> {
        let url = reminder_action_url(event_id, action);
        let request = Request::with_body(Method::Post, &url, body.unwrap_or_else(|| json!({})));
        let answer = self
            .transport
            .send(request, Scope::graph(CALENDARS_SCOPE))
            .await?;
        if answer.is_success() {
            return Ok(());
        }
        Err(Error::Session(session::Error::api(
            answer.status,
            &url,
            answer.body,
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;
    use session::ApiResponse;

    use super::*;

    struct Scripted {
        answers: Mutex<Vec<(u16, Value)>>,
        sent: Mutex<Vec<Request>>,
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
        async fn send(&self, request: Request, scope: Scope) -> Result<ApiResponse> {
            assert_eq!(scope, Scope::graph(CALENDARS_SCOPE));
            self.sent.lock().unwrap().push(request);
            let (status, body) = self.answers.lock().unwrap().remove(0);
            Ok(ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 19, hour, minute, 0).unwrap()
    }

    fn reminder_entry(event_id: &str, fire_time: &str) -> Value {
        json!({
            "eventId": event_id,
            "eventSubject": "Daily Standup",
            "eventStartTime": {"dateTime": "2026-10-19T09:45:00.0000000", "timeZone": "UTC"},
            "reminderFireTime": {"dateTime": fire_time, "timeZone": "UTC"},
        })
    }

    #[test]
    fn reminders_carry_the_event_id_and_the_fire_time() {
        let body = json!({"value": [
            reminder_entry("AAMk-a", "2026-10-19T09:30:00.0000000"),
            {"eventId": "AAMk-b", "reminderFireTime": {"dateTime": "2026-10-19T09:30:00", "timeZone": "W. Europe Standard Time"}},
            {"reminderFireTime": {"dateTime": "2026-10-19T09:30:00", "timeZone": "UTC"}},
        ]});
        assert_eq!(
            parse_reminders(&body),
            vec![Reminder {
                event_id: "AAMk-a".into(),
                fire_time: at(9, 30),
            }]
        );
    }

    #[tokio::test]
    async fn the_reminder_view_asks_for_utc_and_follows_the_next_link() {
        let calendar = Calendar::with_transport(
            Scripted::new(vec![
                (
                    200,
                    json!({
                        "value": [reminder_entry("AAMk-a", "2026-10-19T09:30:00.0000000")],
                        "@odata.nextLink": "https://graph.microsoft.com/v1.0/next-page",
                    }),
                ),
                (
                    200,
                    json!({"value": [reminder_entry("AAMk-b", "2026-10-19T09:40:00.0000000")]}),
                ),
            ]),
            "emea",
        );
        let reminders = calendar.reminder_view(at(9, 0), at(11, 0)).await.unwrap();
        assert_eq!(reminders.len(), 2);
        let sent = calendar.transport.sent.lock().unwrap();
        assert_eq!(
            sent[0].url,
            "https://graph.microsoft.com/v1.0/me/reminderView(startDateTime='2026-10-19T09:00:00Z',endDateTime='2026-10-19T11:00:00Z')"
        );
        assert!(
            sent[0]
                .headers
                .contains(&("Prefer".to_owned(), UTC_PREFERENCE.to_owned()))
        );
        assert_eq!(sent[1].url, "https://graph.microsoft.com/v1.0/next-page");
    }

    #[tokio::test]
    async fn snooze_posts_the_new_reminder_time_for_the_occurrence() {
        let calendar = Calendar::with_transport(Scripted::new(vec![(200, Value::Null)]), "emea");
        calendar
            .snooze_reminder("AAMk+a/b=", at(9, 45))
            .await
            .unwrap();
        let sent = calendar.transport.sent.lock().unwrap();
        assert_eq!(sent[0].method, Method::Post);
        assert_eq!(
            sent[0].url,
            "https://graph.microsoft.com/v1.0/me/events/AAMk%2Ba%2Fb%3D/snoozeReminder"
        );
        assert_eq!(
            sent[0].body,
            Some(
                json!({"newReminderTime": {"dateTime": "2026-10-19T09:45:00", "timeZone": "UTC"}})
            )
        );
    }

    #[tokio::test]
    async fn dismiss_posts_and_reports_a_failure() {
        let calendar = Calendar::with_transport(
            Scripted::new(vec![
                (200, Value::Null),
                (
                    404,
                    json!({"error": {"message": "The specified object was not found"}}),
                ),
            ]),
            "emea",
        );
        calendar.dismiss_reminder("AAMk-a").await.unwrap();
        let error = calendar.dismiss_reminder("AAMk-gone").await.unwrap_err();
        assert!(error.to_string().contains("not found"));
        let sent = calendar.transport.sent.lock().unwrap();
        assert_eq!(
            sent[0].url,
            "https://graph.microsoft.com/v1.0/me/events/AAMk-a/dismissReminder"
        );
    }
}
