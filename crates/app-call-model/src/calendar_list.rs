//! Calendar list pane logic: group check state, Show all, mini month cells.

use chrono::{Datelike, NaiveDate};

/// Groups with more calendars than this hide the ones that are off until Show all.
pub const SHOW_ALL_AFTER: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupCheck {
    All,
    Some,
    None,
}

pub fn group_check(selected: &[bool]) -> GroupCheck {
    match selected.iter().filter(|on| **on).count() {
        0 => GroupCheck::None,
        count if count == selected.len() => GroupCheck::All,
        _ => GroupCheck::Some,
    }
}

/// Indexes of the calendars a group lists: all of them when short or expanded,
/// otherwise the ones turned on plus the primary one.
pub fn listed_calendars(selected: &[bool], primary: &[bool], expanded: bool) -> Vec<usize> {
    let all = 0..selected.len();
    if expanded || selected.len() <= SHOW_ALL_AFTER {
        return all.collect();
    }
    all.filter(|&index| selected[index] || primary.get(index).copied().unwrap_or(false))
        .collect()
}

/// Whether a group offers Show all / Show fewer.
pub fn offers_show_all(count: usize) -> bool {
    count > SHOW_ALL_AFTER
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniMonthCell {
    pub date: NaiveDate,
    pub in_month: bool,
    /// Inside the range the grid shows.
    pub shown: bool,
    pub today: bool,
}

pub fn mini_month_cell(
    date: NaiveDate,
    month: NaiveDate,
    shown: &[NaiveDate],
    today: NaiveDate,
) -> MiniMonthCell {
    MiniMonthCell {
        date,
        in_month: date.month() == month.month() && date.year() == month.year(),
        shown: shown.contains(&date),
        today: date == today,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_group_box_reads_all_some_or_none() {
        assert_eq!(group_check(&[true, true]), GroupCheck::All);
        assert_eq!(group_check(&[true, false]), GroupCheck::Some);
        assert_eq!(group_check(&[false, false]), GroupCheck::None);
    }

    #[test]
    fn a_long_group_lists_the_primary_and_the_ones_on_until_show_all() {
        let mut selected = [false; 8];
        selected[3] = true;
        selected[6] = true;
        let mut primary = [false; 8];
        primary[0] = true;
        assert_eq!(listed_calendars(&selected, &primary, false), [0, 3, 6]);
        assert_eq!(listed_calendars(&selected, &primary, true).len(), 8);
        assert!(offers_show_all(8));
        assert!(!offers_show_all(5));
        assert_eq!(listed_calendars(&[false; 4], &[false; 4], false).len(), 4);
    }

    #[test]
    fn mini_month_marks_the_shown_range_and_today() {
        let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
        let shown: Vec<_> = (16..=20).map(day).collect();
        let cell = mini_month_cell(day(18), day(1), &shown, day(15));
        assert!(cell.in_month && cell.shown && !cell.today);
        let today = mini_month_cell(day(15), day(1), &shown, day(15));
        assert!(today.today && !today.shown);
        let outside = mini_month_cell(
            NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
            day(1),
            &shown,
            day(15),
        );
        assert!(!outside.in_month);
    }
}
