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

/// Add EXDATE for one occurrence on the series master and bump SEQUENCE.
/// Leaves overrides and other VEVENT blocks untouched.
pub fn exclude_occurrence_from_ics(ics: &str, occurrence: DateTime<Utc>) -> Result<String, String> {
    let mut out = Vec::new();
    let mut in_event = false;
    let mut is_master = false;
    let mut event_lines: Vec<String> = Vec::new();
    let mut patched = false;

    for raw in ics.lines() {
        let line = raw.trim_end_matches('\r').to_owned();
        let upper = line.to_ascii_uppercase();
        if upper == "BEGIN:VEVENT" {
            in_event = true;
            is_master = false;
            event_lines = vec![line];
            continue;
        }
        if !in_event {
            out.push(line);
            continue;
        }
        if upper == "END:VEVENT" {
            event_lines.push(line);
            if is_master {
                out.extend(patch_master_exdate(event_lines, occurrence)?);
                patched = true;
            } else {
                out.extend(event_lines);
            }
            in_event = false;
            event_lines = Vec::new();
            continue;
        }
        if upper.starts_with("RRULE") || upper.starts_with("RDATE") {
            is_master = true;
        }
        event_lines.push(line);
    }
    if !patched {
        return Err("CalDAV series master with RRULE/RDATE not found".into());
    }
    Ok(out.join("\r\n") + "\r\n")
}

fn patch_master_exdate(
    mut lines: Vec<String>,
    occurrence: DateTime<Utc>,
) -> Result<Vec<String>, String> {
    let all_day = lines.iter().any(|line| {
        let upper = line.to_ascii_uppercase();
        upper.starts_with("DTSTART") && upper.contains("VALUE=DATE") && !upper.contains("DATE-TIME")
    });
    let (exdate, value) = if all_day {
        let value = occurrence.format("%Y%m%d").to_string();
        (format!("EXDATE;VALUE=DATE:{value}"), value)
    } else {
        let value = format_utc(occurrence);
        (format!("EXDATE:{value}"), value)
    };
    let mut sequence_bumped = false;
    let mut excluded = false;
    for line in &mut lines {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("SEQUENCE:") {
            let current = line.split_once(':').map(|(_, v)| v).unwrap_or("0");
            let next = current.trim().parse::<u32>().unwrap_or(0).saturating_add(1);
            *line = format!("SEQUENCE:{next}");
            sequence_bumped = true;
        }
        if upper.starts_with("EXDATE")
            && line
                .split_once(':')
                .is_some_and(|(_, values)| values.split(',').any(|item| item == value))
        {
            excluded = true;
        }
    }
    if !sequence_bumped {
        let at = lines
            .iter()
            .position(|l| l.to_ascii_uppercase().starts_with("UID:"))
            .map_or(1, |idx| idx + 1);
        lines.insert(at, "SEQUENCE:1".to_owned());
    }
    if !excluded {
        let Some(idx) = lines.iter().position(|l| {
            let upper = l.to_ascii_uppercase();
            upper.starts_with("RRULE") || upper.starts_with("RDATE")
        }) else {
            return Err("CalDAV series master missing RRULE/RDATE".into());
        };
        lines.insert(idx + 1, exdate);
    }
    Ok(lines)
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

    #[test]
    fn exclude_occurrence_keeps_a_zoned_exdate_and_adds_its_own_line() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:series\r\nDTSTART;TZID=Europe/Berlin:20260105T100000\r\nRRULE:FREQ=WEEKLY\r\nEXDATE;TZID=Europe/Berlin:20260105T100000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let patched =
            exclude_occurrence_from_ics(ics, Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap())
                .unwrap();
        assert!(patched.contains("EXDATE;TZID=Europe/Berlin:20260105T100000\r\n"));
        assert!(patched.contains("EXDATE:20260112T090000Z\r\n"));
    }

    #[test]
    fn exclude_occurrence_of_an_all_day_series_uses_a_date() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:series\r\nDTSTART;VALUE=DATE:20260105\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let patched =
            exclude_occurrence_from_ics(ics, Utc.with_ymd_and_hms(2026, 1, 12, 0, 0, 0).unwrap())
                .unwrap();
        assert!(patched.contains("EXDATE;VALUE=DATE:20260112\r\n"));
    }

    #[test]
    fn exclude_occurrence_adds_exdate_and_keeps_rrule() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:series\r\nSEQUENCE:2\r\nDTSTART:20260105T090000Z\r\nDTEND:20260105T100000Z\r\nRRULE:FREQ=WEEKLY\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let patched =
            exclude_occurrence_from_ics(ics, Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap())
                .unwrap();
        assert!(patched.contains("RRULE:FREQ=WEEKLY"));
        assert!(patched.contains("EXDATE:20260112T090000Z"));
        assert!(patched.contains("SEQUENCE:3"));
    }
}
