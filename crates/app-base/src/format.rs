use std::cell::Cell;

use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, Utc, Weekday};

use crate::theme;

/// `Local` follows the daylight saving rules at the given instant, a fixed offset is the same all year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Local,
    Fixed(FixedOffset),
}

impl Zone {
    pub fn at(self, time: DateTime<Utc>) -> DateTime<FixedOffset> {
        match self {
            Zone::Local => time.with_timezone(&Local).fixed_offset(),
            Zone::Fixed(offset) => time.with_timezone(&offset),
        }
    }
}

impl From<FixedOffset> for Zone {
    fn from(offset: FixedOffset) -> Self {
        Zone::Fixed(offset)
    }
}

const BADGE_CAP: u32 = 99;
const WEEKDAY_WINDOW_DAYS: i64 = 6;
const TWELVE_HOUR_LOCALES: [&str; 6] = ["en-us", "en-ca", "en-ph", "en-au", "en-nz", "en-in"];
const DOTTED_LOCALES: [&str; 7] = ["de", "da", "nb", "no", "fi", "cs", "sk"];
const PADDED_DOT_LOCALES: [&str; 4] = ["ru", "pl", "tr", "uk"];
/// Regions whose week starts on Sunday (CLDR first-day data for the common locales).
const SUNDAY_FIRST_LOCALES: [&str; 13] = [
    "en-us", "en-ca", "en-ph", "en-au", "en-nz", "en-in", "ja", "ko", "zh-tw", "he", "pt-br",
    "es-mx", "zh-hk",
];
/// Arabic regions that start on Monday instead of Saturday.
const ARABIC_MONDAY_LOCALES: [&str; 2] = ["ar-ma", "ar-tn"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateStyle {
    day_month: &'static str,
    day_month_year: &'static str,
    clock: &'static str,
    first_day: Weekday,
}

thread_local! {
    static DATE_STYLE: Cell<Option<DateStyle>> = const { Cell::new(None) };
}

pub fn set_date_locale(locale: Option<&str>) {
    DATE_STYLE.with(|style| style.set(locale.map(date_style)));
}

/// Teams desktop formats dates and times with the OS regional format, not the account language.
pub fn os_regional_locale() -> Option<String> {
    sys_locale::get_locale()
        .as_deref()
        .and_then(normalize_locale)
}

fn normalize_locale(raw: &str) -> Option<String> {
    let tag = raw
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    (!tag.is_empty() && tag != "C" && tag != "POSIX").then_some(tag)
}

fn current_date_style() -> Option<DateStyle> {
    DATE_STYLE.with(Cell::get)
}

/// First day of the week of the regional format; Monday when it is unknown.
pub fn regional_first_day() -> Weekday {
    current_date_style().map_or(Weekday::Mon, |style| style.first_day)
}

/// First day of the week for a locale tag such as `en-CH` (CLDR region data).
pub fn first_day_for_locale(locale: &str) -> Weekday {
    let locale = locale.to_ascii_lowercase().replace('_', "-");
    let language = locale.split('-').next().unwrap_or_default().to_owned();
    first_day_for(&locale, &language)
}

fn first_day_for(locale: &str, language: &str) -> Weekday {
    if SUNDAY_FIRST_LOCALES.contains(&locale) || SUNDAY_FIRST_LOCALES.contains(&language) {
        Weekday::Sun
    } else if (language == "ar" && !ARABIC_MONDAY_LOCALES.contains(&locale)) || language == "fa" {
        Weekday::Sat
    } else {
        Weekday::Mon
    }
}

pub fn clock_format() -> &'static str {
    current_date_style().map_or("%H:%M", |style| style.clock)
}

impl DateStyle {
    fn month_first(&self) -> bool {
        self.day_month.starts_with("%-m")
    }

    fn day_number(&self) -> &'static str {
        if self.day_month.starts_with("%d") {
            "%d"
        } else {
            "%-d"
        }
    }
}

pub fn calendar_day_number(date: NaiveDate) -> String {
    date.format(current_date_style().map_or("%-d", |style| style.day_number()))
        .to_string()
}

/// Month name in the app language (`calendar.month.mNN`), not chrono's English `%B`.
pub fn month_name(date: NaiveDate) -> String {
    crate::ui_locale::tr(&format!("calendar.month.m{:02}", date.month()))
}

/// "November 2026" / "November 2026" — month title of the month grid.
pub fn month_year_label(date: NaiveDate) -> String {
    format!("{} {}", month_name(date), date.year())
}

const WEEKDAY_KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekdayWidth {
    /// "Monday" / "Montag"
    Full,
    /// "Mon" / "Mo"
    Short,
    /// "M" / "M"
    Letter,
}

/// Weekday name in the app language; chrono's `%A`/`%a` are always English.
pub fn weekday_name(day: Weekday, width: WeekdayWidth) -> String {
    let key = WEEKDAY_KEYS[day.num_days_from_monday() as usize];
    match width {
        WeekdayWidth::Full => crate::ui_locale::tr(&format!("calendar.weekday.{key}")),
        WeekdayWidth::Short => crate::ui_locale::tr(&format!("calendar.weekday_short.{key}")),
        WeekdayWidth::Letter => crate::ui_locale::tr(&format!("calendar.weekday_short.{key}"))
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default(),
    }
}

pub fn calendar_range_label(start: NaiveDate, end: NaiveDate) -> String {
    let style = current_date_style();
    let day = |date: NaiveDate| {
        date.format(style.map_or("%-d", |style| style.day_number()))
            .to_string()
    };
    let (start_month, end_month) = (month_name(start), month_name(end));
    if style.is_none_or(|style| style.month_first()) {
        let to = format!("{end_month} {}, {}", day(end), end.year());
        return if start == end {
            to
        } else if start.year() != end.year() {
            format!("{start_month} {}, {} - {to}", day(start), start.year())
        } else if start.month() != end.month() {
            format!("{start_month} {} - {to}", day(start))
        } else {
            format!("{start_month} {}-{}, {}", day(start), day(end), end.year())
        };
    }
    if style.is_some_and(|style| style.day_month.ends_with('.')) {
        // German and other dotted styles: "16.–20. November 2026".
        let to = format!("{}. {end_month} {}", day(end), end.year());
        return if start == end {
            to
        } else if start.year() != end.year() {
            format!("{}. {start_month} {} – {to}", day(start), start.year())
        } else if start.month() != end.month() {
            format!("{}. {start_month} – {to}", day(start))
        } else {
            format!("{}.–{to}", day(start))
        };
    }
    let to = format!("{} {end_month}, {}", day(end), end.year());
    if start == end {
        to
    } else if start.year() != end.year() {
        format!("{} {start_month}, {} - {to}", day(start), start.year())
    } else if start.month() != end.month() {
        format!("{} {start_month} - {to}", day(start))
    } else {
        format!("{}-{to}", day(start))
    }
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
        _ if locale == "en-us" || locale == "en-ph" || locale == "en-ca" => {
            ("%-m/%-d", "%-m/%-d/%Y")
        }
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
        first_day: first_day_for(&locale, language),
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

/// Teams letters a team tile from the first and last word, split at spaces and underscores, in their own case.
pub fn team_initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split([' ', '_'])
        .filter(|word| word.chars().next().is_some_and(char::is_alphanumeric))
        .collect();
    match words.as_slice() {
        [] => "?".to_owned(),
        [single] => single.chars().take(1).collect(),
        [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
    }
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

pub fn list_time_label(time: DateTime<Utc>, today: NaiveDate, offset: impl Into<Zone>) -> String {
    list_time_label_in(current_date_style(), time, today, offset)
}

pub fn list_time_label_in(
    style: Option<DateStyle>,
    time: DateTime<Utc>,
    today: NaiveDate,
    offset: impl Into<Zone>,
) -> String {
    let local = offset.into().at(time);
    let day = local.date_naive();
    let days_ago = (today - day).num_days();
    if days_ago <= 0 {
        local
            .format(style.map_or("%H:%M", |style| style.clock))
            .to_string()
    } else if days_ago <= WEEKDAY_WINDOW_DAYS {
        weekday_label(local.weekday()).to_owned()
    } else {
        local
            .format(style.map_or("%b %-d", |style| style.day_month))
            .to_string()
    }
}

/// The last activity next to a channel name: the time today, else day and month, with the year once older.
pub fn channel_time_label(
    time: DateTime<Utc>,
    today: NaiveDate,
    offset: impl Into<Zone>,
) -> String {
    channel_time_label_in(current_date_style(), time, today, offset)
}

pub fn channel_time_label_in(
    style: Option<DateStyle>,
    time: DateTime<Utc>,
    today: NaiveDate,
    offset: impl Into<Zone>,
) -> String {
    let local = offset.into().at(time);
    let pattern = if local.date_naive() >= today {
        style.map_or("%H:%M", |style| style.clock)
    } else if local.year() == today.year() {
        style.map_or("%b %-d", |style| style.day_month)
    } else {
        style.map_or("%b %-d, %Y", |style| style.day_month_year)
    };
    local.format(pattern).to_string()
}

pub fn post_time_label(time: DateTime<Utc>, today: NaiveDate, offset: impl Into<Zone>) -> String {
    post_time_label_in(current_date_style(), time, today, offset)
}

pub fn post_time_label_in(
    style: Option<DateStyle>,
    time: DateTime<Utc>,
    today: NaiveDate,
    offset: impl Into<Zone>,
) -> String {
    let local = offset.into().at(time);
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

    #[test]
    fn the_first_day_of_week_follows_the_region() {
        let first = |locale| {
            set_date_locale(Some(locale));
            regional_first_day()
        };
        assert_eq!(first("en-US"), Weekday::Sun);
        assert_eq!(first("de-CH"), Weekday::Mon);
        assert_eq!(first("en-GB"), Weekday::Mon);
        assert_eq!(first("ar-SA"), Weekday::Sat);
        assert_eq!(first("ar-MA"), Weekday::Mon);
        assert_eq!(first("ja-JP"), Weekday::Sun);
        assert_eq!(first_day_for_locale("en-CH"), Weekday::Mon);
        assert_eq!(first_day_for_locale("en_US"), Weekday::Sun);
        set_date_locale(None);
        assert_eq!(regional_first_day(), Weekday::Mon);
    }

    #[test]
    fn calendar_names_follow_the_app_language() {
        let _guard = crate::ui_locale::locale_test_lock();
        let date = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        rust_i18n::set_locale("de");
        assert_eq!(weekday_name(Weekday::Mon, WeekdayWidth::Full), "Montag");
        assert_eq!(weekday_name(Weekday::Thu, WeekdayWidth::Short), "Do");
        assert_eq!(weekday_name(Weekday::Sun, WeekdayWidth::Letter), "S");
        assert_eq!(month_year_label(date), "November 2026");
        assert_eq!(
            month_name(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()),
            "März"
        );
        rust_i18n::set_locale("en");
        assert_eq!(weekday_name(Weekday::Mon, WeekdayWidth::Full), "Monday");
        assert_eq!(weekday_name(Weekday::Thu, WeekdayWidth::Letter), "T");
    }

    #[test]
    fn calendar_range_reads_german_with_dots() {
        let _guard = crate::ui_locale::locale_test_lock();
        let date = |month, day| NaiveDate::from_ymd_opt(2026, month, day).unwrap();
        rust_i18n::set_locale("de");
        set_date_locale(Some("de-CH"));
        assert_eq!(
            calendar_range_label(date(11, 16), date(11, 20)),
            "16.–20. November 2026"
        );
        assert_eq!(
            calendar_range_label(date(11, 30), date(12, 4)),
            "30. November – 4. Dezember 2026"
        );
        assert_eq!(
            calendar_range_label(date(11, 16), date(11, 16)),
            "16. November 2026"
        );
        rust_i18n::set_locale("en");
        set_date_locale(None);
    }

    #[test]
    fn calendar_range_follows_the_account_locale() {
        let _guard = crate::ui_locale::locale_test_lock();
        rust_i18n::set_locale("en");
        let date = |month, day| NaiveDate::from_ymd_opt(2026, month, day).unwrap();
        set_date_locale(Some("en-GB"));
        assert_eq!(
            calendar_range_label(date(11, 9), date(11, 15)),
            "09-15 November, 2026"
        );
        assert_eq!(
            calendar_range_label(date(11, 30), date(12, 4)),
            "30 November - 04 December, 2026"
        );
        assert_eq!(calendar_day_number(date(11, 9)), "09");
        set_date_locale(Some("en-US"));
        assert_eq!(
            calendar_range_label(date(11, 9), date(11, 13)),
            "November 9-13, 2026"
        );
        assert_eq!(calendar_day_number(date(11, 9)), "9");
        // Intl.DateTimeFormat in de-DE writes the day with a dot and no comma.
        set_date_locale(Some("de-DE"));
        assert_eq!(
            calendar_range_label(date(11, 9), date(11, 9)),
            "9. November 2026"
        );
        set_date_locale(None);
    }

    #[test]
    fn team_tiles_letter_first_and_last_word_like_teams() {
        assert_eq!(team_initials("e_IT-Support"), "eI");
        assert_eq!(team_initials("e_ComponentLibrary"), "eC");
        assert_eq!(team_initials("e_Human Resources"), "eR");
        assert_eq!(team_initials("e_NLP / AI"), "eA");
        assert_eq!(team_initials("Platform"), "P");
        assert_eq!(team_initials(""), "?");
    }

    #[test]
    fn os_locale_names_become_language_tags() {
        assert_eq!(normalize_locale("en-GB").as_deref(), Some("en-GB"));
        assert_eq!(normalize_locale("en_GB.UTF-8").as_deref(), Some("en-GB"));
        assert_eq!(normalize_locale("de_DE@euro").as_deref(), Some("de-DE"));
        assert_eq!(normalize_locale("C"), None);
        assert_eq!(normalize_locale("POSIX"), None);
        assert_eq!(normalize_locale(""), None);
    }

    #[test]
    fn the_clock_follows_the_display_language() {
        set_date_locale(Some("en-US"));
        assert_eq!(clock_format(), "%-I:%M %p");
        set_date_locale(Some("de-DE"));
        assert_eq!(clock_format(), "%H:%M");
        set_date_locale(None);
        assert_eq!(clock_format(), "%H:%M");
    }

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
    fn channel_times_show_the_clock_today_then_day_and_month_then_the_year() {
        let style = Some(date_style("en-gb"));
        let label = |time| channel_time_label_in(style, time, today(), zero());
        assert_eq!(label(utc(7, 8)), "08:05");
        assert_eq!(label(utc(1, 8)), "01/10");
        assert_eq!(
            label(Utc.with_ymd_and_hms(2025, 10, 23, 8, 0, 0).unwrap()),
            "23/10/2025"
        );
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
        let label =
            |locale: &str| post_time_label_in(Some(date_style(locale)), september, today(), zero());
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
        assert_eq!(
            list_time_label_in(style, september, today(), zero()),
            "24/09"
        );
        assert_eq!(
            list_time_label_in(style, utc(7, 13), today(), zero()),
            "13:05"
        );
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
