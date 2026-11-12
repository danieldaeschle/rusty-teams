use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde_json::Value;

use crate::calendar::{CalendarEvent, CalendarSource, ResponseType, ShowAs};

pub fn parse_calendar_list(body: &Value) -> Vec<(String, String, bool)> {
    body.get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some((
                entry.get("id")?.as_str()?.to_owned(),
                entry
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("Calendar")
                    .to_owned(),
                entry
                    .get("primary")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ))
        })
        .collect()
}

pub fn parse_google_events(
    body: &Value,
    account_id: Option<&str>,
    calendar_id: Option<&str>,
) -> Vec<CalendarEvent> {
    let mut events: Vec<CalendarEvent> = body
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| parse_event(entry, account_id, calendar_id))
        .collect();
    events.sort_by_key(|event| (event.start, event.end));
    events
}

fn parse_event(
    entry: &Value,
    account_id: Option<&str>,
    calendar_id: Option<&str>,
) -> Option<CalendarEvent> {
    if entry.get("status").and_then(Value::as_str) == Some("cancelled") {
        // Still show cancelled when times exist.
    }
    let object_id = entry.get("id")?.as_str()?.to_owned();
    let (start, all_day) = event_instant(entry.get("start")?)?;
    let (end, _) = event_instant(entry.get("end")?)?;
    let join_url = conference_join_url(entry)
        .or_else(|| text(entry, "location").filter(|value| value.contains("teams.microsoft.com")))
        .or_else(|| {
            text(entry, "description").and_then(|description| {
                description
                    .split_whitespace()
                    .find(|token| token.contains("teams.microsoft.com"))
                    .map(str::to_owned)
            })
        });
    Some(CalendarEvent {
        object_id,
        subject: text(entry, "summary").unwrap_or_default(),
        start,
        end,
        all_day,
        cancelled: entry.get("status").and_then(Value::as_str) == Some("cancelled"),
        online: join_url.is_some(),
        join_url,
        thread_id: None,
        organizer_name: entry
            .pointer("/organizer/displayName")
            .and_then(Value::as_str)
            .map(str::to_owned),
        organizer_address: entry
            .pointer("/organizer/email")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ical_uid: text(entry, "iCalUID"),
        response: response_status(entry),
        show_as: ShowAs::Busy,
        dial_in: None,
        source: CalendarSource::Google,
        account_id: account_id.map(str::to_owned),
        calendar_id: calendar_id.map(str::to_owned),
        sources: vec![CalendarSource::Google],
    })
}

fn conference_join_url(entry: &Value) -> Option<String> {
    entry
        .pointer("/conferenceData/entryPoints")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|point| {
            if point.get("entryPointType").and_then(Value::as_str) == Some("video") {
                text(point, "uri")
            } else {
                None
            }
        })
}

fn event_instant(value: &Value) -> Option<(DateTime<Utc>, bool)> {
    if let Some(date) = value.get("date").and_then(Value::as_str) {
        let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        let start = day.and_time(NaiveTime::MIN).and_utc();
        return Some((start, true));
    }
    let date_time = value.get("dateTime")?.as_str()?;
    DateTime::parse_from_rfc3339(date_time)
        .ok()
        .map(|time| (time.with_timezone(&Utc), false))
}

fn response_status(entry: &Value) -> ResponseType {
    let status = entry
        .get("attendees")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|attendee| attendee.get("self").and_then(Value::as_bool) == Some(true))
        .and_then(|attendee| attendee.get("responseStatus").and_then(Value::as_str))
        .or_else(|| {
            entry
                .pointer("/organizer/self")
                .and_then(Value::as_bool)
                .filter(|value| *value)
                .map(|_| "accepted")
        });
    match status {
        Some("accepted") if entry.pointer("/organizer/self").and_then(Value::as_bool) == Some(true) => {
            ResponseType::Organizer
        }
        Some("accepted") => ResponseType::Accepted,
        Some("tentative") => ResponseType::Tentative,
        Some("declined") => ResponseType::Declined,
        _ => ResponseType::NotResponded,
    }
}

fn text(entry: &Value, key: &str) -> Option<String> {
    entry.get(key)?.as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timed_event_with_teams_link_in_location() {
        let body = serde_json::json!({
            "items": [{
                "id": "evt1",
                "summary": "Standup",
                "iCalUID": "uid-1",
                "start": {"dateTime": "2026-11-11T09:00:00Z"},
                "end": {"dateTime": "2026-11-11T09:30:00Z"},
                "location": "https://teams.microsoft.com/l/meetup-join/abc",
                "attendees": [{"self": true, "responseStatus": "accepted"}]
            }]
        });
        let events = parse_google_events(&body, Some("acc"), Some("primary"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subject, "Standup");
        assert_eq!(events[0].source, CalendarSource::Google);
        assert_eq!(events[0].account_id.as_deref(), Some("acc"));
        assert_eq!(
            events[0].join_url.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/abc")
        );
        assert_eq!(events[0].response, ResponseType::Accepted);
    }

    #[test]
    fn parses_all_day_and_calendar_list() {
        let body = serde_json::json!({
            "items": [{
                "id": "day",
                "summary": "Offsite",
                "start": {"date": "2026-11-12"},
                "end": {"date": "2026-11-13"},
                "status": "confirmed"
            }]
        });
        let events = parse_google_events(&body, None, None);
        assert!(events[0].all_day);
        let list = parse_calendar_list(&serde_json::json!({
            "items": [
                {"id": "primary", "summary": "Joe", "primary": true},
                {"id": "work", "summary": "Work"}
            ]
        }));
        assert_eq!(list[0].0, "primary");
        assert!(list[0].2);
    }
}
