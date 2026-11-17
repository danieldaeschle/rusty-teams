use crate::calendar::{CalendarEvent, CalendarSource};

/// Identity of the home calendar used when Outlook is not among merge contributors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeCalendar<'a> {
    pub source: CalendarSource,
    pub account_id: &'a str,
    pub calendar_id: &'a str,
}

/// Collapse duplicate meetings that share an iCal UID or the same Teams join URL.
///
/// Preference when both sides contribute: Outlook wins; else the home calendar's copy; else keep
/// the first.
pub fn merge_calendar_events(
    events: Vec<CalendarEvent>,
    home: Option<HomeCalendar<'_>>,
) -> Vec<CalendarEvent> {
    let mut merged: Vec<CalendarEvent> = Vec::new();
    for event in events {
        if let Some(existing) = merged
            .iter_mut()
            .find(|candidate| same_meeting(candidate, &event))
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
            if existing.location.is_none() {
                existing.location = event.location.clone();
            }
            if existing.body_preview.is_none() {
                existing.body_preview = event.body_preview.clone();
            }
            if existing.body_html.is_none() {
                existing.body_html = event.body_html.clone();
            }
            if existing.response_counts.is_none() {
                existing.response_counts = event.response_counts;
            }
            if existing.attendees.is_empty() {
                existing.attendees = event.attendees.clone();
            }
            if existing.reminders.is_empty() {
                existing.reminders = event.reminders.clone();
            }
            if existing.source_link.is_none() {
                existing.source_link = event.source_link.clone();
            }
            if existing.web_link.is_none() {
                existing.web_link = event.web_link.clone();
            }
            if existing.calendar_name.is_none() {
                existing.calendar_name = event.calendar_name.clone();
            }
            if event.series {
                existing.series = true;
            }
            if existing.series_master_id.is_none() {
                existing.series_master_id = event.series_master_id.clone();
            }
            if prefers_incoming(existing, &event, home) {
                take_identity(existing, &event);
            }
            continue;
        }
        merged.push(event);
    }
    merged.sort_by_key(|event| (event.start, event.end));
    merged
}

fn prefers_incoming(
    existing: &CalendarEvent,
    incoming: &CalendarEvent,
    home: Option<HomeCalendar<'_>>,
) -> bool {
    if incoming.source.is_outlook() && !existing.source.is_outlook() {
        return true;
    }
    if existing.source.is_outlook() {
        return false;
    }
    if let Some(home) = home {
        let incoming_home = matches_home(incoming, home);
        let existing_home = matches_home(existing, home);
        if incoming_home && !existing_home {
            return true;
        }
    }
    false
}

fn matches_home(event: &CalendarEvent, home: HomeCalendar<'_>) -> bool {
    if event.source != home.source {
        return false;
    }
    if home.source.is_outlook() {
        return true;
    }
    event.account_id.as_deref() == Some(home.account_id)
        && event.calendar_id.as_deref() == Some(home.calendar_id)
}

fn take_identity(existing: &mut CalendarEvent, incoming: &CalendarEvent) {
    existing.object_id = incoming.object_id.clone();
    existing.source = incoming.source;
    existing.account_id = incoming.account_id.clone();
    existing.calendar_id = incoming.calendar_id.clone();
    existing.response = incoming.response;
    existing.show_as = incoming.show_as;
}

fn same_meeting(left: &CalendarEvent, right: &CalendarEvent) -> bool {
    let same_all_day = left.all_day
        && right.all_day
        && (left.start - right.start).abs() < chrono::Duration::days(1);
    if left.start != right.start && !same_all_day {
        return false;
    }
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
            start: chrono::Utc
                .with_ymd_and_hms(2026, 11, 11, 10, 0, 0)
                .unwrap(),
            end: chrono::Utc
                .with_ymd_and_hms(2026, 11, 11, 10, 30, 0)
                .unwrap(),
            ical_uid: ical.map(str::to_owned),
            join_url: join.map(str::to_owned),
            online: join.is_some(),
            source,
            sources: vec![source],
            account_id: match source {
                CalendarSource::Google => Some("g1".into()),
                CalendarSource::CalDav => Some("c1".into()),
                _ => None,
            },
            calendar_id: match source {
                CalendarSource::Google => Some("primary".into()),
                CalendarSource::CalDav => Some("/cal/".into()),
                _ => None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn merges_on_ical_uid_and_prefers_outlook() {
        let merged = merge_calendar_events(
            vec![
                event("o1", CalendarSource::Outlook, Some("uid"), None),
                event(
                    "g1",
                    CalendarSource::Google,
                    Some("uid"),
                    Some("https://teams.microsoft.com/l/meetup-join/x"),
                ),
            ],
            None,
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].sources.len(), 2);
        assert_eq!(merged[0].object_id, "o1");
        assert!(merged[0].join_url.is_some());
    }

    #[test]
    fn prefers_home_when_outlook_absent() {
        let home = HomeCalendar {
            source: CalendarSource::CalDav,
            account_id: "c1",
            calendar_id: "/cal/",
        };
        let google_first = event("g1", CalendarSource::Google, Some("uid"), None);
        let caldav = event("c1", CalendarSource::CalDav, Some("uid"), None);
        let merged = merge_calendar_events(vec![google_first, caldav], Some(home));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, CalendarSource::CalDav);
        assert_eq!(merged[0].object_id, "c1");
    }

    #[test]
    fn occurrences_of_one_series_stay_separate() {
        let join = Some("https://teams.microsoft.com/l/meetup-join/x");
        let mut next_day = event("o2", CalendarSource::Outlook, Some("uid"), join);
        next_day.start += chrono::Duration::days(1);
        next_day.end += chrono::Duration::days(1);
        let merged = merge_calendar_events(
            vec![
                event("o1", CalendarSource::Outlook, Some("uid"), join),
                next_day,
            ],
            None,
        );
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merges_on_normalized_join_url() {
        let merged = merge_calendar_events(
            vec![
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
            ],
            None,
        );
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn does_not_merge_on_title_and_time_alone() {
        let a = event("a", CalendarSource::Outlook, Some("a"), None);
        let mut b = event("b", CalendarSource::Google, Some("b"), None);
        b.subject = a.subject.clone();
        assert_eq!(merge_calendar_events(vec![a, b], None).len(), 2);
    }
}
