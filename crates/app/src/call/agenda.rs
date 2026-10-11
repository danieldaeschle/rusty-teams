use chatsvc::{CalendarEvent, ResponseType};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};

const JOIN_LEAD_MINUTES: i64 = 5;
const AGENDA_DAYS: i64 = 7;
const ALL_DAY_LABEL: &str = "All day";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaRow {
    pub key: String,
    pub time_label: String,
    pub subject: String,
    pub detail: String,
    pub all_day: bool,
    pub join_url: Option<String>,
    pub thread_id: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaDay {
    pub date: NaiveDate,
    pub label: String,
    pub rows: Vec<AgendaRow>,
}

pub fn fetch_window(now: DateTime<Utc>, offset: FixedOffset) -> (DateTime<Utc>, DateTime<Utc>) {
    let midnight = now
        .with_timezone(&offset)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|local| local.and_local_timezone(offset).single())
        .map_or(now, |local| local.with_timezone(&Utc));
    (midnight, midnight + Duration::days(AGENDA_DAYS))
}

pub fn join_available(
    row_start: DateTime<Utc>,
    row_end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    now >= row_start - Duration::minutes(JOIN_LEAD_MINUTES) && now < row_end
}

pub fn rsvp_label(response: ResponseType) -> Option<&'static str> {
    match response {
        ResponseType::Organizer | ResponseType::Accepted => None,
        ResponseType::Tentative => Some("tentative"),
        ResponseType::Declined => Some("declined"),
        ResponseType::NotResponded => Some("not answered"),
    }
}

pub fn day_heading(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".to_owned()
    } else if today.succ_opt() == Some(day) {
        "Tomorrow".to_owned()
    } else {
        day.format("%a, %b %-d").to_string()
    }
}

fn detail_label(event: &CalendarEvent) -> String {
    let rsvp = rsvp_label(event.response);
    match (event.organizer_name.as_deref(), rsvp) {
        (Some(organizer), Some(rsvp)) => format!("{organizer} \u{b7} {rsvp}"),
        (Some(organizer), None) => organizer.to_owned(),
        (None, Some(rsvp)) => rsvp.to_owned(),
        (None, None) => String::new(),
    }
}

fn time_label(event: &CalendarEvent, offset: FixedOffset) -> String {
    if event.all_day {
        return ALL_DAY_LABEL.to_owned();
    }
    format!(
        "{} - {}",
        event.start.with_timezone(&offset).format("%H:%M"),
        event.end.with_timezone(&offset).format("%H:%M")
    )
}

fn agenda_day(event: &CalendarEvent, today: NaiveDate, offset: FixedOffset) -> NaiveDate {
    let start_day = if event.all_day {
        event.start.date_naive()
    } else {
        event.start.with_timezone(&offset).date_naive()
    };
    start_day.max(today)
}

fn agenda_row(event: &CalendarEvent, offset: FixedOffset) -> AgendaRow {
    AgendaRow {
        key: event.object_id.clone(),
        time_label: time_label(event, offset),
        subject: if event.subject.is_empty() {
            "(No title)".to_owned()
        } else {
            event.subject.clone()
        },
        detail: detail_label(event),
        all_day: event.all_day,
        join_url: event.join_url.clone().filter(|_| event.online),
        thread_id: event.thread_id.clone(),
        start: event.start,
        end: event.end,
    }
}

pub fn group_by_day(
    events: &[CalendarEvent],
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> Vec<AgendaDay> {
    let today = now.with_timezone(&offset).date_naive();
    let last_day = today + Duration::days(AGENDA_DAYS - 1);
    let mut placed: Vec<(NaiveDate, &CalendarEvent)> = events
        .iter()
        .filter(|event| !event.cancelled && event.end > now)
        .map(|event| (agenda_day(event, today, offset), event))
        .filter(|(day, _)| *day <= last_day)
        .collect();
    placed.sort_by_key(|(day, event)| (*day, !event.all_day, event.start, event.end));
    let mut days: Vec<AgendaDay> = Vec::new();
    for (date, event) in placed {
        if days.last().is_none_or(|day| day.date != date) {
            days.push(AgendaDay {
                date,
                label: day_heading(date, today),
                rows: Vec::new(),
            });
        }
        if let Some(day) = days.last_mut() {
            day.rows.push(agenda_row(event, offset));
        }
    }
    days
}

pub fn demo_events(now: DateTime<Utc>) -> Vec<CalendarEvent> {
    let event = |id: &str,
                 subject: &str,
                 minutes_from_now: i64,
                 length_minutes: i64,
                 organizer: &str,
                 response: ResponseType| CalendarEvent {
        object_id: id.to_owned(),
        subject: subject.to_owned(),
        start: now + Duration::minutes(minutes_from_now),
        end: now + Duration::minutes(minutes_from_now + length_minutes),
        all_day: false,
        cancelled: false,
        online: true,
        join_url: Some(format!(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_{id}%40thread.v2/0"
        )),
        thread_id: None,
        organizer_name: Some(organizer.to_owned()),
        organizer_address: None,
        response,
    };
    vec![
        event(
            "demo-event-1",
            "Team standup",
            3,
            15,
            "Mara Lindqvist",
            ResponseType::Accepted,
        ),
        event(
            "demo-event-2",
            "Design review",
            240,
            60,
            "Jonas Ortega",
            ResponseType::NotResponded,
        ),
        event(
            "demo-event-3",
            "Planning",
            1700,
            45,
            "Priya Nair",
            ResponseType::Tentative,
        ),
        event(
            "demo-event-4",
            "Retro",
            3200,
            30,
            "Mara Lindqvist",
            ResponseType::Accepted,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap()
    }

    fn zero() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn event(id: &str, start: DateTime<Utc>, minutes: i64) -> CalendarEvent {
        CalendarEvent {
            object_id: id.into(),
            subject: id.into(),
            start,
            end: start + Duration::minutes(minutes),
            all_day: false,
            cancelled: false,
            online: true,
            join_url: Some("https://teams.microsoft.com/l/meetup-join/x".into()),
            thread_id: None,
            organizer_name: Some("Mara".into()),
            organizer_address: None,
            response: ResponseType::Accepted,
        }
    }

    #[test]
    fn the_join_window_runs_from_five_minutes_before_start_to_the_end() {
        let start = utc(12, 9, 0);
        let end = utc(12, 9, 30);
        assert!(!join_available(start, end, utc(12, 8, 54)));
        assert!(join_available(start, end, utc(12, 8, 55)));
        assert!(join_available(start, end, utc(12, 9, 29)));
        assert!(!join_available(start, end, utc(12, 9, 30)));
    }

    #[test]
    fn rsvp_labels_only_show_when_not_accepted() {
        assert_eq!(rsvp_label(ResponseType::Accepted), None);
        assert_eq!(rsvp_label(ResponseType::Organizer), None);
        assert_eq!(rsvp_label(ResponseType::NotResponded), Some("not answered"));
        assert_eq!(rsvp_label(ResponseType::Tentative), Some("tentative"));
        assert_eq!(rsvp_label(ResponseType::Declined), Some("declined"));
    }

    #[test]
    fn events_group_by_local_day_with_headings() {
        let now = utc(12, 8, 0);
        let events = [
            event("later", utc(14, 10, 0), 30),
            event("today", utc(12, 9, 0), 30),
            event("tomorrow", utc(13, 9, 0), 30),
        ];
        let days = group_by_day(&events, now, zero());
        let labels: Vec<&str> = days.iter().map(|day| day.label.as_str()).collect();
        assert_eq!(labels, ["Today", "Tomorrow", "Wed, Oct 14"]);
        assert_eq!(days[0].rows[0].time_label, "09:00 - 09:30");
    }

    #[test]
    fn the_offset_moves_late_events_to_the_next_day() {
        let now = utc(12, 8, 0);
        let plus_two = FixedOffset::east_opt(2 * 3600).unwrap();
        let days = group_by_day(&[event("late", utc(12, 23, 0), 30)], now, plus_two);
        assert_eq!(days[0].label, "Tomorrow");
        assert_eq!(days[0].rows[0].time_label, "01:00 - 01:30");
    }

    #[test]
    fn cancelled_ended_and_out_of_range_events_are_hidden() {
        let now = utc(12, 8, 0);
        let cancelled = CalendarEvent {
            cancelled: true,
            ..event("cancelled", utc(12, 9, 0), 30)
        };
        let events = [
            cancelled,
            event("ended", utc(12, 6, 0), 30),
            event("running", utc(12, 7, 45), 30),
            event("far", utc(19, 9, 0), 30),
            event("edge", utc(18, 9, 0), 30),
        ];
        let days = group_by_day(&events, now, zero());
        let keys: Vec<&str> = days
            .iter()
            .flat_map(|day| day.rows.iter().map(|row| row.key.as_str()))
            .collect();
        assert_eq!(keys, ["running", "edge"]);
    }

    #[test]
    fn all_day_events_lead_their_day_without_times() {
        let now = utc(12, 8, 0);
        let all_day = CalendarEvent {
            all_day: true,
            online: false,
            join_url: None,
            ..event("offsite", utc(12, 0, 0), 24 * 60)
        };
        let days = group_by_day(&[event("early", utc(12, 8, 30), 30), all_day], now, zero());
        assert_eq!(days[0].rows[0].key, "offsite");
        assert_eq!(days[0].rows[0].time_label, "All day");
        assert_eq!(days[0].rows[1].key, "early");
    }

    #[test]
    fn a_running_all_day_event_from_yesterday_shows_today() {
        let now = utc(12, 8, 0);
        let all_day = CalendarEvent {
            all_day: true,
            ..event("trip", utc(11, 0, 0), 48 * 60)
        };
        let days = group_by_day(&[all_day], now, zero());
        assert_eq!(days[0].label, "Today");
    }

    #[test]
    fn rows_carry_organizer_and_rsvp_and_only_online_join_urls() {
        let now = utc(12, 8, 0);
        let open = CalendarEvent {
            response: ResponseType::NotResponded,
            ..event("open", utc(12, 9, 0), 30)
        };
        let offline = CalendarEvent {
            online: false,
            ..event("offline", utc(12, 10, 0), 30)
        };
        let days = group_by_day(&[open, offline], now, zero());
        assert_eq!(days[0].rows[0].detail, "Mara \u{b7} not answered");
        assert!(days[0].rows[0].join_url.is_some());
        assert_eq!(days[0].rows[1].join_url, None);
    }

    #[test]
    fn the_fetch_window_is_seven_days_from_local_midnight() {
        let plus_two = FixedOffset::east_opt(2 * 3600).unwrap();
        let (start, end) = fetch_window(utc(12, 8, 0), plus_two);
        assert_eq!(start, utc(11, 22, 0));
        assert_eq!(end, utc(18, 22, 0));
    }
}
