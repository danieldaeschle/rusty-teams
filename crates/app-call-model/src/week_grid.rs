use chatsvc::{CalendarEvent, ResponseType};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Utc, Weekday};

pub const WORK_WEEK_DAYS: i64 = 5;
pub const MINUTES_PER_DAY: i64 = 24 * 60;

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
                if event.all_day {
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
    let end = start + Duration::days(days - 1);
    if start.month() == end.month() {
        start.format("%B %Y").to_string()
    } else if start.year() == end.year() {
        format!("{} - {}", start.format("%B"), end.format("%B %Y"))
    } else {
        format!("{} - {}", start.format("%B %Y"), end.format("%B %Y"))
    }
}

pub fn hour_label(hour: u32) -> String {
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
            response: ResponseType::Accepted,
        }
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
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
    fn labels_read_like_teams() {
        let monday = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        assert_eq!(range_label(monday, 5), "November - December 2026");
        assert_eq!(
            range_label(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), 5),
            "November 2026"
        );
        assert_eq!(hour_label(0), "12 AM");
        assert_eq!(hour_label(13), "1 PM");
    }
}
