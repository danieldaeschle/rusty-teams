//! Regional format rows: region, time format and first day of week.

use app_base::format::{DateStyle, WeekdayWidth, weekday_name};
use app_base::regional::{ClockPreference, Region, RegionalPrefs, SystemFormat};
use app_base::ui_locale::tr_with;
use chrono::{NaiveTime, Weekday};
use gpui_kit::*;

use super::rows::{Choice, control_row, select_with_width};
use crate::app_state::AppState;
use crate::i18n::tr;

const SELECT_WIDTH: f32 = 240.;
const FIRST_DAY_CHOICES: [Weekday; 3] = [Weekday::Mon, Weekday::Sun, Weekday::Sat];

pub fn rows(app: &Entity<AppState>, cx: &App) -> Vec<Div> {
    let state = app.read(cx);
    let prefs = state.regional_prefs;
    let system = SystemFormat::read();
    let automatic =
        |change: fn(RegionalPrefs) -> RegionalPrefs| state.regional_style(&change(prefs), &system);
    let auto_clock = automatic(|prefs| RegionalPrefs {
        clock: ClockPreference::Auto,
        ..prefs
    });
    let auto_first_day = automatic(|prefs| RegionalPrefs {
        first_day: None,
        ..prefs
    })
    .first_day();
    let system_region = system
        .region()
        .map(Region::label)
        .or(system.locale.clone())
        .unwrap_or_default();
    let region_label = |region: Option<Region>| match region {
        Some(region) => region.label(),
        None => tr_with(
            "settings.appearance.region.follow_system",
            &[("region", &system_region)],
        ),
    };
    let mut regions = Region::ALL.to_vec();
    regions.sort_by_cached_key(|region| region.label());

    vec![
        control_row(
            tr("settings.appearance.region.title"),
            tr("settings.appearance.region.detail"),
            select_with_width(
                "settings-region",
                region_label(prefs.region),
                SELECT_WIDTH,
                false,
                std::iter::once(None)
                    .chain(regions.into_iter().map(Some))
                    .map(|region| {
                        choice(
                            app,
                            region_label(region),
                            prefs.region == region,
                            RegionalPrefs { region, ..prefs },
                        )
                    })
                    .collect(),
            ),
        ),
        control_row(
            tr("settings.appearance.clock.title"),
            tr("settings.appearance.clock.detail"),
            select_with_width(
                "settings-clock",
                clock_label(prefs.clock, auto_clock),
                SELECT_WIDTH,
                false,
                ClockPreference::ALL
                    .into_iter()
                    .map(|clock| {
                        choice(
                            app,
                            clock_label(clock, auto_clock),
                            prefs.clock == clock,
                            RegionalPrefs { clock, ..prefs },
                        )
                    })
                    .collect(),
            ),
        ),
        control_row(
            tr("settings.appearance.first_day.title"),
            tr("settings.appearance.first_day.detail"),
            select_with_width(
                "settings-first-day",
                first_day_label(prefs.first_day, auto_first_day),
                SELECT_WIDTH,
                false,
                std::iter::once(None)
                    .chain(FIRST_DAY_CHOICES.into_iter().map(Some))
                    .map(|first_day| {
                        choice(
                            app,
                            first_day_label(first_day, auto_first_day),
                            prefs.first_day == first_day,
                            RegionalPrefs { first_day, ..prefs },
                        )
                    })
                    .collect(),
            ),
        ),
    ]
}

fn choice(app: &Entity<AppState>, label: String, checked: bool, prefs: RegionalPrefs) -> Choice {
    let app = app.clone();
    Choice::new(label, checked, move |cx| {
        app.update(cx, |state, cx| state.set_regional_prefs(prefs, cx));
        cx.refresh_windows();
    })
}

/// Each choice names its example time: "24-hour (14:30)".
fn clock_label(clock: ClockPreference, automatic: DateStyle) -> String {
    let (key, style) = match clock {
        ClockPreference::Auto => ("settings.appearance.clock.auto", automatic),
        ClockPreference::Hours24 => (
            "settings.appearance.clock.hours_24",
            automatic.with_twelve_hour(false),
        ),
        ClockPreference::Hours12 => (
            "settings.appearance.clock.hours_12",
            automatic.with_twelve_hour(true),
        ),
    };
    let example = NaiveTime::from_hms_opt(14, 30, 0)
        .unwrap_or_default()
        .format(style.clock())
        .to_string();
    tr_with(key, &[("example", &example)])
}

fn first_day_label(first_day: Option<Weekday>, automatic: Weekday) -> String {
    match first_day {
        Some(day) => weekday_name(day, WeekdayWidth::Full),
        None => tr_with(
            "settings.appearance.first_day.auto",
            &[("day", &weekday_name(automatic, WeekdayWidth::Full))],
        ),
    }
}
