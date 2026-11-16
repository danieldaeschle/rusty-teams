use chrono::{DateTime, Utc};

use crate::calendar::Repeat;
use crate::google_calendar::rrule_for_repeat;

#[derive(Debug, Clone)]
pub struct CalDavEventDraft {
    pub uid: String,
    pub summary: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub description: Option<String>,
    pub location: Option<String>,
    pub join_url: Option<String>,
    pub attendees: Vec<String>,
    pub repeat: Repeat,
    pub sequence: u32,
}

/// Build a VCALENDAR 2.0 document containing one VEVENT (no METHOD).
pub fn build_vevent(draft: &CalDavEventDraft, stamp: DateTime<Utc>) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//rusty-teams//CalDAV//EN".to_owned(),
        "CALSCALE:GREGORIAN".to_owned(),
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}", draft.uid),
        format!("DTSTAMP:{}", format_utc(stamp)),
        format!("SEQUENCE:{}", draft.sequence),
        format!("SUMMARY:{}", escape(&draft.summary)),
    ];
    if draft.all_day {
        lines.push(format!(
            "DTSTART;VALUE=DATE:{}",
            draft.start.format("%Y%m%d")
        ));
        lines.push(format!("DTEND;VALUE=DATE:{}", draft.end.format("%Y%m%d")));
    } else {
        lines.push(format!("DTSTART:{}", format_utc(draft.start)));
        lines.push(format!("DTEND:{}", format_utc(draft.end)));
    }
    let mut description = draft.description.clone().unwrap_or_default();
    if let Some(join) = &draft.join_url {
        if !description.is_empty() {
            description.push_str("\n\n");
        }
        description.push_str(join);
        lines.push(format!("URL:{join}"));
    }
    if !description.is_empty() {
        lines.push(format!("DESCRIPTION:{}", escape(&description)));
    }
    if let Some(location) = &draft.location {
        lines.push(format!("LOCATION:{}", escape(location)));
    }
    if let Some(rrule) = rrule_for_repeat(draft.repeat, draft.start.naive_utc()) {
        let rule = rrule.trim_start_matches("RRULE:");
        lines.push(format!("RRULE:{rule}"));
    }
    for attendee in &draft.attendees {
        lines.push(format!("ATTENDEE:mailto:{attendee}"));
    }
    lines.push("END:VEVENT".to_owned());
    lines.push("END:VCALENDAR".to_owned());
    lines.join("\r\n") + "\r\n"
}

fn format_utc(dt: DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn builds_without_method() {
        let ics = build_vevent(
            &CalDavEventDraft {
                uid: "u1".into(),
                summary: "Meet".into(),
                start: Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap(),
                end: Utc.with_ymd_and_hms(2026, 1, 1, 11, 0, 0).unwrap(),
                all_day: false,
                description: None,
                location: None,
                join_url: Some("https://teams.microsoft.com/l/meetup-join/x".into()),
                attendees: vec!["a@b.c".into()],
                repeat: Repeat::Never,
                sequence: 0,
            },
            Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
        );
        assert!(ics.contains("BEGIN:VCALENDAR"));
        assert!(!ics.contains("METHOD:"));
        assert!(ics.contains("URL:https://teams.microsoft.com/l/meetup-join/x"));
        assert!(ics.contains("ATTENDEE:mailto:a@b.c"));
    }
}
