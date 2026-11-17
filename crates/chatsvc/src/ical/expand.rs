use chrono::{DateTime, TimeZone, Utc};
use rrule::{RRuleSet, Tz};

use super::parse::ParsedEvent;

/// Expand masters with RRULE into the window; apply EXDATE and RECURRENCE-ID overrides.
pub fn expand_events(
    events: Vec<ParsedEvent>,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Vec<ParsedEvent> {
    let mut overrides: Vec<ParsedEvent> = Vec::new();
    let mut masters: Vec<ParsedEvent> = Vec::new();
    for event in events {
        if event.recurrence_id.is_some() {
            overrides.push(event);
        } else if event.rrule.is_some() || !event.rdates.is_empty() {
            masters.push(event);
        } else if event.start < window_end && event.end > window_start {
            masters.push(event);
        }
    }

    let mut out = Vec::new();
    for master in masters {
        if master.rrule.is_none() && master.rdates.is_empty() {
            out.push(master);
            continue;
        }
        let duration = master.end - master.start;
        let occurrences = occurrence_starts(&master, window_start, window_end);
        for start in occurrences {
            if overrides.iter().any(|over| {
                over.uid == master.uid
                    && over
                        .recurrence_id
                        .is_some_and(|id| (id - start).num_seconds().abs() < 60)
            }) {
                continue;
            }
            let mut occurrence = master.clone();
            occurrence.start = start;
            occurrence.end = start + duration;
            occurrence.rrule = None;
            occurrence.rdates.clear();
            occurrence.exdates.clear();
            out.push(occurrence);
        }
    }
    for over in overrides {
        if over.start < window_end && over.end > window_start && !over.cancelled {
            out.push(over);
        }
    }
    out.sort_by_key(|event| (event.start, event.end, event.uid.clone()));
    out
}

fn occurrence_starts(
    master: &ParsedEvent,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Vec<DateTime<Utc>> {
    let mut starts = Vec::new();
    if let Some(rrule) = &master.rrule {
        let dtstart = master.start.format("%Y%m%dT%H%M%SZ").to_string();
        let set_text = format!("DTSTART:{dtstart}\nRRULE:{rrule}");
        if let Ok(set) = set_text.parse::<RRuleSet>() {
            let after = Tz::UTC
                .from_utc_datetime(&(window_start - chrono::Duration::seconds(1)).naive_utc());
            let before = Tz::UTC.from_utc_datetime(&window_end.naive_utc());
            let limited = set.after(after).before(before);
            for dt in limited.all(512).dates {
                starts.push(Utc.from_utc_datetime(&dt.naive_utc()));
            }
        }
    } else if master.start < window_end && master.end > window_start {
        starts.push(master.start);
    }
    for rdate in &master.rdates {
        if *rdate >= window_start && *rdate < window_end {
            starts.push(*rdate);
        }
    }
    starts.retain(|start| {
        !master
            .exdates
            .iter()
            .any(|ex| (*ex - *start).num_seconds().abs() < 60)
    });
    starts.sort();
    starts.dedup();
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master_weekly() -> ParsedEvent {
        ParsedEvent {
            uid: "series".into(),
            href_hint: None,
            summary: "Standup".into(),
            start: Utc.with_ymd_and_hms(2025, 12, 1, 9, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2025, 12, 1, 9, 30, 0).unwrap(),
            all_day: false,
            cancelled: false,
            transparent: false,
            description: None,
            location: None,
            join_url: None,
            organizer: None,
            attendees: Vec::new(),
            reminders_minutes: Vec::new(),
            rrule: Some("FREQ=WEEKLY;BYDAY=MO".into()),
            rdates: Vec::new(),
            exdates: vec![Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap()],
            recurrence_id: None,
            sequence: 0,
            status: None,
        }
    }

    #[test]
    fn expands_series_older_than_window_and_honours_exdate() {
        let window_start = Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
        let window_end = Utc.with_ymd_and_hms(2026, 1, 19, 0, 0, 0).unwrap();
        let expanded = expand_events(vec![master_weekly()], window_start, window_end);
        let starts: Vec<_> = expanded.iter().map(|e| e.start).collect();
        assert!(starts.contains(&Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap()));
        assert!(!starts.contains(&Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap()));
    }

    #[test]
    fn override_replaces_occurrence() {
        let mut master = master_weekly();
        master.rrule = Some("FREQ=WEEKLY;COUNT=3".into());
        master.start = Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap();
        master.end = Utc.with_ymd_and_hms(2026, 1, 5, 9, 30, 0).unwrap();
        let over = ParsedEvent {
            uid: "series".into(),
            summary: "Moved".into(),
            start: Utc.with_ymd_and_hms(2026, 1, 12, 10, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 1, 12, 10, 30, 0).unwrap(),
            recurrence_id: Some(Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap()),
            rrule: None,
            ..master.clone()
        };
        let window_start = Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
        let window_end = Utc.with_ymd_and_hms(2026, 1, 20, 0, 0, 0).unwrap();
        let expanded = expand_events(vec![master, over], window_start, window_end);
        assert!(expanded.iter().any(|e| e.summary == "Moved"));
        assert!(!expanded.iter().any(|e| {
            e.summary == "Standup" && e.start == Utc.with_ymd_and_hms(2026, 1, 12, 9, 0, 0).unwrap()
        }));
    }
}
