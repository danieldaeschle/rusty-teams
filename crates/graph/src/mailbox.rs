//! Outlook mailbox working hours (`/me/mailboxSettings`), the setting Teams and
//! Outlook read for work days and hours.
//!
//! `MailboxSettings.Read` is not granted to the Teams client; `ReadWrite` is.

use chrono::{NaiveTime, Weekday};
use serde_json::{Value, json};
use session::{Method, Scope};

use crate::client::Graph;
use crate::error::{Error, Result};
use crate::urls;

const MAILBOX_SCOPE: &str = "MailboxSettings.ReadWrite";
const TIME_FORMAT: &str = "%H:%M:%S%.f";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxWorkingHours {
    pub days: Vec<Weekday>,
    pub start: NaiveTime,
    pub end: NaiveTime,
    /// Windows or IANA name as the mailbox stores it; written back unchanged.
    pub time_zone: Option<String>,
    /// Mailbox language (`en-CH`); Outlook derives the first day of week from it.
    pub locale: Option<String>,
}

impl Graph {
    pub async fn working_hours(&self) -> Result<MailboxWorkingHours> {
        let url = urls::mailbox_settings();
        let body = self.get(&url, &Scope::graph(MAILBOX_SCOPE)).await?;
        parse_working_hours(&body)
            .ok_or_else(|| Error::from(session::Error::api(200, &url, body.clone())))
    }

    pub async fn set_working_hours(&self, hours: &MailboxWorkingHours) -> Result<()> {
        self.session()
            .request(
                Method::Patch,
                &urls::mailbox_settings(),
                &Scope::graph(MAILBOX_SCOPE),
                Some(working_hours_body(hours)),
            )
            .await?;
        Ok(())
    }
}

pub fn parse_working_hours(body: &Value) -> Option<MailboxWorkingHours> {
    let hours = body.get("workingHours")?;
    let days = hours
        .get("daysOfWeek")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|day| day.parse::<Weekday>().ok())
        .collect();
    let time = |key: &str| {
        let text = hours.get(key)?.as_str()?;
        NaiveTime::parse_from_str(text, TIME_FORMAT)
            .or_else(|_| NaiveTime::parse_from_str(text, "%H:%M"))
            .ok()
    };
    Some(MailboxWorkingHours {
        days,
        start: time("startTime")?,
        end: time("endTime")?,
        time_zone: hours
            .pointer("/timeZone/name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        locale: body
            .pointer("/language/locale")
            .and_then(Value::as_str)
            .filter(|locale| !locale.is_empty())
            .map(str::to_owned),
    })
}

pub fn working_hours_body(hours: &MailboxWorkingHours) -> Value {
    let mut working = json!({
        "daysOfWeek": hours.days.iter().map(|day| day_name(*day)).collect::<Vec<_>>(),
        "startTime": graph_time(hours.start),
        "endTime": graph_time(hours.end),
    });
    if let Some(name) = &hours.time_zone {
        working["timeZone"] = json!({ "name": name });
    }
    json!({ "workingHours": working })
}

/// Graph writes seven fraction digits; working hours never carry fractions.
fn graph_time(time: NaiveTime) -> String {
    format!("{}.0000000", time.format("%H:%M:%S"))
}

fn day_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "monday",
        Weekday::Tue => "tuesday",
        Weekday::Wed => "wednesday",
        Weekday::Thu => "thursday",
        Weekday::Fri => "friday",
        Weekday::Sat => "saturday",
        Weekday::Sun => "sunday",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured() -> Value {
        // Shape of GET /me/mailboxSettings as returned on 2026-11-15.
        json!({
            "timeFormat": "HH:mm",
            "dateFormat": "d MMM yyyy",
            "language": {"locale": "en-CH", "displayName": "English (Switzerland)"},
            "workingHours": {
                "daysOfWeek": ["monday", "tuesday", "wednesday", "thursday", "friday"],
                "startTime": "08:00:00.0000000",
                "endTime": "17:00:00.0000000",
                "timeZone": {"name": "W. Europe Standard Time"}
            }
        })
    }

    #[test]
    fn mailbox_working_hours_parse() {
        let hours = parse_working_hours(&captured()).unwrap();
        assert_eq!(
            hours.days,
            [
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri
            ]
        );
        assert_eq!(hours.start, NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        assert_eq!(hours.end, NaiveTime::from_hms_opt(17, 0, 0).unwrap());
        assert_eq!(hours.time_zone.as_deref(), Some("W. Europe Standard Time"));
        assert_eq!(hours.locale.as_deref(), Some("en-CH"));
    }

    #[test]
    fn mailbox_working_hours_body_round_trips() {
        let mut hours = parse_working_hours(&captured()).unwrap();
        hours.days.retain(|day| *day != Weekday::Fri);
        hours.start = NaiveTime::from_hms_opt(7, 30, 0).unwrap();
        let body = working_hours_body(&hours);
        assert_eq!(
            body["workingHours"]["daysOfWeek"],
            json!(["monday", "tuesday", "wednesday", "thursday"])
        );
        assert_eq!(body["workingHours"]["startTime"], "07:30:00.0000000");
        assert_eq!(
            body["workingHours"]["timeZone"]["name"],
            "W. Europe Standard Time"
        );
        let parsed = parse_working_hours(&body).unwrap();
        assert_eq!(
            (parsed.days, parsed.start, parsed.end),
            (hours.days, hours.start, hours.end)
        );
    }

    #[test]
    fn mailbox_without_working_hours_is_none() {
        assert!(parse_working_hours(&json!({"timeFormat": "HH:mm"})).is_none());
    }
}
