use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Utc, Weekday};

use crate::theme;

const BADGE_CAP: u32 = 99;
const WEEKDAY_WINDOW_DAYS: i64 = 6;

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
        Weekday::Mon => "Mo",
        Weekday::Tue => "Di",
        Weekday::Wed => "Mi",
        Weekday::Thu => "Do",
        Weekday::Fri => "Fr",
        Weekday::Sat => "Sa",
        Weekday::Sun => "So",
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
    let local = time.with_timezone(&offset);
    let day = local.date_naive();
    let days_ago = (today - day).num_days();
    if days_ago <= 0 {
        local.format("%H:%M").to_string()
    } else if days_ago <= WEEKDAY_WINDOW_DAYS {
        weekday_label(local.weekday()).to_owned()
    } else {
        local.format("%d.%m.").to_string()
    }
}

pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Heute".to_owned()
    } else if today.pred_opt() == Some(day) {
        "Gestern".to_owned()
    } else {
        format!(
            "{} {}",
            weekday_label(day.weekday()),
            day.format("%d.%m.%Y")
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
        assert_eq!(list_time_label(utc(5, 13), today(), zero()), "Mo");
        assert_eq!(list_time_label(utc(6, 9), today(), zero()), "Di");
        assert_eq!(list_time_label(utc(1, 13), today(), zero()), "Do");
        let september = Utc.with_ymd_and_hms(2026, 9, 30, 13, 0, 0).unwrap();
        assert_eq!(list_time_label(september, today(), zero()), "30.09.");
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
    fn day_labels() {
        assert_eq!(day_label(today(), today()), "Heute");
        assert_eq!(day_label(today().pred_opt().unwrap(), today()), "Gestern");
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), today()),
            "Do 01.10.2026"
        );
    }
}
