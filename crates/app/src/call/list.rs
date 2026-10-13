use std::ops::Range;

use chatsvc::CalendarEvent;
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use store::ChatRecord;
use teams_core::CallLogEntry;

use super::agenda::{AgendaRow, group_by_day};
use super::history::{CallRow, call_rows};

pub const SECTION_HEIGHT: f32 = 34.;
pub const DAY_HEIGHT: f32 = 26.;
pub const AGENDA_ROW_HEIGHT: f32 = 58.;
pub const CALL_ROW_HEIGHT: f32 = 54.;
pub const EMPTY_HEIGHT: f32 = 56.;
const MAX_AGE_SECONDS: i64 = 60;
const UPCOMING_TITLE: &str = "Upcoming";
const RECENT_TITLE: &str = "Recent";
const NO_MEETINGS_TEXT: &str = "No meetings in the next 7 days";
const NO_CALLS_TEXT: &str = "No calls yet";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListEntry {
    Section(&'static str),
    Day(String),
    Agenda(AgendaRow),
    Call(CallRow),
    Empty(&'static str),
}

impl ListEntry {
    pub fn height(&self) -> f32 {
        match self {
            ListEntry::Section(_) => SECTION_HEIGHT,
            ListEntry::Day(_) => DAY_HEIGHT,
            ListEntry::Agenda(_) => AGENDA_ROW_HEIGHT,
            ListEntry::Call(_) => CALL_ROW_HEIGHT,
            ListEntry::Empty(_) => EMPTY_HEIGHT,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisibleWindow {
    pub range: Range<usize>,
    pub before: f32,
    pub after: f32,
}

#[derive(Debug, Clone)]
struct BuiltAt {
    day: NaiveDate,
    time: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct CallsList {
    entries: Vec<ListEntry>,
    starts: Vec<f32>,
    ends: Vec<f32>,
    built_at: Option<BuiltAt>,
    invalidated: bool,
}

impl CallsList {
    pub fn invalidate(&mut self) {
        self.invalidated = true;
    }

    pub fn needs_rebuild(&self, now: DateTime<Utc>, offset: FixedOffset) -> bool {
        let Some(built_at) = &self.built_at else {
            return true;
        };
        self.invalidated
            || built_at.day != now.with_timezone(&offset).date_naive()
            || now - built_at.time >= Duration::seconds(MAX_AGE_SECONDS)
    }

    pub fn entries(&self) -> &[ListEntry] {
        &self.entries
    }

    pub fn rebuild(
        &mut self,
        history: &[CallLogEntry],
        chats: &[ChatRecord],
        events: &[CalendarEvent],
        now: DateTime<Utc>,
        offset: FixedOffset,
    ) {
        let mut entries = vec![ListEntry::Section(UPCOMING_TITLE)];
        let days = group_by_day(events, now, offset);
        if days.is_empty() {
            entries.push(ListEntry::Empty(NO_MEETINGS_TEXT));
        }
        for day in days {
            entries.push(ListEntry::Day(day.label));
            entries.extend(day.rows.into_iter().map(ListEntry::Agenda));
        }
        entries.push(ListEntry::Section(RECENT_TITLE));
        let calls = call_rows(history, chats, now, offset);
        if calls.is_empty() {
            entries.push(ListEntry::Empty(NO_CALLS_TEXT));
        }
        entries.extend(calls.into_iter().map(ListEntry::Call));
        self.set_entries(entries);
        self.built_at = Some(BuiltAt {
            day: now.with_timezone(&offset).date_naive(),
            time: now,
        });
        self.invalidated = false;
    }

    fn set_entries(&mut self, entries: Vec<ListEntry>) {
        let mut position = 0.;
        self.starts = Vec::with_capacity(entries.len());
        self.ends = Vec::with_capacity(entries.len());
        for entry in &entries {
            self.starts.push(position);
            position += entry.height();
            self.ends.push(position);
        }
        self.entries = entries;
    }

    pub fn total_height(&self) -> f32 {
        self.ends.last().copied().unwrap_or(0.)
    }

    pub fn window(&self, from: f32, to: f32) -> VisibleWindow {
        let first = self.ends.partition_point(|end| *end < from);
        let last = self.starts.partition_point(|start| *start <= to).max(first);
        VisibleWindow {
            before: self
                .starts
                .get(first)
                .copied()
                .unwrap_or(self.total_height()),
            after: match last.checked_sub(1).and_then(|index| self.ends.get(index)) {
                Some(end) if last > first => self.total_height() - end,
                _ => 0.,
            },
            range: first..last,
        }
    }
}

#[cfg(test)]
mod tests {
    use chatsvc::ResponseType;
    use chrono::TimeZone;
    use teams_core::{CallDirection, CallOutcome};

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, 8, 0, 0).unwrap()
    }

    fn zero() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn call(id: &str) -> CallLogEntry {
        CallLogEntry {
            call_id: id.into(),
            started_at: now() - Duration::hours(3),
            duration_seconds: Some(60),
            direction: CallDirection::Outgoing,
            outcome: CallOutcome::Accepted,
            is_meeting: false,
            peer_id: Some("8:orgid:bea".into()),
            peer_name: Some("Bea".into()),
            thread_id: None,
        }
    }

    fn event(id: &str, minutes_from_now: i64) -> CalendarEvent {
        CalendarEvent {
            object_id: id.into(),
            subject: id.into(),
            start: now() + Duration::minutes(minutes_from_now),
            end: now() + Duration::minutes(minutes_from_now + 30),
            all_day: false,
            cancelled: false,
            online: true,
            join_url: None,
            thread_id: None,
            organizer_name: None,
            organizer_address: None,
            ical_uid: None,
            response: ResponseType::Accepted,
        }
    }

    fn built(calls: usize, events: usize) -> CallsList {
        let history: Vec<CallLogEntry> =
            (0..calls).map(|index| call(&format!("c{index}"))).collect();
        let agenda: Vec<CalendarEvent> = (0..events)
            .map(|index| event(&format!("e{index}"), 30 + index as i64 * 60))
            .collect();
        let mut list = CallsList::default();
        list.rebuild(&history, &[], &agenda, now(), zero());
        list
    }

    #[test]
    fn empty_sections_show_their_empty_text() {
        let list = built(0, 0);
        assert_eq!(
            list.entries(),
            [
                ListEntry::Section("Upcoming"),
                ListEntry::Empty("No meetings in the next 7 days"),
                ListEntry::Section("Recent"),
                ListEntry::Empty("No calls yet"),
            ]
        );
    }

    #[test]
    fn upcoming_comes_before_recent_with_day_headers() {
        let list = built(2, 2);
        let kinds: Vec<&str> = list
            .entries()
            .iter()
            .map(|entry| match entry {
                ListEntry::Section(_) => "section",
                ListEntry::Day(_) => "day",
                ListEntry::Agenda(_) => "agenda",
                ListEntry::Call(_) => "call",
                ListEntry::Empty(_) => "empty",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "section", "day", "agenda", "agenda", "section", "call", "call"
            ]
        );
    }

    #[test]
    fn a_fresh_list_needs_a_rebuild_until_built() {
        let mut list = CallsList::default();
        assert!(list.needs_rebuild(now(), zero()));
        list.rebuild(&[], &[], &[], now(), zero());
        assert!(!list.needs_rebuild(now() + Duration::seconds(30), zero()));
    }

    #[test]
    fn invalidation_age_and_a_new_day_force_a_rebuild() {
        let mut list = built(0, 0);
        list.invalidate();
        assert!(list.needs_rebuild(now(), zero()));
        list.rebuild(&[], &[], &[], now(), zero());
        assert!(list.needs_rebuild(now() + Duration::seconds(61), zero()));
        assert!(list.needs_rebuild(now() + Duration::hours(17), zero()));
    }

    #[test]
    fn the_window_keeps_only_rows_on_screen_and_spacers_for_the_rest() {
        let list = built(100, 0);
        let total = list.total_height();
        let window = list.window(1000., 1200.);
        assert!(window.range.len() < 12);
        assert!(window.range.start > 10);
        assert_eq!(window.before, list.starts[window.range.start]);
        let shown: f32 = list.entries()[window.range.clone()]
            .iter()
            .map(ListEntry::height)
            .sum();
        assert!((window.before + shown + window.after - total).abs() < 0.01);
    }

    #[test]
    fn the_window_clamps_at_both_ends() {
        let list = built(100, 0);
        let top = list.window(-300., 100.);
        assert_eq!(top.range.start, 0);
        assert_eq!(top.before, 0.);
        let bottom = list.window(list.total_height() - 100., list.total_height() + 500.);
        assert_eq!(bottom.range.end, list.entries().len());
        assert_eq!(bottom.after, 0.);
        let empty = CallsList::default().window(0., 100.);
        assert_eq!(empty.range, 0..0);
        assert_eq!(empty.before, 0.);
        assert_eq!(empty.after, 0.);
    }
}
