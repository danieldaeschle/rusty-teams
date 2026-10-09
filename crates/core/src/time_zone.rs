use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Utc, Weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dst {
    None,
    Europe,
    NorthAmerica,
    Australia,
    NewZealand,
}

const ZONES: &[(&str, i32, Dst)] = &[
    ("UTC", 0, Dst::None),
    ("Dateline Standard Time", -720, Dst::None),
    ("UTC-11", -660, Dst::None),
    ("Hawaiian Standard Time", -600, Dst::None),
    ("Alaskan Standard Time", -540, Dst::NorthAmerica),
    ("Pacific Standard Time", -480, Dst::NorthAmerica),
    ("US Mountain Standard Time", -420, Dst::None),
    ("Mountain Standard Time", -420, Dst::NorthAmerica),
    ("Central Standard Time", -360, Dst::NorthAmerica),
    ("Central America Standard Time", -360, Dst::None),
    ("Canada Central Standard Time", -360, Dst::None),
    ("Mexico Standard Time", -360, Dst::None),
    ("Eastern Standard Time", -300, Dst::NorthAmerica),
    ("US Eastern Standard Time", -300, Dst::NorthAmerica),
    ("SA Pacific Standard Time", -300, Dst::None),
    ("Atlantic Standard Time", -240, Dst::NorthAmerica),
    ("Newfoundland Standard Time", -210, Dst::NorthAmerica),
    ("E. South America Standard Time", -180, Dst::None),
    ("Argentina Standard Time", -180, Dst::None),
    ("GMT Standard Time", 0, Dst::Europe),
    ("Greenwich Standard Time", 0, Dst::None),
    ("W. Europe Standard Time", 60, Dst::Europe),
    ("Central Europe Standard Time", 60, Dst::Europe),
    ("Romance Standard Time", 60, Dst::Europe),
    ("Central European Standard Time", 60, Dst::Europe),
    ("W. Central Africa Standard Time", 60, Dst::None),
    ("South Africa Standard Time", 120, Dst::None),
    ("E. Europe Standard Time", 120, Dst::Europe),
    ("GTB Standard Time", 120, Dst::Europe),
    ("FLE Standard Time", 120, Dst::Europe),
    ("Turkey Standard Time", 180, Dst::None),
    ("Arab Standard Time", 180, Dst::None),
    ("Russian Standard Time", 180, Dst::None),
    ("E. Africa Standard Time", 180, Dst::None),
    ("Iran Standard Time", 210, Dst::None),
    ("Arabian Standard Time", 240, Dst::None),
    ("Pakistan Standard Time", 300, Dst::None),
    ("West Asia Standard Time", 300, Dst::None),
    ("India Standard Time", 330, Dst::None),
    ("Sri Lanka Standard Time", 330, Dst::None),
    ("Nepal Standard Time", 345, Dst::None),
    ("Central Asia Standard Time", 360, Dst::None),
    ("Bangladesh Standard Time", 360, Dst::None),
    ("SE Asia Standard Time", 420, Dst::None),
    ("China Standard Time", 480, Dst::None),
    ("Singapore Standard Time", 480, Dst::None),
    ("W. Australia Standard Time", 480, Dst::None),
    ("Taipei Standard Time", 480, Dst::None),
    ("Tokyo Standard Time", 540, Dst::None),
    ("Korea Standard Time", 540, Dst::None),
    ("AUS Central Standard Time", 570, Dst::None),
    ("Cen. Australia Standard Time", 570, Dst::Australia),
    ("E. Australia Standard Time", 600, Dst::None),
    ("AUS Eastern Standard Time", 600, Dst::Australia),
    ("Tasmania Standard Time", 600, Dst::Australia),
    ("New Zealand Standard Time", 720, Dst::NewZealand),
];

/// `None` for a zone outside the table.
pub fn local_time(zone_name: &str, now: DateTime<Utc>) -> Option<DateTime<FixedOffset>> {
    let (_, standard_minutes, dst) = ZONES
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(zone_name.trim()))?;
    let saving = if is_daylight_time(*dst, *standard_minutes, now) {
        60
    } else {
        0
    };
    let offset = FixedOffset::east_opt((standard_minutes + saving) * 60)?;
    Some(now.with_timezone(&offset))
}

fn is_daylight_time(rule: Dst, standard_minutes: i32, now: DateTime<Utc>) -> bool {
    let year = now.year();
    let utc = |date: NaiveDate, local_standard_minutes: i32| {
        Utc.from_utc_datetime(
            &date
                .and_hms_opt(0, 0, 0)
                .expect("midnight exists")
                .checked_add_signed(Duration::minutes(i64::from(
                    local_standard_minutes - standard_minutes,
                )))
                .expect("transition is in range"),
        )
    };
    let sunday = |month: u32, nth: Option<u32>| match nth {
        Some(nth) => nth_sunday(year, month, nth),
        None => last_sunday(year, month),
    };
    match rule {
        Dst::None => false,
        Dst::Europe => {
            utc(sunday(3, None), 60 + standard_minutes) <= now
                && now < utc(sunday(10, None), 60 + standard_minutes)
        }
        Dst::NorthAmerica => {
            utc(sunday(3, Some(2)), 120) <= now && now < utc(sunday(11, Some(1)), 60)
        }
        Dst::Australia => {
            utc(sunday(10, Some(1)), 120) <= now || now < utc(sunday(4, Some(1)), 120)
        }
        Dst::NewZealand => utc(sunday(9, None), 120) <= now || now < utc(sunday(4, Some(1)), 120),
    }
}

fn nth_sunday(year: i32, month: u32, nth: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("month is valid");
    let to_sunday = (7 - first.weekday().num_days_from_sunday()) % 7;
    first + Duration::days(i64::from(to_sunday + 7 * (nth - 1)))
}

fn last_sunday(year: i32, month: u32) -> NaiveDate {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let mut day = NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("month is valid")
        - Duration::days(1);
    while day.weekday() != Weekday::Sun {
        day -= Duration::days(1);
    }
    day
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn clock(
        zone: &str,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
    ) -> Option<String> {
        let now = Utc
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap();
        local_time(zone, now).map(|time| time.format("%H:%M %z").to_string())
    }

    #[test]
    fn europe_switches_on_the_last_sunday_at_one_utc() {
        let zone = "W. Europe Standard Time";
        assert_eq!(
            clock(zone, 2026, 1, 15, 12, 0).as_deref(),
            Some("13:00 +0100")
        );
        assert_eq!(
            clock(zone, 2026, 3, 29, 0, 59).as_deref(),
            Some("01:59 +0100")
        );
        assert_eq!(
            clock(zone, 2026, 3, 29, 1, 0).as_deref(),
            Some("03:00 +0200")
        );
        assert_eq!(
            clock(zone, 2026, 10, 9, 12, 0).as_deref(),
            Some("14:00 +0200")
        );
        assert_eq!(
            clock(zone, 2026, 10, 25, 0, 59).as_deref(),
            Some("02:59 +0200")
        );
        assert_eq!(
            clock(zone, 2026, 10, 25, 1, 0).as_deref(),
            Some("02:00 +0100")
        );
    }

    #[test]
    fn gmt_and_eastern_europe_follow_the_same_utc_instant() {
        assert_eq!(
            clock("GMT Standard Time", 2026, 7, 1, 12, 0).as_deref(),
            Some("13:00 +0100")
        );
        assert_eq!(
            clock("GTB Standard Time", 2026, 3, 29, 1, 0).as_deref(),
            Some("04:00 +0300")
        );
    }

    #[test]
    fn north_america_switches_on_the_second_sunday_of_march() {
        let zone = "Pacific Standard Time";
        assert_eq!(
            clock(zone, 2026, 3, 8, 9, 59).as_deref(),
            Some("01:59 -0800")
        );
        assert_eq!(
            clock(zone, 2026, 3, 8, 10, 0).as_deref(),
            Some("03:00 -0700")
        );
        assert_eq!(
            clock(zone, 2026, 11, 1, 8, 59).as_deref(),
            Some("01:59 -0700")
        );
        assert_eq!(
            clock(zone, 2026, 11, 1, 9, 0).as_deref(),
            Some("01:00 -0800")
        );
    }

    #[test]
    fn southern_zones_are_daylight_time_over_the_new_year() {
        assert_eq!(
            clock("AUS Eastern Standard Time", 2026, 1, 15, 0, 0).as_deref(),
            Some("11:00 +1100")
        );
        assert_eq!(
            clock("AUS Eastern Standard Time", 2026, 7, 15, 0, 0).as_deref(),
            Some("10:00 +1000")
        );
        assert_eq!(
            clock("New Zealand Standard Time", 2026, 12, 1, 0, 0).as_deref(),
            Some("13:00 +1300")
        );
        assert_eq!(
            clock("New Zealand Standard Time", 2026, 6, 1, 0, 0).as_deref(),
            Some("12:00 +1200")
        );
    }

    #[test]
    fn zones_without_daylight_time_keep_their_offset() {
        assert_eq!(
            clock("India Standard Time", 2026, 7, 1, 12, 0).as_deref(),
            Some("17:30 +0530")
        );
        assert_eq!(
            clock("UTC", 2026, 7, 1, 12, 0).as_deref(),
            Some("12:00 +0000")
        );
    }

    #[test]
    fn unknown_zones_have_no_clock() {
        assert_eq!(clock("Custom Zone", 2026, 7, 1, 12, 0), None);
    }

    #[test]
    fn zone_names_ignore_case() {
        assert!(clock("w. europe standard time", 2026, 7, 1, 12, 0).is_some());
    }
}
