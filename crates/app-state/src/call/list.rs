use std::ops::Range;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use store::ChatRecord;
use teams_core::{CallDirection, CallLogEntry};

use super::history::{CallRow, call_rows};

pub const CALL_ROW_HEIGHT: f32 = 54.;
pub const EMPTY_HEIGHT: f32 = 56.;
const MAX_AGE_SECONDS: i64 = 60;
const NO_CALLS_TEXT: &str = "No calls yet";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryFilter {
    #[default]
    All,
    Missed,
    Incoming,
    Outgoing,
}

impl HistoryFilter {
    pub const ALL: [HistoryFilter; 4] = [
        HistoryFilter::All,
        HistoryFilter::Missed,
        HistoryFilter::Incoming,
        HistoryFilter::Outgoing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HistoryFilter::All => "All",
            HistoryFilter::Missed => "Missed",
            HistoryFilter::Incoming => "Incoming",
            HistoryFilter::Outgoing => "Outgoing",
        }
    }

    fn empty_text(self) -> &'static str {
        match self {
            HistoryFilter::All => NO_CALLS_TEXT,
            HistoryFilter::Missed => "No missed calls",
            HistoryFilter::Incoming => "No incoming calls",
            HistoryFilter::Outgoing => "No outgoing calls",
        }
    }

    fn admits(self, entry: &CallLogEntry) -> bool {
        match self {
            HistoryFilter::All => true,
            HistoryFilter::Missed => entry.missed(),
            HistoryFilter::Incoming => {
                entry.direction == CallDirection::Incoming && !entry.missed()
            }
            HistoryFilter::Outgoing => entry.direction == CallDirection::Outgoing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListEntry {
    Call(CallRow),
    Empty(&'static str),
}

impl ListEntry {
    pub fn height(&self) -> f32 {
        match self {
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
    filter: HistoryFilter,
}

impl CallsList {
    pub fn invalidate(&mut self) {
        self.invalidated = true;
    }

    pub fn filter(&self) -> HistoryFilter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: HistoryFilter) {
        if self.filter != filter {
            self.filter = filter;
            self.invalidated = true;
        }
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
        now: DateTime<Utc>,
        offset: FixedOffset,
    ) {
        let mut entries = Vec::new();
        let filter = self.filter;
        let admitted = history.iter().filter(|entry| filter.admits(entry));
        let calls = call_rows(admitted, chats, now, offset);
        if calls.is_empty() {
            entries.push(ListEntry::Empty(self.filter.empty_text()));
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

    fn built(calls: usize) -> CallsList {
        let history: Vec<CallLogEntry> =
            (0..calls).map(|index| call(&format!("c{index}"))).collect();
        let mut list = CallsList::default();
        list.rebuild(&history, &[], now(), zero());
        list
    }

    #[test]
    fn an_empty_history_shows_its_empty_text() {
        assert_eq!(built(0).entries(), [ListEntry::Empty("No calls yet")]);
    }

    #[test]
    fn the_history_filter_keeps_only_matching_calls() {
        let missed = CallLogEntry {
            direction: CallDirection::Incoming,
            outcome: CallOutcome::Missed,
            ..call("missed")
        };
        let incoming = CallLogEntry {
            direction: CallDirection::Incoming,
            ..call("incoming")
        };
        let history = [missed, incoming, call("outgoing")];
        let keys = |filter: HistoryFilter| -> Vec<String> {
            let mut list = CallsList::default();
            list.set_filter(filter);
            list.rebuild(&history, &[], now(), zero());
            list.entries()
                .iter()
                .filter_map(|entry| match entry {
                    ListEntry::Call(row) => Some(row.key.clone()),
                    ListEntry::Empty(text) => Some((*text).to_owned()),
                })
                .collect()
        };
        assert_eq!(keys(HistoryFilter::All), ["missed", "incoming", "outgoing"]);
        assert_eq!(keys(HistoryFilter::Missed), ["missed"]);
        assert_eq!(keys(HistoryFilter::Incoming), ["incoming"]);
        assert_eq!(keys(HistoryFilter::Outgoing), ["outgoing"]);
        let mut list = CallsList::default();
        list.set_filter(HistoryFilter::Missed);
        list.rebuild(&[call("outgoing")], &[], now(), zero());
        assert_eq!(
            list.entries().last(),
            Some(&ListEntry::Empty("No missed calls"))
        );
    }

    #[test]
    fn a_fresh_list_needs_a_rebuild_until_built() {
        let mut list = CallsList::default();
        assert!(list.needs_rebuild(now(), zero()));
        list.rebuild(&[], &[], now(), zero());
        assert!(!list.needs_rebuild(now() + Duration::seconds(30), zero()));
    }

    #[test]
    fn invalidation_age_and_a_new_day_force_a_rebuild() {
        let mut list = built(0);
        list.invalidate();
        assert!(list.needs_rebuild(now(), zero()));
        list.rebuild(&[], &[], now(), zero());
        assert!(list.needs_rebuild(now() + Duration::seconds(61), zero()));
        assert!(list.needs_rebuild(now() + Duration::hours(17), zero()));
    }

    #[test]
    fn the_window_keeps_only_rows_on_screen_and_spacers_for_the_rest() {
        let list = built(100);
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
        let list = built(100);
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
