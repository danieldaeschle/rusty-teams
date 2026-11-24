//! Picking a time block on the calendar's hour grid by dragging over empty slots.
//!
//! The grid snaps to its time scale (15, 30 or 60 minutes); a selection stays in one
//! day because the meeting form has a single date.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use crate::week_grid::MINUTES_PER_DAY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSelection {
    pub date: NaiveDate,
    /// Minutes after midnight, snapped to the grid step.
    pub start_minute: i64,
    /// Exclusive end; at most a whole day (24:00).
    pub end_minute: i64,
}

impl SlotSelection {
    /// The block from where the press started to where the pointer is now.
    pub fn between(date: NaiveDate, anchor: i64, current: i64, step: i64) -> Self {
        let (start_minute, end_minute) = span_between(anchor, current, step);
        SlotSelection {
            date,
            start_minute,
            end_minute,
        }
    }

    pub fn contains(&self, date: NaiveDate, minute: i64) -> bool {
        self.date == date && (self.start_minute..self.end_minute).contains(&minute)
    }

    /// Start and end for the meeting form. A block that runs to midnight ends at
    /// 23:59, since the form keeps start and end on the same date.
    pub fn times(&self) -> (NaiveTime, NaiveTime) {
        let time = |minute: i64| {
            let minute = minute.clamp(0, MINUTES_PER_DAY - 1);
            NaiveTime::from_hms_opt((minute / 60) as u32, (minute % 60) as u32, 0)
                .unwrap_or_default()
        };
        (time(self.start_minute), time(self.end_minute))
    }

    pub fn window(&self) -> (NaiveDateTime, NaiveDateTime) {
        let (start, end) = self.times();
        (self.date.and_time(start), self.date.and_time(end))
    }
}

/// The grid step a pixel offset inside a day column falls in, as its starting minute.
pub fn minute_at(y: f32, hour_height: f32, step: i64) -> i64 {
    let step = step.max(1);
    if hour_height <= 0. {
        return 0;
    }
    let minute = (y / hour_height * 60.).floor() as i64;
    (minute.div_euclid(step) * step).clamp(0, MINUTES_PER_DAY - step)
}

/// Both grid steps and everything between them, whichever way the pointer moved.
pub fn span_between(anchor: i64, current: i64, step: i64) -> (i64, i64) {
    let step = step.max(1);
    (
        anchor.min(current),
        (anchor.max(current) + step).min(MINUTES_PER_DAY),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: f32 = 48.;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 24).unwrap()
    }

    #[test]
    fn a_press_snaps_to_the_grid_step_it_lands_in() {
        // 10:40 on a 48px-per-hour grid.
        let y = 10. * HOUR + 40. / 60. * HOUR;
        assert_eq!(minute_at(y, HOUR, 15), 10 * 60 + 30);
        assert_eq!(minute_at(y, HOUR, 30), 10 * 60 + 30);
        assert_eq!(minute_at(y, HOUR, 60), 10 * 60);
    }

    #[test]
    fn presses_outside_the_column_stay_within_the_day() {
        assert_eq!(minute_at(-20., HOUR, 30), 0);
        assert_eq!(minute_at(25. * HOUR, HOUR, 30), 23 * 60 + 30);
        assert_eq!(minute_at(25. * HOUR, HOUR, 15), 23 * 60 + 45);
    }

    #[test]
    fn dragging_down_or_up_selects_the_same_block() {
        let down = SlotSelection::between(day(), 600, 630, 30);
        let up = SlotSelection::between(day(), 630, 600, 30);
        assert_eq!(down, up);
        assert_eq!((down.start_minute, down.end_minute), (600, 660));
    }

    #[test]
    fn a_click_selects_one_step() {
        assert_eq!(span_between(600, 600, 15), (600, 615));
        assert_eq!(span_between(600, 600, 60), (600, 660));
    }

    #[test]
    fn an_hour_and_a_half_gives_the_form_its_times() {
        let selection = SlotSelection::between(day(), 600, 660, 30);
        let (start, end) = selection.times();
        assert_eq!(start, NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        assert_eq!(end, NaiveTime::from_hms_opt(11, 30, 0).unwrap());
        assert_eq!(selection.window().0, day().and_time(start));
    }

    #[test]
    fn a_block_to_midnight_ends_at_23_59() {
        let selection = SlotSelection::between(day(), 23 * 60, 23 * 60 + 30, 30);
        assert_eq!(selection.end_minute, MINUTES_PER_DAY);
        assert_eq!(
            selection.times().1,
            NaiveTime::from_hms_opt(23, 59, 0).unwrap()
        );
    }

    #[test]
    fn contains_checks_day_and_minute() {
        let selection = SlotSelection::between(day(), 600, 660, 30);
        assert!(selection.contains(day(), 600));
        assert!(selection.contains(day(), 689));
        assert!(!selection.contains(day(), 690));
        assert!(!selection.contains(day().succ_opt().unwrap(), 620));
    }
}
