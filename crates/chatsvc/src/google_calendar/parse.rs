use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde_json::Value;

use crate::calendar::{
    AttendeeResponseCounts, CalendarEvent, CalendarSource, EventAttendee, ResponseType, ShowAs,
    strip_html, truncate_preview,
};

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
    let defaults = default_reminder_minutes(body);
    let mut events: Vec<CalendarEvent> = body
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| parse_event(entry, account_id, calendar_id, &defaults))
        .collect();
    events.sort_by_key(|event| (event.start, event.end));
    events
}

fn parse_event(
    entry: &Value,
    account_id: Option<&str>,
    calendar_id: Option<&str>,
    default_reminders: &[u32],
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
    let location = text(entry, "location").filter(|value| !value.contains("teams.microsoft.com"));
    let description = text(entry, "description");
    let body_preview = description
        .as_ref()
        .map(|value| truncate_preview(&strip_html(value)))
        .filter(|value| !value.is_empty());
    let body_html = description
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| description_html(value));
    let series = entry
        .get("recurrence")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
        || entry.get("recurringEventId").is_some();
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
        location,
        body_preview,
        body_html,
        series,
        series_master_id: text(entry, "recurringEventId"),
        response_counts: google_response_counts(entry),
        attendees: google_attendees(entry),
        reminders: google_reminders(entry, default_reminders),
        source_link: google_source_link(entry),
        web_link: text(entry, "htmlLink"),
        calendar_name: None,
    })
}

/// Plain Google descriptions become escaped HTML with links and line breaks.
/// Descriptions that already look like HTML are kept as-is.
pub fn description_html(text: &str) -> String {
    if looks_like_html(text) {
        return text.to_owned();
    }
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let with_links = linkify_escaped(&escaped);
    with_links
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>")
}

fn looks_like_html(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("<br")
        || lower.contains("<p")
        || lower.contains("<div")
        || lower.contains("<a ")
        || lower.contains("<b>")
        || lower.contains("<i>")
        || lower.contains("<ul")
        || lower.contains("<ol")
        || lower.contains("<li")
}

fn linkify_escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let next_https = rest.find("https://");
        let next_http = rest.find("http://");
        let (at, scheme) = match (next_https, next_http) {
            (Some(a), Some(b)) if a <= b => (a, "https://"),
            (Some(a), None) => (a, "https://"),
            (None, Some(b)) => (b, "http://"),
            (None, None) => {
                out.push_str(rest);
                break;
            }
            (Some(_), Some(b)) => (b, "http://"),
        };
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let after = &rest[scheme.len()..];
        let end = after
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, '<' | '>' | '"' | '\'' | ')' | ']' | '}')
            })
            .unwrap_or(after.len());
        let mut url = format!("{}{}", scheme, &after[..end]);
        while url
            .chars()
            .last()
            .is_some_and(|character| matches!(character, '.' | ',' | ';' | ':' | '!'))
        {
            url.pop();
        }
        let href = url.replace('"', "&quot;");
        out.push_str(&format!(r#"<a href="{href}">{url}</a>"#));
        rest = &rest[url.len()..];
    }
    out
}

fn default_reminder_minutes(body: &Value) -> Vec<u32> {
    reminder_minutes_from(body.get("defaultReminders"))
}

fn google_reminders(entry: &Value, defaults: &[u32]) -> Vec<u32> {
    let reminders = entry.get("reminders");
    if reminders
        .and_then(|value| value.get("overrides"))
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        return reminder_minutes_from(reminders.and_then(|value| value.get("overrides")));
    }
    if reminders
        .and_then(|value| value.get("useDefault"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return defaults.to_vec();
    }
    Vec::new()
}

fn reminder_minutes_from(list: Option<&Value>) -> Vec<u32> {
    let Some(items) = list.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut minutes: Vec<u32> = items
        .iter()
        .filter_map(|item| {
            let method = item.get("method").and_then(Value::as_str).unwrap_or("popup");
            if method != "popup" && method != "email" {
                return None;
            }
            item.get("minutes")
                .and_then(Value::as_u64)
                .map(|value| value as u32)
        })
        .collect();
    minutes.sort_unstable();
    minutes.dedup();
    minutes
}

fn google_source_link(entry: &Value) -> Option<(String, String)> {
    let source = entry.get("source")?;
    let url = text(source, "url").filter(|value| !value.is_empty())?;
    let title = text(source, "title")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| url.clone());
    Some((title, url))
}

fn google_attendees(entry: &Value) -> Vec<EventAttendee> {
    let Some(list) = entry.get("attendees").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter(|attendee| attendee.get("resource").and_then(Value::as_bool) != Some(true))
        .filter_map(|attendee| {
            let address = text(attendee, "email");
            let name = text(attendee, "displayName").or_else(|| address.clone())?;
            let response = match attendee.get("responseStatus").and_then(Value::as_str) {
                Some("accepted") => ResponseType::Accepted,
                Some("declined") => ResponseType::Declined,
                Some("tentative") => ResponseType::Tentative,
                _ => ResponseType::NotResponded,
            };
            Some(EventAttendee {
                name,
                address,
                response,
                required: attendee.get("optional").and_then(Value::as_bool) != Some(true),
            })
        })
        .collect()
}

fn google_response_counts(entry: &Value) -> Option<AttendeeResponseCounts> {
    let attendees = entry.get("attendees")?.as_array()?;
    if attendees.is_empty() {
        return None;
    }
    let mut counts = AttendeeResponseCounts::default();
    for attendee in attendees {
        match attendee
            .get("responseStatus")
            .and_then(Value::as_str)
            .unwrap_or("needsAction")
        {
            "accepted" => counts.accepted += 1,
            "declined" => counts.declined += 1,
            "tentative" => counts.tentative += 1,
            _ => counts.not_responded += 1,
        }
    }
    Some(counts)
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
        Some("accepted")
            if entry.pointer("/organizer/self").and_then(Value::as_bool) == Some(true) =>
        {
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
    fn parses_location_description_series_and_counts() {
        let body = serde_json::json!({
            "items": [{
                "id": "evt2",
                "summary": "Standup",
                "description": "Bring updates",
                "location": "Building 3",
                "recurrence": ["RRULE:FREQ=WEEKLY"],
                "start": {"dateTime": "2026-11-11T09:00:00Z"},
                "end": {"dateTime": "2026-11-11T09:30:00Z"},
                "attendees": [
                    {"self": true, "responseStatus": "accepted"},
                    {"responseStatus": "declined"},
                    {"responseStatus": "needsAction"}
                ]
            }]
        });
        let events = parse_google_events(&body, Some("acc"), Some("primary"));
        assert_eq!(events[0].location.as_deref(), Some("Building 3"));
        assert_eq!(events[0].body_preview.as_deref(), Some("Bring updates"));
        assert!(events[0].series);
        let counts = events[0].response_counts.expect("counts");
        assert_eq!(counts.accepted, 1);
        assert_eq!(counts.declined, 1);
        assert_eq!(counts.not_responded, 1);
    }

    #[test]
    fn strips_html_from_google_description() {
        let body = serde_json::json!({
            "items": [{
                "id": "html",
                "summary": "Notes",
                "description": "<b>Agenda</b><br> Item&nbsp;1 &amp; notes",
                "start": {"dateTime": "2026-11-11T09:00:00Z"},
                "end": {"dateTime": "2026-11-11T09:30:00Z"}
            }]
        });
        let events = parse_google_events(&body, None, None);
        assert_eq!(
            events[0].body_preview.as_deref(),
            Some("Agenda Item 1 & notes")
        );
        assert_eq!(
            events[0].body_html.as_deref(),
            Some("<b>Agenda</b><br> Item&nbsp;1 &amp; notes")
        );
    }

    #[test]
    fn plain_description_keeps_breaks_and_links() {
        let html = description_html(
            "source: llm\nCloud Native\nhttps://email.linuxfoundation.org/foo?x=1\nFrom: CNCF",
        );
        assert!(html.contains("source: llm<br>Cloud Native<br>"));
        assert!(html.contains(
            r#"<a href="https://email.linuxfoundation.org/foo?x=1">https://email.linuxfoundation.org/foo?x=1</a>"#
        ));
        assert!(html.ends_with("From: CNCF") || html.contains("From: CNCF"));
        assert!(!html.contains("<br><br>From") || html.contains("<br>From: CNCF"));
    }

    #[test]
    fn reminders_from_overrides_and_defaults() {
        let body = serde_json::json!({
            "defaultReminders": [
                {"method": "popup", "minutes": 10},
                {"method": "email", "minutes": 1440}
            ],
            "items": [
                {
                    "id": "override",
                    "summary": "A",
                    "start": {"dateTime": "2026-11-11T09:00:00Z"},
                    "end": {"dateTime": "2026-11-11T09:30:00Z"},
                    "reminders": {
                        "useDefault": false,
                        "overrides": [
                            {"method": "popup", "minutes": 5},
                            {"method": "popup", "minutes": 30},
                            {"method": "email", "minutes": 15}
                        ]
                    },
                    "source": {"title": "[Mail] KubeCon", "url": "https://mail.example/1"},
                    "htmlLink": "https://calendar.google.com/event?eid=abc"
                },
                {
                    "id": "default",
                    "summary": "B",
                    "start": {"dateTime": "2026-11-11T10:00:00Z"},
                    "end": {"dateTime": "2026-11-11T10:30:00Z"},
                    "reminders": {"useDefault": true}
                }
            ]
        });
        let events = parse_google_events(&body, None, None);
        assert_eq!(events[0].reminders, vec![5, 15, 30]);
        assert_eq!(
            events[0].source_link.as_ref().map(|(t, u)| (t.as_str(), u.as_str())),
            Some(("[Mail] KubeCon", "https://mail.example/1"))
        );
        assert_eq!(
            events[0].web_link.as_deref(),
            Some("https://calendar.google.com/event?eid=abc")
        );
        assert_eq!(events[1].reminders, vec![10, 1440]);
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
