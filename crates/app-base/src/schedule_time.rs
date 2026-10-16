use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike, Weekday,
};

const LATER_TODAY_HOURS: i64 = 3;
const LATER_TODAY_CUTOFF_HOUR: u32 = 20;
const MORNING_HOUR: u32 = 8;
const SLOT_MINUTES: u32 = 30;
const TIME_STEP_MINUTES: i32 = 15;
const MAX_DAYS_AHEAD: i64 = 7;
pub const DAY_CHOICES: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetKind {
    LaterToday,
    TomorrowMorning,
    MondayMorning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub kind: PresetKind,
    pub label: &'static str,
    pub time_label: String,
    pub at: DateTime<Local>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleError {
    Past,
    TooFar,
}

impl ScheduleError {
    pub fn message(self) -> &'static str {
        match self {
            ScheduleError::Past => "That time has passed.",
            ScheduleError::TooFar => "Teams schedules up to 7 days ahead.",
        }
    }
}

pub fn validate(at: DateTime<Local>, now: DateTime<Local>) -> Result<(), ScheduleError> {
    if at <= now {
        Err(ScheduleError::Past)
    } else if at > now + Duration::days(MAX_DAYS_AHEAD) {
        Err(ScheduleError::TooFar)
    } else {
        Ok(())
    }
}

fn local_at(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&date.and_time(time)).earliest()
}

fn morning(date: NaiveDate) -> Option<DateTime<Local>> {
    local_at(date, NaiveTime::from_hms_opt(MORNING_HOUR, 0, 0)?)
}

fn later_today(now: DateTime<Local>) -> Option<DateTime<Local>> {
    let target = now + Duration::hours(LATER_TODAY_HOURS);
    let past_slot = !target.minute().is_multiple_of(SLOT_MINUTES)
        || target.second() != 0
        || target.nanosecond() != 0;
    let minutes_to_slot = if past_slot {
        i64::from(SLOT_MINUTES - target.minute() % SLOT_MINUTES)
    } else {
        0
    };
    let rounded = (target + Duration::minutes(minutes_to_slot))
        .with_second(0)?
        .with_nanosecond(0)?;
    let today = rounded.date_naive() == now.date_naive();
    (today && rounded.hour() < LATER_TODAY_CUTOFF_HOUR).then_some(rounded)
}

fn next_monday(today: NaiveDate) -> NaiveDate {
    let days_ahead = 7 - i64::from(today.weekday().num_days_from_monday());
    today + Duration::days(days_ahead)
}

pub fn presets(now: DateTime<Local>) -> Vec<Preset> {
    let today = now.date_naive();
    let tomorrow = today + Duration::days(1);
    let mut presets = Vec::new();
    if let Some(at) = later_today(now) {
        presets.push(Preset {
            kind: PresetKind::LaterToday,
            label: "Later today",
            time_label: at.format("%H:%M").to_string(),
            at,
        });
    }
    let weekend_ahead = matches!(tomorrow.weekday(), Weekday::Sat | Weekday::Sun);
    let monday = if tomorrow.weekday() == Weekday::Mon {
        None
    } else {
        morning(next_monday(today))
    };
    let tomorrow_morning = morning(tomorrow).filter(|_| !weekend_ahead);
    for (kind, label, at) in [
        (
            PresetKind::TomorrowMorning,
            "Tomorrow morning",
            tomorrow_morning,
        ),
        (PresetKind::MondayMorning, "Monday morning", monday),
    ] {
        if let Some(at) = at.filter(|at| validate(*at, now).is_ok()) {
            presets.push(Preset {
                kind,
                label,
                time_label: at.format("%a %H:%M").to_string(),
                at,
            });
        }
    }
    presets
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayChoice {
    pub date: NaiveDate,
    pub label: String,
}

pub fn day_choices(today: NaiveDate) -> Vec<DayChoice> {
    (0..DAY_CHOICES as i64)
        .map(|offset| {
            let date = today + Duration::days(offset);
            DayChoice {
                date,
                label: date.format("%a %-d %b").to_string(),
            }
        })
        .collect()
}

pub fn parse_time(text: &str) -> Option<NaiveTime> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let well_formed = (1..=2).contains(&hours.len())
        && minutes.len() == 2
        && hours
            .chars()
            .chain(minutes.chars())
            .all(|digit| digit.is_ascii_digit());
    if !well_formed {
        return None;
    }
    NaiveTime::from_hms_opt(hours.parse().ok()?, minutes.parse().ok()?, 0)
}

pub fn custom_time(day: NaiveDate, text: &str) -> Option<DateTime<Local>> {
    local_at(day, parse_time(text)?)
}

pub fn default_custom_time(now: DateTime<Local>) -> NaiveTime {
    let step = TIME_STEP_MINUTES as u32;
    let minutes = now.hour() * 60 + now.minute();
    let next = (minutes / step + 1) * step;
    NaiveTime::from_hms_opt(next / 60 % 24, next % 60, 0).unwrap_or_default()
}

pub fn step_time(time: NaiveTime, steps: i32) -> NaiveTime {
    let minutes = (time.hour() * 60 + time.minute()) as i32 + steps * TIME_STEP_MINUTES;
    let wrapped = minutes.rem_euclid(24 * 60) as u32;
    NaiveTime::from_hms_opt(wrapped / 60, wrapped % 60, 0).unwrap_or_default()
}

pub fn format_time(time: NaiveTime) -> String {
    time.format("%H:%M").to_string()
}

pub fn describe(at: DateTime<Local>, now: DateTime<Local>) -> String {
    let days_ahead = (at.date_naive() - now.date_naive()).num_days();
    match days_ahead {
        0 => at.format("today %H:%M").to_string(),
        1..=6 => at.format("%a %H:%M").to_string(),
        _ => at.format("%a %-d %b %H:%M").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap()
    }

    fn kinds(now: DateTime<Local>) -> Vec<PresetKind> {
        presets(now).iter().map(|preset| preset.kind).collect()
    }

    #[test]
    fn later_today_is_three_hours_rounded_up_to_the_half_hour() {
        let preset = &presets(at(6, 10, 10))[0];
        assert_eq!(preset.kind, PresetKind::LaterToday);
        assert_eq!(preset.at, at(6, 13, 30));
        assert_eq!(preset.time_label, "13:30");
        assert_eq!(presets(at(6, 10, 31))[0].at, at(6, 14, 0));
        assert_eq!(presets(at(6, 10, 30))[0].at, at(6, 13, 30));
    }

    #[test]
    fn later_today_needs_to_stay_today_and_before_eight_pm() {
        assert_eq!(presets(at(6, 16, 20))[0].at, at(6, 19, 30));
        assert_ne!(kinds(at(6, 16, 31))[0], PresetKind::LaterToday);
        assert_ne!(kinds(at(6, 22, 0))[0], PresetKind::LaterToday);
    }

    #[test]
    fn midweek_offers_tomorrow_and_next_monday() {
        let found = presets(at(8, 9, 0));
        let labels: Vec<_> = found.iter().map(|preset| preset.label).collect();
        assert_eq!(
            labels,
            ["Later today", "Tomorrow morning", "Monday morning"]
        );
        assert_eq!(found[1].time_label, "Fri 08:00");
        assert_eq!(found[1].at, at(9, 8, 0));
        assert_eq!(found[2].at, at(12, 8, 0));
        assert_eq!(found[2].time_label, "Mon 08:00");
    }

    #[test]
    fn friday_and_saturday_replace_tomorrow_with_monday() {
        for day in [9, 10] {
            let found = presets(at(day, 21, 0));
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].kind, PresetKind::MondayMorning);
            assert_eq!(found[0].at, at(12, 8, 0));
        }
    }

    #[test]
    fn sunday_shows_tomorrow_only_because_monday_would_duplicate_it() {
        let found = presets(at(11, 21, 0));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, PresetKind::TomorrowMorning);
        assert_eq!(found[0].time_label, "Mon 08:00");
    }

    #[test]
    fn next_monday_is_hidden_while_it_is_more_than_seven_days_away() {
        assert_eq!(
            kinds(at(5, 7, 0)),
            [PresetKind::LaterToday, PresetKind::TomorrowMorning]
        );
        assert_eq!(
            kinds(at(5, 21, 0)),
            [PresetKind::TomorrowMorning, PresetKind::MondayMorning]
        );
    }

    #[test]
    fn validation_blocks_the_past_and_more_than_seven_days() {
        let now = at(6, 10, 0);
        assert_eq!(validate(at(6, 9, 59), now), Err(ScheduleError::Past));
        assert_eq!(validate(now, now), Err(ScheduleError::Past));
        assert_eq!(validate(at(6, 10, 1), now), Ok(()));
        assert_eq!(validate(at(13, 10, 0), now), Ok(()));
        assert_eq!(validate(at(13, 10, 1), now), Err(ScheduleError::TooFar));
        assert_eq!(ScheduleError::Past.message(), "That time has passed.");
        assert_eq!(
            ScheduleError::TooFar.message(),
            "Teams schedules up to 7 days ahead."
        );
    }

    #[test]
    fn day_choices_run_from_today_for_seven_days() {
        let choices = day_choices(at(9, 12, 0).date_naive());
        assert_eq!(choices.len(), 7);
        assert_eq!(choices[0].label, "Fri 9 Oct");
        assert_eq!(choices[6].label, "Thu 15 Oct");
    }

    #[test]
    fn typed_times_accept_any_minute_and_reject_garbage() {
        let day = at(9, 0, 0).date_naive();
        assert_eq!(custom_time(day, "9:07"), Some(at(9, 9, 7)));
        assert_eq!(custom_time(day, " 09:45 "), Some(at(9, 9, 45)));
        assert_eq!(custom_time(day, "23:59"), Some(at(9, 23, 59)));
        for text in ["", "9", "24:00", "9:60", "9:5", "ab:cd", "9:30pm", "123:00"] {
            assert_eq!(parse_time(text), None, "{text}");
        }
    }

    #[test]
    fn the_default_custom_time_is_the_next_quarter_hour() {
        assert_eq!(format_time(default_custom_time(at(9, 10, 0))), "10:15");
        assert_eq!(format_time(default_custom_time(at(9, 10, 14))), "10:15");
        assert_eq!(format_time(default_custom_time(at(9, 23, 50))), "00:00");
    }

    #[test]
    fn stepping_moves_by_quarter_hours_and_wraps() {
        let time = NaiveTime::from_hms_opt(23, 55, 0).unwrap();
        assert_eq!(format_time(step_time(time, 1)), "00:10");
        assert_eq!(format_time(step_time(time, -2)), "23:25");
    }

    #[test]
    fn describing_a_time_names_today_or_the_weekday() {
        let now = at(9, 10, 0);
        assert_eq!(describe(at(9, 15, 30), now), "today 15:30");
        assert_eq!(describe(at(12, 8, 15), now), "Mon 08:15");
        assert_eq!(describe(at(16, 8, 15), now), "Fri 16 Oct 08:15");
    }
}
