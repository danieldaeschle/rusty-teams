use std::cell::Cell;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Utc, Weekday};

use crate::theme;

const BADGE_CAP: u32 = 99;
const WEEKDAY_WINDOW_DAYS: i64 = 6;
const TWELVE_HOUR_LOCALES: [&str; 6] = ["en-us", "en-ca", "en-ph", "en-au", "en-nz", "en-in"];
const DOTTED_LOCALES: [&str; 7] = ["de", "da", "nb", "no", "fi", "cs", "sk"];
const PADDED_DOT_LOCALES: [&str; 4] = ["ru", "pl", "tr", "uk"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateStyle {
    day_month: &'static str,
    day_month_year: &'static str,
    clock: &'static str,
}

thread_local! {
    static DATE_STYLE: Cell<Option<DateStyle>> = const { Cell::new(None) };
}

pub fn set_date_locale(locale: Option<&str>) {
    DATE_STYLE.with(|style| style.set(locale.map(date_style)));
}

fn current_date_style() -> Option<DateStyle> {
    DATE_STYLE.with(Cell::get)
}

/// Numeric day and month in the order of the account's display language, as `Intl.DateTimeFormat` does in Teams.
pub fn date_style(locale: &str) -> DateStyle {
    let locale = locale.to_ascii_lowercase();
    let language = locale.split(['-', '_']).next().unwrap_or_default();
    let twelve_hour = TWELVE_HOUR_LOCALES.contains(&locale.as_str());
    let clock = match language {
        _ if !twelve_hour => "%H:%M",
        _ if locale == "en-au" || locale == "en-nz" || locale == "en-in" => "%-I:%M %P",
        _ => "%-I:%M %p",
    };
    let (day_month, day_month_year) = match language {
        _ if locale == "en-us" || locale == "en-ph" || locale == "en-ca" => ("%-m/%-d", "%-m/%-d/%Y"),
        "ja" | "zh" => ("%-m/%-d", "%Y/%-m/%-d"),
        "nl" => ("%-d-%-m", "%-d-%-m-%Y"),
        "sv" => ("%-d/%-m", "%Y-%m-%d"),
        language if DOTTED_LOCALES.contains(&language) => ("%-d.%-m.", "%-d.%-m.%Y"),
        language if PADDED_DOT_LOCALES.contains(&language) => ("%d.%m", "%d.%m.%Y"),
        _ => ("%d/%m", "%d/%m/%Y"),
    };
    DateStyle {
        day_month,
        day_month_year,
        clock,
    }
}

pub fn palette_index(key: &str) -> usize {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in key.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as usize % theme::avatar_palette_len()
}

pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let letters: Vec<char> = match words.as_slice() {
        [] => return "?".to_owned(),
        [single] => single.chars().take(2).collect(),
        [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
    };
    letters.into_iter().flat_map(char::to_uppercase).collect()
}

pub fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

pub fn badge_text(count: u32) -> String {
    if count > BADGE_CAP {
        format!("{BADGE_CAP}+")
    } else {
        count.to_string()
    }
}

fn weekday_label(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

pub fn file_size_label(bytes: u64) -> String {
    const KIB: f64 = 1024.;
    let value = bytes as f64;
    if value < KIB {
        format!("{bytes} B")
    } else if value < KIB * KIB {
        format!("{:.0} KB", value / KIB)
    } else {
        format!("{:.1} MB", value / (KIB * KIB))
    }
}

pub fn short_version(build_id: &str, package_version: &str) -> String {
    let commit = build_id.split('-').next().unwrap_or(build_id);
    format!("v{package_version} ({commit})")
}

pub fn list_time_label(time: DateTime<Utc>, today: NaiveDate, offset: FixedOffset) -> String {
    list_time_label_in(current_date_style(), time, today, offset)
}

pub fn list_time_label_in(
    style: Option<DateStyle>,
    time: DateTime<Utc>,
    today: NaiveDate,
    offset: FixedOffset,
) -> String {
    let local = time.with_timezone(&offset);
    let day = local.date_naive();
    let days_ago = (today - day).num_days();
    if days_ago <= 0 {
        local.format(style.map_or("%H:%M", |style| style.clock)).to_string()
    } else if days_ago <= WEEKDAY_WINDOW_DAYS {
        weekday_label(local.weekday()).to_owned()
    } else {
        local
            .format(style.map_or("%b %-d", |style| style.day_month))
            .to_string()
    }
}

pub fn post_time_label(time: DateTime<Utc>, today: NaiveDate, offset: FixedOffset) -> String {
    post_time_label_in(current_date_style(), time, today, offset)
}

pub fn post_time_label_in(
    style: Option<DateStyle>,
    time: DateTime<Utc>,
    today: NaiveDate,
    offset: FixedOffset,
) -> String {
    let local = time.with_timezone(&offset);
    let day = local.date_naive();
    let days_ago = (today - day).num_days();
    let clock = local.format(style.map_or("%H:%M", |style| style.clock));
    let same_year = day.year() == today.year();
    if days_ago <= 0 {
        clock.to_string()
    } else if days_ago == 1 {
        format!("Yesterday {clock}")
    } else if days_ago <= WEEKDAY_WINDOW_DAYS {
        format!("{} {clock}", weekday_label(local.weekday()))
    } else if let Some(style) = style {
        let date = if same_year {
            style.day_month
        } else {
            style.day_month_year
        };
        format!("{} {clock}", local.format(date))
    } else if same_year {
        format!("{} {clock}", local.format("%b %-d"))
    } else {
        format!("{} {clock}", local.format("%b %-d, %Y"))
    }
}

pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".to_owned()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_owned()
    } else {
        format!(
            "{}, {}",
            weekday_label(day.weekday()),
            day.format("%b %-d, %Y")
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 5, 0).unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
    }

    fn zero() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    #[test]
    fn palette_index_is_stable_and_in_range() {
        assert_eq!(palette_index("user-1"), palette_index("user-1"));
        for key in [
            "a",
            "b",
            "demo-ada",
            "",
            "00000000-0000-0000-0000-000000000001",
        ] {
            assert!(palette_index(key) < theme::avatar_palette_len());
        }
    }

    #[test]
    fn palette_index_spreads_over_several_colors() {
        let used: std::collections::HashSet<usize> = (0..40)
            .map(|number| palette_index(&format!("user-{number}")))
            .collect();
        assert!(used.len() >= 4);
    }

    #[test]
    fn initials_use_first_and_last_word() {
        assert_eq!(initials("Mara Lindqvist"), "ML");
        assert_eq!(initials("Ada"), "AD");
        assert_eq!(initials("anna maria von Berg"), "AB");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("Release planning"), "RP");
    }

    #[test]
    fn first_name_takes_the_first_word() {
        assert_eq!(first_name("Priya Nair"), "Priya");
        assert_eq!(first_name("Priya"), "Priya");
    }

    #[test]
    fn file_sizes_use_binary_units() {
        assert_eq!(file_size_label(900), "900 B");
        assert_eq!(file_size_label(48_213), "47 KB");
        assert_eq!(file_size_label(5 * 1024 * 1024 + 300_000), "5.3 MB");
    }

    #[test]
    fn badge_caps_at_99_plus() {
        assert_eq!(badge_text(1), "1");
        assert_eq!(badge_text(99), "99");
        assert_eq!(badge_text(100), "99+");
        assert_eq!(badge_text(4000), "99+");
    }

    #[test]
    fn list_time_is_clock_today_weekday_this_week_date_before() {
        assert_eq!(list_time_label(utc(7, 13), today(), zero()), "13:05");
        assert_eq!(list_time_label(utc(5, 13), today(), zero()), "Mon");
        assert_eq!(list_time_label(utc(6, 9), today(), zero()), "Tue");
        assert_eq!(list_time_label(utc(1, 13), today(), zero()), "Thu");
        let september = Utc.with_ymd_and_hms(2026, 9, 30, 13, 0, 0).unwrap();
        assert_eq!(list_time_label(september, today(), zero()), "Sep 30");
    }

    #[test]
    fn list_time_respects_the_offset_for_the_day_boundary() {
        let plus_two = FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(list_time_label(utc(6, 23), today(), plus_two), "01:05");
    }

    #[test]
    fn short_version_keeps_the_commit_only() {
        assert_eq!(
            short_version("687e2dc-20261007T124355Z", "0.1.0"),
            "v0.1.0 (687e2dc)"
        );
        assert_eq!(short_version("dev", "0.1.0"), "v0.1.0 (dev)");
    }

    #[test]
    fn post_time_adds_the_day_when_not_today() {
        assert_eq!(post_time_label(utc(7, 13), today(), zero()), "13:05");
        assert_eq!(
            post_time_label(utc(6, 9), today(), zero()),
            "Yesterday 09:05"
        );
        assert_eq!(post_time_label(utc(5, 13), today(), zero()), "Mon 13:05");
        let september = Utc.with_ymd_and_hms(2026, 9, 30, 8, 42, 0).unwrap();
        assert_eq!(post_time_label(september, today(), zero()), "Sep 30 08:42");
        let last_year = Utc.with_ymd_and_hms(2020, 5, 22, 8, 42, 0).unwrap();
        assert_eq!(
            post_time_label(last_year, today(), zero()),
            "May 22, 2020 08:42"
        );
    }

    #[test]
    fn post_time_follows_the_display_language_like_teams() {
        let september = Utc.with_ymd_and_hms(2026, 9, 24, 20, 54, 0).unwrap();
        let label = |locale: &str| {
            post_time_label_in(Some(date_style(locale)), september, today(), zero())
        };
        assert_eq!(label("en-GB"), "24/09 20:54");
        assert_eq!(label("de-DE"), "24.9. 20:54");
        assert_eq!(label("en-US"), "9/24 8:54 PM");
        let last_year = Utc.with_ymd_and_hms(2020, 5, 2, 8, 42, 0).unwrap();
        assert_eq!(
            post_time_label_in(Some(date_style("en-GB")), last_year, today(), zero()),
            "02/05/2020 08:42"
        );
    }

    #[test]
    fn list_time_follows_the_display_language_like_teams() {
        let september = Utc.with_ymd_and_hms(2026, 9, 24, 20, 54, 0).unwrap();
        let style = Some(date_style("en-GB"));
        assert_eq!(list_time_label_in(style, september, today(), zero()), "24/09");
        assert_eq!(list_time_label_in(style, utc(7, 13), today(), zero()), "13:05");
    }

    #[test]
    fn day_labels() {
        assert_eq!(day_label(today(), today()), "Today");
        assert_eq!(day_label(today().pred_opt().unwrap(), today()), "Yesterday");
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), today()),
            "Thu, Oct 1, 2026"
        );
    }
}
