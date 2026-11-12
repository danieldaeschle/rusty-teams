use crate::calendar::{CalendarEvent, CalendarSource};

/// Collapse duplicate meetings that share an iCal UID or the same Teams join URL.
pub fn merge_calendar_events(events: Vec<CalendarEvent>) -> Vec<CalendarEvent> {
    let mut merged: Vec<CalendarEvent> = Vec::new();
    for event in events {
        if let Some(existing) = merged.iter_mut().find(|candidate| same_meeting(candidate, &event))
        {
            if !existing.sources.contains(&event.source) {
                existing.sources.push(event.source);
            }
            if existing.join_url.is_none() {
                existing.join_url = event.join_url.clone();
                existing.online = existing.online || event.online;
            }
            if existing.thread_id.is_none() {
                existing.thread_id = event.thread_id.clone();
            }
            // Prefer Outlook object id when both sides contribute.
            if existing.source == CalendarSource::Google && event.source == CalendarSource::Outlook
            {
                existing.object_id = event.object_id.clone();
                existing.source = CalendarSource::Outlook;
                existing.account_id = event.account_id.clone();
                existing.calendar_id = event.calendar_id.clone();
                existing.response = event.response;
                existing.show_as = event.show_as;
            }
            continue;
        }
        merged.push(event);
    }
    merged.sort_by_key(|event| (event.start, event.end));
    merged
}

fn same_meeting(left: &CalendarEvent, right: &CalendarEvent) -> bool {
    if let (Some(a), Some(b)) = (&left.ical_uid, &right.ical_uid) {
        if !a.is_empty() && a == b {
            return true;
        }
    }
    match (
        left.join_url.as_deref().map(normalize_join_url),
        right.join_url.as_deref().map(normalize_join_url),
    ) {
        (Some(a), Some(b)) if !a.is_empty() && a == b => true,
        _ => false,
    }
}

pub fn normalize_join_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let without_query = trimmed.split('?').next().unwrap_or(trimmed);
    without_query.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn event(
        id: &str,
        source: CalendarSource,
        ical: Option<&str>,
        join: Option<&str>,
    ) -> CalendarEvent {
        CalendarEvent {
            object_id: id.into(),
            subject: "Sync".into(),
            start: chrono::Utc.with_ymd_and_hms(2026, 11, 11, 10, 0, 0).unwrap(),
            end: chrono::Utc.with_ymd_and_hms(2026, 11, 11, 10, 30, 0).unwrap(),
            ical_uid: ical.map(str::to_owned),
            join_url: join.map(str::to_owned),
            online: join.is_some(),
            source,
            sources: vec![source],
            ..Default::default()
        }
    }

    #[test]
    fn merges_on_ical_uid_and_keeps_both_sources() {
        let merged = merge_calendar_events(vec![
            event("o1", CalendarSource::Outlook, Some("uid"), None),
            event("g1", CalendarSource::Google, Some("uid"), Some("https://teams.microsoft.com/l/meetup-join/x")),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].sources.len(), 2);
        assert_eq!(merged[0].object_id, "o1");
        assert!(merged[0].join_url.is_some());
    }

    #[test]
    fn merges_on_normalized_join_url() {
        let merged = merge_calendar_events(vec![
            event(
                "o1",
                CalendarSource::Outlook,
                None,
                Some("https://teams.microsoft.com/l/meetup-join/X?x=1"),
            ),
            event(
                "g1",
                CalendarSource::Google,
                None,
                Some("https://teams.microsoft.com/l/meetup-join/x/"),
            ),
        ]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn does_not_merge_on_title_and_time_alone() {
        let a = event("a", CalendarSource::Outlook, Some("a"), None);
        let mut b = event("b", CalendarSource::Google, Some("b"), None);
        b.subject = a.subject.clone();
        assert_eq!(merge_calendar_events(vec![a, b]).len(), 2);
    }
}
