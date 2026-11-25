//! Picking a time block on the calendar's hour grid by dragging over empty slots.
//!
//! The grid snaps to its time scale (15, 30 or 60 minutes); a selection stays in one
//! day because the meeting form has a single date. See ADR 0023.

use chrono::{Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone};

use crate::week_grid::{MINUTES_PER_DAY, local_midnight};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSelection {
    pub date: NaiveDate,
    /// Minutes after the grid's midnight, snapped to the grid step.
    pub start_minute: i64,
    /// Exclusive end; at most a whole day (24:00).
    pub end_minute: i64,
}

/// What the meeting form gets from a picked block, in the zone's wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormTimes {
    pub date: NaiveDate,
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl FormTimes {
    pub fn label(&self, clock_format: &str) -> String {
        format!(
            "{} - {}",
            self.start.format(clock_format),
            self.end.format(clock_format)
        )
    }
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

    /// Whether the block still belongs on a grid showing `dates` at `step` minutes;
    /// a block from another range or another time scale is dropped.
    pub fn fits(&self, dates: &[NaiveDate], step: i64) -> bool {
        let step = step.max(1);
        dates.contains(&self.date)
            && self.start_minute % step == 0
            && (self.end_minute - self.start_minute) % step == 0
    }

    /// Date, start and end for the meeting form.
    ///
    /// The grid draws every day with one fixed `offset` (today's), so on the other
    /// side of a daylight saving switch a row is an hour away from its wall-clock
    /// time. Going through that origin puts the meeting where it was dragged. A block
    /// that runs past the form date's midnight ends at 23:59, since the form keeps
    /// start and end on one date.
    pub fn form_times<Tz: TimeZone>(&self, offset: FixedOffset, zone: &Tz) -> FormTimes {
        let origin = local_midnight(self.date, offset);
        let wall = |minute: i64| {
            (origin + Duration::minutes(minute))
                .with_timezone(zone)
                .naive_local()
        };
        let (start, end) = (wall(self.start_minute), wall(self.end_minute));
        let end = if end.date() == start.date() {
            end.time()
        } else {
            NaiveTime::from_hms_opt(23, 59, 0).unwrap_or_default()
        };
        FormTimes {
            date: start.date(),
            start: start.time(),
            end,
        }
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
fn span_between(anchor: i64, current: i64, step: i64) -> (i64, i64) {
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

    fn hours(hours: i32) -> FixedOffset {
        FixedOffset::east_opt(hours * 3600).unwrap()
    }

    fn time(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
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
    fn a_grid_without_height_or_step_does_not_panic() {
        assert_eq!(minute_at(300., 0., 30), 0);
        assert_eq!(minute_at(10. * HOUR, HOUR, 0), 600);
        assert_eq!(span_between(600, 600, 0), (600, 601));
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
        let form = selection.form_times(hours(1), &hours(1));
        assert_eq!(
            form,
            FormTimes {
                date: day(),
                start: time(10, 0),
                end: time(11, 30),
            }
        );
        assert_eq!(form.label("%H:%M"), "10:00 - 11:30");
        assert_eq!(form.label("%-I:%M %p"), "10:00 AM - 11:30 AM");
    }

    #[test]
    fn a_block_to_midnight_ends_at_23_59_in_the_form_and_the_label() {
        let selection = SlotSelection::between(day(), 23 * 60, 23 * 60 + 30, 30);
        assert_eq!(selection.end_minute, MINUTES_PER_DAY);
        let form = selection.form_times(hours(1), &hours(1));
        assert_eq!(form.end, time(23, 59));
        assert_eq!(form.label("%H:%M"), "23:00 - 23:59");
    }

    #[test]
    fn across_a_dst_switch_the_meeting_lands_where_it_was_dragged() {
        // Grid laid out with today's +01:00; the shown day is on +02:00 summer time.
        // The 09:00 row there holds 10:00 wall-clock meetings, so a block dragged
        // in it must become a 10:00 meeting.
        let selection = SlotSelection::between(day(), 9 * 60, 9 * 60 + 30, 30);
        let form = selection.form_times(hours(1), &hours(2));
        assert_eq!(
            (form.date, form.start, form.end),
            (day(), time(10, 0), time(11, 0))
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

    #[test]
    fn a_block_leaves_with_its_range_or_its_time_scale() {
        let selection = SlotSelection::between(day(), 615, 615, 15);
        let shown = [day()];
        assert!(selection.fits(&shown, 15));
        assert!(!selection.fits(&shown, 30));
        assert!(!selection.fits(&[day().succ_opt().unwrap()], 15));
        // A half-hour block still lines up on a 15-minute grid.
        assert!(SlotSelection::between(day(), 600, 630, 30).fits(&shown, 15));
    }
}
