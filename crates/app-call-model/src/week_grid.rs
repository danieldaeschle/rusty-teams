use chatsvc::{CalendarEvent, ResponseType, ShowAs};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Months, NaiveDate, NaiveTime, Utc, Weekday,
};

pub const WORK_WEEK_DAYS: i64 = 5;
pub const MINUTES_PER_DAY: i64 = 24 * 60;
pub const MONTH_GRID_DAYS: i64 = 42;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekBlock {
    pub event_index: usize,
    pub start_minute: i64,
    pub end_minute: i64,
    pub lane: usize,
    pub lanes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekDay {
    pub date: NaiveDate,
    pub all_day: Vec<usize>,
    pub blocks: Vec<WeekBlock>,
}

pub fn week_start(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarSpan {
    Day,
    #[default]
    WorkWeek,
    Week,
    Month,
}

impl CalendarSpan {
    pub const ALL: [CalendarSpan; 4] = [
        CalendarSpan::Day,
        CalendarSpan::WorkWeek,
        CalendarSpan::Week,
        CalendarSpan::Month,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CalendarSpan::Day => "Day",
            CalendarSpan::WorkWeek => "Work week",
            CalendarSpan::Week => "Week",
            CalendarSpan::Month => "Month",
        }
    }

    pub fn days(self) -> i64 {
        match self {
            CalendarSpan::Day => 1,
            CalendarSpan::WorkWeek => WORK_WEEK_DAYS,
            CalendarSpan::Week => 7,
            CalendarSpan::Month => MONTH_GRID_DAYS,
        }
    }

    pub fn start_for(self, date: NaiveDate) -> NaiveDate {
        match self {
            CalendarSpan::Day => date,
            CalendarSpan::WorkWeek => work_week_for(date),
            CalendarSpan::Week => week_start(date),
            CalendarSpan::Month => month_grid_start(date),
        }
    }

    pub fn shifted(self, start: NaiveDate, steps: i32) -> NaiveDate {
        match self {
            CalendarSpan::Day => start + Duration::days(i64::from(steps)),
            CalendarSpan::WorkWeek | CalendarSpan::Week => {
                start + Duration::weeks(i64::from(steps))
            }
            CalendarSpan::Month => {
                let month = month_shown(start);
                let moved = if steps >= 0 {
                    month.checked_add_months(Months::new(steps.unsigned_abs()))
                } else {
                    month.checked_sub_months(Months::new(steps.unsigned_abs()))
                };
                month_grid_start(moved.unwrap_or(month))
            }
        }
    }

    pub fn title(self, start: NaiveDate) -> String {
        match self {
            CalendarSpan::Month => month_shown(start).format("%B %Y").to_string(),
            _ => range_label(start, self.days()),
        }
    }
}

pub fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

pub fn month_grid_start(date: NaiveDate) -> NaiveDate {
    week_start(first_of_month(date))
}

/// The grid starts up to six days before the 1st, so a week in is always inside the shown month.
pub fn month_shown(grid_start: NaiveDate) -> NaiveDate {
    first_of_month(grid_start + Duration::weeks(1))
}

/// On a weekend the work week view shows the coming week, as Outlook does.
pub fn work_week_for(today: NaiveDate) -> NaiveDate {
    match today.weekday() {
        Weekday::Sat | Weekday::Sun => week_start(today) + Duration::weeks(1),
        _ => week_start(today),
    }
}

pub fn local_midnight(date: NaiveDate, offset: FixedOffset) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0)
        .and_then(|local| local.and_local_timezone(offset).single())
        .map(|local| local.with_timezone(&Utc))
        .unwrap_or_else(|| date.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc())
}

pub fn week_range(
    start: NaiveDate,
    days: i64,
    offset: FixedOffset,
) -> (DateTime<Utc>, DateTime<Utc>) {
    (
        local_midnight(start, offset),
        local_midnight(start + Duration::days(days), offset),
    )
}

pub fn shown_in_calendar(event: &CalendarEvent) -> bool {
    event.response != ResponseType::Declined
}

/// True all-day events, plus timed blocks that cover whole local days
/// (Google holidays/week markers as 00:00→00:00, or multi-day 00:00→23:59).
pub fn is_effective_all_day(event: &CalendarEvent, offset: FixedOffset) -> bool {
    if event.all_day {
        return true;
    }
    let start = event.start.with_timezone(&offset);
    let end = event.end.with_timezone(&offset);
    if start.time() != NaiveTime::MIN || end <= start {
        return false;
    }
    if end.time() == NaiveTime::MIN {
        return true;
    }
    // Multi-day blocks that run from midnight through end-of-day (e.g. 23:59).
    end.date_naive() > start.date_naive()
        && end.time() >= NaiveTime::from_hms_opt(23, 59, 0).unwrap()
}

pub fn week_days(
    events: &[CalendarEvent],
    start: NaiveDate,
    days: i64,
    offset: FixedOffset,
) -> Vec<WeekDay> {
    (0..days)
        .map(|day| {
            let date = start + Duration::days(day);
            let (day_start, day_end) = week_range(date, 1, offset);
            let mut all_day = Vec::new();
            let mut spans = Vec::new();
            for (index, event) in events.iter().enumerate() {
                if !shown_in_calendar(event) || event.end <= day_start || event.start >= day_end {
                    continue;
                }
                if is_effective_all_day(event, offset) {
                    all_day.push(index);
                    continue;
                }
                let start_minute = (event.start.max(day_start) - day_start).num_minutes();
                let end_minute = (event.end.min(day_end) - day_start).num_minutes();
                spans.push((index, start_minute, end_minute.max(start_minute + 1)));
            }
            WeekDay {
                date,
                all_day,
                blocks: lay_out(spans),
            }
        })
        .collect()
}

fn lay_out(mut spans: Vec<(usize, i64, i64)>) -> Vec<WeekBlock> {
    spans.sort_by_key(|&(index, start, end)| (start, std::cmp::Reverse(end), index));
    let mut blocks: Vec<WeekBlock> = Vec::with_capacity(spans.len());
    let mut cluster_start = 0;
    let mut cluster_end = i64::MIN;
    let mut lane_ends: Vec<i64> = Vec::new();
    for (index, start, end) in spans {
        if start >= cluster_end {
            close_cluster(&mut blocks[cluster_start..], lane_ends.len());
            cluster_start = blocks.len();
            lane_ends.clear();
        }
        let lane = match lane_ends.iter().position(|&lane_end| lane_end <= start) {
            Some(free) => {
                lane_ends[free] = end;
                free
            }
            None => {
                lane_ends.push(end);
                lane_ends.len() - 1
            }
        };
        cluster_end = cluster_end.max(end);
        blocks.push(WeekBlock {
            event_index: index,
            start_minute: start,
            end_minute: end,
            lane,
            lanes: 1,
        });
    }
    close_cluster(&mut blocks[cluster_start..], lane_ends.len());
    blocks
}

fn close_cluster(blocks: &mut [WeekBlock], lanes: usize) {
    for block in blocks {
        block.lanes = lanes.max(1);
    }
}

pub fn range_label(start: NaiveDate, days: i64) -> String {
    app_base::format::calendar_range_label(start, start + Duration::days(days - 1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLook {
    Busy,
    Free,
    Tentative,
}

pub fn event_look(event: &CalendarEvent) -> EventLook {
    match event.show_as {
        ShowAs::Free => EventLook::Free,
        ShowAs::Tentative => EventLook::Tentative,
        ShowAs::Busy | ShowAs::OutOfOffice | ShowAs::WorkingElsewhere => EventLook::Busy,
    }
}

pub fn hour_label(hour: u32, twelve_hour: bool) -> String {
    if !twelve_hour {
        return hour.to_string();
    }
    match hour {
        0 => "12 AM".to_owned(),
        12 => "12 PM".to_owned(),
        hour if hour < 12 => format!("{hour} AM"),
        hour => format!("{} PM", hour - 12),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn event(start_hour: u32, start_minute: u32, minutes: i64) -> CalendarEvent {
        let start = Utc
            .with_ymd_and_hms(2026, 11, 9, start_hour, start_minute, 0)
            .unwrap();
        CalendarEvent {
            object_id: format!("{start_hour}:{start_minute}"),
            subject: "Sync".into(),
            start,
            end: start + Duration::minutes(minutes),
            all_day: false,
            cancelled: false,
            online: true,
            join_url: None,
            thread_id: None,
            organizer_name: None,
            organizer_address: None,
            ical_uid: None,
            show_as: Default::default(),
            dial_in: None,
            response: ResponseType::Accepted,
            ..Default::default()
        }
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    #[test]
    fn month_view_steps_whole_months_from_a_monday_grid() {
        let date = |day| NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
        let start = CalendarSpan::Month.start_for(date(18));
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 10, 26).unwrap());
        assert_eq!(CalendarSpan::Month.title(start), "November 2026");
        let next = CalendarSpan::Month.shifted(start, 1);
        assert_eq!(next, NaiveDate::from_ymd_opt(2026, 11, 30).unwrap());
        assert_eq!(CalendarSpan::Month.title(next), "December 2026");
        let back = CalendarSpan::Month.shifted(start, -11);
        assert_eq!(CalendarSpan::Month.title(back), "December 2025");
        assert_eq!(
            CalendarSpan::Week.shifted(date(2), -1),
            date(2) - Duration::weeks(1)
        );
    }

    #[test]
    fn the_week_starts_on_monday() {
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        assert_eq!(
            week_start(sunday),
            NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()
        );
        assert_eq!(week_start(week_start(sunday)), week_start(sunday));
        assert_eq!(
            work_week_for(sunday),
            NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
        );
        let friday = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        assert_eq!(
            work_week_for(friday),
            NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()
        );
    }

    #[test]
    fn overlapping_meetings_share_the_column_in_lanes() {
        let events = vec![
            event(9, 0, 60),
            event(9, 30, 60),
            event(10, 0, 30),
            event(13, 0, 30),
        ];
        let monday = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let days = week_days(&events, monday, WORK_WEEK_DAYS, utc());
        assert_eq!(days.len(), 5);
        let lanes: Vec<(usize, usize, usize)> = days[0]
            .blocks
            .iter()
            .map(|block| (block.event_index, block.lane, block.lanes))
            .collect();
        assert_eq!(lanes, [(0, 0, 2), (1, 1, 2), (2, 0, 2), (3, 0, 1)]);
        assert_eq!(days[0].blocks[0].start_minute, 9 * 60);
        assert!(days[1].blocks.is_empty());
    }

    #[test]
    fn declined_and_other_days_are_left_out_and_all_day_is_separate() {
        let mut declined = event(9, 0, 30);
        declined.response = ResponseType::Declined;
        let mut all_day = event(0, 0, 24 * 60);
        all_day.all_day = true;
        let late = event(23, 30, 60);
        let monday = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let days = week_days(&[declined, all_day, late], monday, 2, utc());
        assert!(days[0].blocks.iter().all(|block| block.event_index != 0));
        assert_eq!(days[0].all_day, [1]);
        assert_eq!(days[0].blocks[0].end_minute, MINUTES_PER_DAY);
        assert_eq!(
            (days[1].blocks[0].start_minute, days[1].blocks[0].end_minute),
            (0, 30)
        );
    }

    #[test]
    fn midnight_to_midnight_timed_events_sit_in_the_all_day_row() {
        // Google holidays / week markers often arrive as dateTime 00:00→00:00.
        let holiday = event(0, 0, 24 * 60);
        let meeting = event(9, 30, 60);
        let monday = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert!(is_effective_all_day(&holiday, utc()));
        assert!(!is_effective_all_day(&meeting, utc()));
        let days = week_days(&[holiday, meeting], monday, 1, utc());
        assert_eq!(days[0].all_day, [0]);
        assert_eq!(days[0].blocks.len(), 1);
        assert_eq!(days[0].blocks[0].event_index, 1);
    }

    #[test]
    fn multi_day_midnight_to_end_of_day_is_all_day() {
        // e.g. KubeCon as 00:00 Mon → 23:59 Thu.
        let conference = event(0, 0, 4 * 24 * 60 - 1);
        assert!(is_effective_all_day(&conference, utc()));
        let monday = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let days = week_days(&[conference], monday, 5, utc());
        assert!(days.iter().all(|day| day.blocks.is_empty()));
        assert_eq!(days[0].all_day, [0]);
        assert_eq!(days[3].all_day, [0]);
    }

    #[test]
    fn spans_start_and_step_like_teams() {
        let wednesday = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert_eq!(CalendarSpan::Day.start_for(wednesday), wednesday);
        assert_eq!(CalendarSpan::WorkWeek.start_for(wednesday), monday);
        assert_eq!(CalendarSpan::Week.start_for(wednesday), monday);
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        assert_eq!(CalendarSpan::Week.start_for(sunday), monday);
        assert_eq!(
            CalendarSpan::Day.shifted(wednesday, 1),
            wednesday + Duration::days(1)
        );
        assert_eq!(CalendarSpan::Week.days(), 7);
    }

    #[test]
    fn labels_read_like_teams() {
        let monday = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        assert_eq!(range_label(monday, 5), "November 30 - December 4, 2026");
        assert_eq!(
            range_label(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), 5),
            "November 9-13, 2026"
        );
        assert_eq!(
            range_label(NaiveDate::from_ymd_opt(2026, 12, 28).unwrap(), 7),
            "December 28, 2026 - January 3, 2027"
        );
        assert_eq!(
            range_label(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), 1),
            "November 9, 2026"
        );
        assert_eq!(hour_label(0, true), "12 AM");
        assert_eq!(hour_label(13, true), "1 PM");
        assert_eq!(hour_label(13, false), "13");
    }

    #[test]
    fn free_and_tentative_meetings_look_like_teams() {
        let mut following = event(9, 0, 30);
        following.show_as = ShowAs::Free;
        following.response = ResponseType::Tentative;
        let mut tentative = event(9, 0, 30);
        tentative.show_as = ShowAs::Tentative;
        let mut away = event(9, 0, 30);
        away.show_as = ShowAs::OutOfOffice;
        assert_eq!(event_look(&following), EventLook::Free);
        assert_eq!(event_look(&tentative), EventLook::Tentative);
        assert_eq!(event_look(&away), EventLook::Busy);
        assert_eq!(event_look(&event(9, 0, 30)), EventLook::Busy);
    }
}
