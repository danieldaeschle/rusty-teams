use chrono::{Datelike, Timelike};
use serde_json::{Value, json};

use crate::calendar::{Attendee, NewEvent, Repeat, Reply};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleEventDraft {
    pub subject: String,
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
    pub attendees: Vec<Attendee>,
    pub details: String,
    pub join_url: Option<String>,
    pub repeat: Repeat,
}

impl From<&NewEvent> for GoogleEventDraft {
    fn from(event: &NewEvent) -> Self {
        GoogleEventDraft {
            subject: event.subject.clone(),
            start: event.start,
            end: event.end,
            attendees: event.attendees.clone(),
            details: event.details.clone(),
            join_url: None,
            repeat: event.repeat,
        }
    }
}

pub fn rrule_for_repeat(repeat: Repeat, start: chrono::DateTime<chrono::Utc>) -> Option<String> {
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
    let mut body = json!({
        "summary": draft.subject,
        "start": {
            "dateTime": draft.start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "timeZone": "UTC",
        },
        "end": {
            "dateTime": draft.end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "timeZone": "UTC",
        },
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
    if let Some(rrule) = rrule_for_repeat(draft.repeat, draft.start) {
        body["recurrence"] = json!([rrule]);
    }
    let _ = (draft.start.hour(), draft.start.minute());
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
    format!(
        "sendUpdates={}",
        if send_updates { "all" } else { "none" }
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn draft(repeat: Repeat) -> GoogleEventDraft {
        GoogleEventDraft {
            subject: "Sync".into(),
            start: chrono::Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap(),
            end: chrono::Utc.with_ymd_and_hms(2026, 11, 11, 9, 30, 0).unwrap(),
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
                chrono::Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap()
            )
            .as_deref(),
            Some("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR")
        );
        assert_eq!(google_rsvp_status(Reply::Decline), "declined");
        assert_eq!(google_rsvp_query(true), "sendUpdates=all");
        assert_eq!(google_rsvp_query(false), "sendUpdates=none");
    }
}
