//! Local calendar preferences: default view, last span, week layout, time scale.
//!
//! Work days and hours belong to the Outlook mailbox (Teams and Outlook read them
//! there); the copy here is only a cache so the grid has them before the mailbox
//! answers and while offline.

use chrono::{NaiveTime, Weekday};
use serde::{Deserialize, Serialize};
use store::Store;

use crate::week_grid::{CalendarSpan, WeekLayout};

pub const CALENDAR_PREFS_KEY: &str = "calendar.prefs";

/// Groups in the calendar list are keyed by this id for Outlook.
pub const OUTLOOK_GROUP: &str = "outlook";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultView {
    #[default]
    LastUsed,
    Day,
    WorkWeek,
    Week,
    Month,
}

impl DefaultView {
    pub const ALL: [DefaultView; 5] = [
        DefaultView::LastUsed,
        DefaultView::Day,
        DefaultView::WorkWeek,
        DefaultView::Week,
        DefaultView::Month,
    ];

    pub fn span(self) -> Option<CalendarSpan> {
        match self {
            DefaultView::LastUsed => None,
            DefaultView::Day => Some(CalendarSpan::Day),
            DefaultView::WorkWeek => Some(CalendarSpan::WorkWeek),
            DefaultView::Week => Some(CalendarSpan::Week),
            DefaultView::Month => Some(CalendarSpan::Month),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeScale {
    Min15,
    /// Teams default.
    #[default]
    Min30,
    Min60,
}

impl TimeScale {
    pub const ALL: [TimeScale; 3] = [TimeScale::Min15, TimeScale::Min30, TimeScale::Min60];

    pub fn minutes(self) -> u32 {
        match self {
            TimeScale::Min15 => 15,
            TimeScale::Min30 => 30,
            TimeScale::Min60 => 60,
        }
    }

    /// Minor grid lines inside one hour.
    pub fn steps_per_hour(self) -> u32 {
        60 / self.minutes()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekendWorkWeek {
    /// Outlook and Teams show the coming work week on a weekend.
    #[default]
    NextWeek,
    ThisWeek,
}

/// Work days and hours as the mailbox stores them, in the mailbox's local time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkHours {
    pub days: Vec<Weekday>,
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl Default for WorkHours {
    fn default() -> Self {
        WorkHours {
            days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ],
            start: NaiveTime::from_hms_opt(8, 0, 0).unwrap_or_default(),
            end: NaiveTime::from_hms_opt(17, 0, 0).unwrap_or_default(),
        }
    }
}

impl WorkHours {
    pub fn is_work_day(&self, day: Weekday) -> bool {
        self.days.contains(&day)
    }

    /// Minutes after midnight where work time starts and ends.
    pub fn minute_range(&self) -> (i64, i64) {
        let minute = |time: NaiveTime| {
            i64::from(chrono::Timelike::hour(&time)) * 60
                + i64::from(chrono::Timelike::minute(&time))
        };
        (minute(self.start), minute(self.end))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarPrefs {
    pub default_view: DefaultView,
    pub last_span: CalendarSpan,
    /// `None` follows the regional format.
    pub first_day: Option<Weekday>,
    pub time_scale: TimeScale,
    pub weekend_work_week: WeekendWorkWeek,
    pub pane_hidden: bool,
    pub collapsed_groups: Vec<String>,
    pub work_hours: WorkHours,
    /// Mailbox language (`en-CH`), cached; Outlook takes the default week start from it.
    pub mailbox_locale: Option<String>,
}

impl CalendarPrefs {
    /// The span the calendar opens in.
    pub fn initial_span(&self) -> CalendarSpan {
        self.default_view.span().unwrap_or(self.last_span)
    }

    pub fn first_day_or(&self, regional: Weekday) -> Weekday {
        self.first_day.unwrap_or(regional)
    }

    /// Week layout for the grid; `regional` is the regional format's first day.
    pub fn layout(&self, regional: Weekday) -> WeekLayout {
        WeekLayout {
            first_day: self.first_day_or(regional),
            work_days: self.work_hours.days.clone(),
            weekend: self.weekend_work_week,
        }
    }

    pub fn group_collapsed(&self, group: &str) -> bool {
        self.collapsed_groups.iter().any(|id| id == group)
    }

    pub fn toggle_group(&mut self, group: &str) {
        if self.group_collapsed(group) {
            self.collapsed_groups.retain(|id| id != group);
        } else {
            self.collapsed_groups.push(group.to_owned());
        }
    }

    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

pub fn load_calendar_prefs(store: &Store) -> CalendarPrefs {
    store
        .meta(CALENDAR_PREFS_KEY)
        .ok()
        .flatten()
        .map(|json| CalendarPrefs::from_json(&json))
        .unwrap_or_default()
}

pub fn save_calendar_prefs(store: &Store, prefs: &CalendarPrefs) {
    if let Err(error) = store.set_meta(CALENDAR_PREFS_KEY, &prefs.to_json()) {
        log::warn!("calendar prefs not saved: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_todays_behaviour() {
        let prefs = CalendarPrefs::default();
        assert_eq!(prefs.default_view, DefaultView::LastUsed);
        assert_eq!(prefs.initial_span(), CalendarSpan::WorkWeek);
        assert_eq!(prefs.time_scale.minutes(), 30);
        assert_eq!(prefs.weekend_work_week, WeekendWorkWeek::NextWeek);
        assert_eq!(prefs.work_hours.minute_range(), (8 * 60, 17 * 60));
        assert!(!prefs.work_hours.is_work_day(Weekday::Sat));
    }

    #[test]
    fn a_fixed_default_view_wins_over_the_last_span() {
        let prefs = CalendarPrefs {
            default_view: DefaultView::Week,
            last_span: CalendarSpan::Month,
            ..CalendarPrefs::default()
        };
        assert_eq!(prefs.initial_span(), CalendarSpan::Week);
        let last_used = CalendarPrefs {
            default_view: DefaultView::LastUsed,
            ..prefs
        };
        assert_eq!(last_used.initial_span(), CalendarSpan::Month);
    }

    #[test]
    fn prefs_round_trip_through_the_store() {
        let store = Store::open_in_memory().unwrap();
        let mut prefs = CalendarPrefs {
            default_view: DefaultView::Month,
            first_day: Some(Weekday::Sun),
            time_scale: TimeScale::Min15,
            pane_hidden: true,
            ..CalendarPrefs::default()
        };
        prefs.toggle_group("g1");
        prefs.work_hours.days = vec![Weekday::Mon, Weekday::Tue, Weekday::Thu];
        save_calendar_prefs(&store, &prefs);
        assert_eq!(load_calendar_prefs(&store), prefs);
    }

    #[test]
    fn missing_fields_take_defaults_and_broken_json_resets() {
        let partial = CalendarPrefs::from_json(r#"{"default_view":"month"}"#);
        assert_eq!(partial.default_view, DefaultView::Month);
        assert_eq!(partial.time_scale, TimeScale::Min30);
        assert_eq!(
            CalendarPrefs::from_json("{not json"),
            CalendarPrefs::default()
        );
        assert_eq!(
            CalendarPrefs::from_json(r#"{"time_scale":"min7"}"#),
            CalendarPrefs::default()
        );
    }

    #[test]
    fn a_group_collapses_and_expands() {
        let mut prefs = CalendarPrefs::default();
        prefs.toggle_group(OUTLOOK_GROUP);
        assert!(prefs.group_collapsed(OUTLOOK_GROUP));
        prefs.toggle_group(OUTLOOK_GROUP);
        assert!(!prefs.group_collapsed(OUTLOOK_GROUP));
    }
}
