use chrono::Datelike;
use serde_json::{Value, json};

use crate::calendar::{Attendee, NewEvent, Repeat, Reply};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleEventDraft {
    pub subject: String,
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
    pub local: Option<LocalTimes>,
    pub attendees: Vec<Attendee>,
    pub details: String,
    pub join_url: Option<String>,
    pub repeat: Repeat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTimes {
    pub start: chrono::NaiveDateTime,
    pub end: chrono::NaiveDateTime,
    pub iana_zone: String,
}

const LOCAL_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

impl From<&NewEvent> for GoogleEventDraft {
    fn from(event: &NewEvent) -> Self {
        GoogleEventDraft {
            subject: event.subject.clone(),
            start: event.start,
            end: event.end,
            local: event
                .time_zone_name
                .as_ref()
                .and_then(|_| iana_time_zone::get_timezone().ok())
                .map(|iana_zone| LocalTimes {
                    start: event.local_start,
                    end: event.local_end,
                    iana_zone,
                }),
            attendees: event.attendees.clone(),
            details: event.details.clone(),
            join_url: None,
            repeat: event.repeat,
        }
    }
}

pub fn rrule_for_repeat(repeat: Repeat, start: chrono::NaiveDateTime) -> Option<String> {
    match repeat {
        Repeat::Never => None,
        Repeat::Daily => Some("RRULE:FREQ=DAILY".to_owned()),
        Repeat::Weekdays => Some("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".to_owned()),
        Repeat::Weekly => {
            let day = match start.weekday() {
                chrono::Weekday::Mon => "MO",
                chrono::Weekday::Tue => "TU",
                chrono::Weekday::Wed => "WE",
                chrono::Weekday::Thu => "TH",
                chrono::Weekday::Fri => "FR",
                chrono::Weekday::Sat => "SA",
                chrono::Weekday::Sun => "SU",
            };
            Some(format!("RRULE:FREQ=WEEKLY;BYDAY={day}"))
        }
    }
}

pub fn google_event_body(draft: &GoogleEventDraft) -> Value {
    let mut description = draft.details.clone();
    if let Some(join_url) = &draft.join_url {
        if !description.is_empty() {
            description.push_str("\n\n");
        }
        description.push_str(join_url);
    }
    let (start, end) = match &draft.local {
        Some(local) => (
            json!({
                "dateTime": local.start.format(LOCAL_TIME_FORMAT).to_string(),
                "timeZone": local.iana_zone,
            }),
            json!({
                "dateTime": local.end.format(LOCAL_TIME_FORMAT).to_string(),
                "timeZone": local.iana_zone,
            }),
        ),
        None => (
            json!({
                "dateTime": draft.start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "timeZone": "UTC",
            }),
            json!({
                "dateTime": draft.end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "timeZone": "UTC",
            }),
        ),
    };
    let series_start = draft
        .local
        .as_ref()
        .map_or(draft.start.naive_utc(), |local| local.start);
    let mut body = json!({
        "summary": draft.subject,
        "start": start,
        "end": end,
        "attendees": draft.attendees.iter().map(|attendee| json!({
            "email": attendee.address,
            "displayName": attendee.name,
        })).collect::<Vec<_>>(),
    });
    if let Some(join_url) = &draft.join_url {
        body["location"] = json!(join_url);
    }
    if !description.is_empty() {
        body["description"] = json!(description);
    }
    if let Some(rrule) = rrule_for_repeat(draft.repeat, series_start) {
        body["recurrence"] = json!([rrule]);
    }
    body
}

pub fn google_rsvp_status(reply: Reply) -> &'static str {
    match reply {
        Reply::Accept => "accepted",
        Reply::Tentative => "tentative",
        Reply::Decline => "declined",
    }
}

pub fn google_rsvp_query(send_updates: bool) -> String {
    format!("sendUpdates={}", if send_updates { "all" } else { "none" })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn draft(repeat: Repeat) -> GoogleEventDraft {
        GoogleEventDraft {
            subject: "Sync".into(),
            start: chrono::Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap(),
            end: chrono::Utc
                .with_ymd_and_hms(2026, 11, 11, 9, 30, 0)
                .unwrap(),
            local: None,
            attendees: vec![Attendee {
                name: "Mara".into(),
                address: "mara@example.com".into(),
            }],
            details: "Notes".into(),
            join_url: Some("https://teams.microsoft.com/l/meetup-join/x".into()),
            repeat,
        }
    }

    #[test]
    fn a_weekly_series_repeats_in_the_local_zone_on_the_local_weekday() {
        let mut weekly = draft(Repeat::Weekly);
        let late_monday = chrono::NaiveDate::from_ymd_opt(2026, 6, 8)
            .unwrap()
            .and_hms_opt(0, 30, 0)
            .unwrap();
        weekly.local = Some(LocalTimes {
            start: late_monday,
            end: late_monday + chrono::Duration::minutes(30),
            iana_zone: "Europe/Berlin".into(),
        });
        let body = google_event_body(&weekly);
        assert_eq!(body["start"]["dateTime"], "2026-06-08T00:30:00");
        assert_eq!(body["start"]["timeZone"], "Europe/Berlin");
        assert_eq!(body["recurrence"][0], "RRULE:FREQ=WEEKLY;BYDAY=MO");
    }

    #[test]
    fn body_puts_join_link_in_location_and_description() {
        let body = google_event_body(&draft(Repeat::Never));
        assert_eq!(
            body["location"],
            "https://teams.microsoft.com/l/meetup-join/x"
        );
        assert!(
            body["description"]
                .as_str()
                .unwrap()
                .contains("teams.microsoft.com")
        );
        assert!(body.get("recurrence").is_none());
    }

    #[test]
    fn weekdays_rrule_and_rsvp_query() {
        assert_eq!(
            rrule_for_repeat(
                Repeat::Weekdays,
                chrono::Utc
                    .with_ymd_and_hms(2026, 11, 11, 9, 0, 0)
                    .unwrap()
                    .naive_utc()
            )
            .as_deref(),
            Some("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR")
        );
        assert_eq!(google_rsvp_status(Reply::Decline), "declined");
        assert_eq!(google_rsvp_query(true), "sendUpdates=all");
        assert_eq!(google_rsvp_query(false), "sendUpdates=none");
    }
}
