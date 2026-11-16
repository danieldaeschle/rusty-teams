//! Regional format: the date and time patterns, clock and week start the app uses.
//!
//! It is separate from the app language, as on the OS: English words in Austrian
//! patterns ("Mon, 16.11.", "14:30") is a normal setup. Teams has no account setting
//! for it and follows the OS, so the choice is stored locally.

use chrono::Weekday;
use serde::{Deserialize, Serialize};
use store::Store;

use crate::format::{self, DateStyle};

const PREFS_KEY: &str = "regional.format";
const FALLBACK_LOCALE: &str = "en";

/// Regions the date patterns in [`format::date_style`] know. A region decides the
/// patterns, whatever its language: Austria formats as `de-AT` in an English UI too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    Australia,
    Austria,
    Brazil,
    Canada,
    Czechia,
    Denmark,
    Finland,
    France,
    Germany,
    India,
    Ireland,
    Italy,
    Japan,
    Netherlands,
    Norway,
    Poland,
    Spain,
    Sweden,
    Switzerland,
    UnitedKingdom,
    UnitedStates,
}

impl Region {
    pub const ALL: [Region; 21] = [
        Region::Australia,
        Region::Austria,
        Region::Brazil,
        Region::Canada,
        Region::Czechia,
        Region::Denmark,
        Region::Finland,
        Region::France,
        Region::Germany,
        Region::India,
        Region::Ireland,
        Region::Italy,
        Region::Japan,
        Region::Netherlands,
        Region::Norway,
        Region::Poland,
        Region::Spain,
        Region::Sweden,
        Region::Switzerland,
        Region::UnitedKingdom,
        Region::UnitedStates,
    ];

    /// ISO 3166 region code.
    pub fn code(self) -> &'static str {
        match self {
            Region::Australia => "AU",
            Region::Austria => "AT",
            Region::Brazil => "BR",
            Region::Canada => "CA",
            Region::Czechia => "CZ",
            Region::Denmark => "DK",
            Region::Finland => "FI",
            Region::France => "FR",
            Region::Germany => "DE",
            Region::India => "IN",
            Region::Ireland => "IE",
            Region::Italy => "IT",
            Region::Japan => "JP",
            Region::Netherlands => "NL",
            Region::Norway => "NO",
            Region::Poland => "PL",
            Region::Spain => "ES",
            Region::Sweden => "SE",
            Region::Switzerland => "CH",
            Region::UnitedKingdom => "GB",
            Region::UnitedStates => "US",
        }
    }

    /// The locale whose patterns the region uses.
    fn format_locale(self) -> &'static str {
        match self {
            Region::Australia => "en-AU",
            Region::Austria => "de-AT",
            Region::Brazil => "pt-BR",
            Region::Canada => "en-CA",
            Region::Czechia => "cs-CZ",
            Region::Denmark => "da-DK",
            Region::Finland => "fi-FI",
            Region::France => "fr-FR",
            Region::Germany => "de-DE",
            Region::India => "en-IN",
            Region::Ireland => "en-IE",
            Region::Italy => "it-IT",
            Region::Japan => "ja-JP",
            Region::Netherlands => "nl-NL",
            Region::Norway => "nb-NO",
            Region::Poland => "pl-PL",
            Region::Spain => "es-ES",
            Region::Sweden => "sv-SE",
            Region::Switzerland => "de-CH",
            Region::UnitedKingdom => "en-GB",
            Region::UnitedStates => "en-US",
        }
    }

    pub fn from_code(code: &str) -> Option<Region> {
        Self::ALL
            .into_iter()
            .find(|region| region.code().eq_ignore_ascii_case(code))
    }

    /// The region of a locale tag such as `en-AT`.
    pub fn of_locale(locale: &str) -> Option<Region> {
        locale
            .split(['-', '_'])
            .skip(1)
            .find(|part| part.len() == 2)
            .and_then(Region::from_code)
    }

    /// Region name in the app language.
    pub fn label(self) -> String {
        crate::ui_locale::tr(&format!(
            "regional.region.{}",
            self.code().to_ascii_lowercase()
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockPreference {
    /// The region's clock, or the system's when the region follows the system.
    #[default]
    Auto,
    Hours24,
    Hours12,
}

impl ClockPreference {
    pub const ALL: [ClockPreference; 3] = [
        ClockPreference::Auto,
        ClockPreference::Hours24,
        ClockPreference::Hours12,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionalPrefs {
    /// `None` follows the system.
    pub region: Option<Region>,
    pub clock: ClockPreference,
    /// `None` follows the mailbox language as Outlook does, then the region.
    pub first_day: Option<Weekday>,
}

impl RegionalPrefs {
    /// The same preferences with every field on automatic except the region.
    pub fn automatic(self) -> RegionalPrefs {
        RegionalPrefs {
            region: self.region,
            ..RegionalPrefs::default()
        }
    }
}

/// `None` when the user never saved a choice.
pub fn load(store: &Store) -> Option<RegionalPrefs> {
    let json = store.meta(PREFS_KEY).ok().flatten()?;
    Some(serde_json::from_str(&json).unwrap_or_default())
}

pub fn save(store: &Store, prefs: &RegionalPrefs) {
    let json = serde_json::to_string(prefs).unwrap_or_default();
    if let Err(error) = store.set_meta(PREFS_KEY, &json) {
        log::warn!("regional format not saved: {error}");
    }
}

/// The OS regional format. Every field is `None` when the OS does not say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemFormat {
    /// Format locale such as `en-AT`; on macOS the region override wins over the language's region.
    pub locale: Option<String>,
    /// Set only when the user forced a clock on the OS.
    pub twelve_hour: Option<bool>,
    /// Set only when the OS has a first day of its own.
    pub first_day: Option<Weekday>,
}

impl SystemFormat {
    pub fn read() -> SystemFormat {
        platform::read()
    }

    pub fn region(&self) -> Option<Region> {
        self.locale.as_deref().and_then(Region::of_locale)
    }
}

/// What the regional format falls back to besides the user's choice.
#[derive(Debug, Clone, Copy)]
pub struct FormatSources<'a> {
    pub system: &'a SystemFormat,
    /// Teams account display language, used when the OS gives no locale.
    pub display_locale: Option<&'a str>,
    /// Outlook mailbox language; Outlook takes the default first day from it.
    pub mailbox_locale: Option<&'a str>,
}

/// The date style for the preferences. The region decides the patterns; the clock and
/// first day can be set on their own.
pub fn resolve(prefs: &RegionalPrefs, sources: FormatSources) -> DateStyle {
    let follows_system = prefs.region.is_none();
    let locale = match prefs.region {
        Some(region) => region.format_locale(),
        None => sources
            .system
            .locale
            .as_deref()
            .or(sources.display_locale)
            .unwrap_or(FALLBACK_LOCALE),
    };
    let locale = Region::of_locale(locale).map_or(locale, |region| region.format_locale());
    let mut style = format::date_style(locale);
    let twelve_hour = match prefs.clock {
        ClockPreference::Hours24 => Some(false),
        ClockPreference::Hours12 => Some(true),
        ClockPreference::Auto => sources.system.twelve_hour.filter(|_| follows_system),
    };
    if let Some(twelve_hour) = twelve_hour {
        style = style.with_twelve_hour(twelve_hour);
    }
    let first_day = prefs
        .first_day
        .or(sources.mailbox_locale.map(format::first_day_for_locale))
        .or(sources.system.first_day.filter(|_| follows_system));
    if let Some(first_day) = first_day {
        style = style.with_first_day(first_day);
    }
    style
}

/// `de_AT.UTF-8` and `de-AT` become `de-AT`; `C` and `POSIX` say nothing.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn normalize_locale(raw: &str) -> Option<String> {
    let tag = raw
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    (!tag.is_empty() && tag != "C" && tag != "POSIX").then_some(tag)
}

/// macOS keeps the region in `AppleLocale`; a region set apart from the language
/// is an `rg` keyword: `en_US@rg=atzzzz` formats as Austria.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn apple_locale_tag(raw: &str) -> Option<String> {
    let (base, keywords) = raw.split_once('@').unwrap_or((raw, ""));
    let mut parts = base.split(['_', '-']).filter(|part| !part.is_empty());
    let language = parts.next()?;
    let base_region = parts.find(|part| part.len() == 2);
    let override_region = keywords
        .split(';')
        .find_map(|keyword| keyword.strip_prefix("rg="))
        .and_then(|value| value.get(..2));
    Some(match override_region.or(base_region) {
        Some(region) => format!("{language}-{}", region.to_ascii_uppercase()),
        None => language.to_owned(),
    })
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;

    use chrono::Weekday;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_foundation_sys::dictionary::{CFDictionaryGetTypeID, CFDictionaryGetValue};
    use core_foundation_sys::preferences::{
        CFPreferencesCopyAppValue, kCFPreferencesAnyApplication,
    };

    use super::SystemFormat;

    /// A value of the global preferences domain (`defaults read -g`).
    fn global(key: &str) -> Option<CFType> {
        let key = CFString::new(key);
        // SAFETY: a valid key and the any-application domain; the copy is owned.
        let value = unsafe {
            CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), kCFPreferencesAnyApplication)
        };
        // SAFETY: non-null values come from a Copy function.
        (!value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
    }

    fn flag(key: &str) -> bool {
        global(key)
            .and_then(|value| value.downcast::<CFBoolean>())
            .is_some_and(bool::from)
    }

    /// `AppleFirstWeekday = { gregorian = 2 }`, counted from Sunday as 1.
    fn first_weekday() -> Option<Weekday> {
        let dictionary = global("AppleFirstWeekday")?;
        // SAFETY: plain type id lookup.
        if dictionary.type_of() != unsafe { CFDictionaryGetTypeID() } {
            return None;
        }
        let key = CFString::new("gregorian");
        // SAFETY: the dictionary type was checked; the value is borrowed from it.
        let value = unsafe {
            CFDictionaryGetValue(
                dictionary.as_CFTypeRef().cast(),
                key.as_concrete_TypeRef() as *const c_void,
            )
        };
        if value.is_null() {
            return None;
        }
        // SAFETY: non-null and owned by the dictionary, which outlives this call.
        let number = unsafe { CFType::wrap_under_get_rule(value) }.downcast::<CFNumber>()?;
        let from_sunday = number.to_i64().filter(|day| (1..=7).contains(day))?;
        // chrono counts from Monday as 0.
        Weekday::try_from(u8::try_from((from_sunday + 5) % 7).ok()?).ok()
    }

    pub fn read() -> SystemFormat {
        let locale = global("AppleLocale")
            .and_then(|value| value.downcast::<CFString>())
            .and_then(|value| super::apple_locale_tag(&value.to_string()));
        let twelve_hour = if flag("AppleICUForce24HourTime") {
            Some(false)
        } else if flag("AppleICUForce12HourTime") {
            Some(true)
        } else {
            None
        };
        SystemFormat {
            locale,
            twelve_hour,
            first_day: first_weekday(),
        }
    }
}

#[cfg(windows)]
mod platform {
    use chrono::Weekday;
    use windows::Win32::Globalization::{
        GetLocaleInfoEx, GetUserDefaultLocaleName, LOCALE_IFIRSTDAYOFWEEK, LOCALE_ITIME,
    };
    use windows::core::PCWSTR;

    use super::SystemFormat;

    const LOCALE_NAME_MAX_LENGTH: usize = 85;

    /// A value of the user's regional format, with the user's overrides applied.
    fn user_info(kind: u32) -> Option<String> {
        let mut buffer = [0u16; 8];
        // SAFETY: a null name reads the user default locale; the buffer is sized for the value.
        let length = unsafe { GetLocaleInfoEx(PCWSTR::null(), kind, Some(&mut buffer)) };
        let length = usize::try_from(length).ok()?.checked_sub(1)?;
        Some(String::from_utf16_lossy(&buffer[..length]))
    }

    pub fn read() -> SystemFormat {
        let mut name = [0u16; LOCALE_NAME_MAX_LENGTH];
        // SAFETY: the buffer has the documented maximum length.
        let length = unsafe { GetUserDefaultLocaleName(&mut name) };
        let locale = usize::try_from(length)
            .ok()
            .and_then(|length| length.checked_sub(1))
            .map(|length| String::from_utf16_lossy(&name[..length]))
            .and_then(|raw| super::normalize_locale(&raw));
        // `LOCALE_IFIRSTDAYOFWEEK` counts from Monday as 0.
        let first_day = user_info(LOCALE_IFIRSTDAYOFWEEK)
            .and_then(|value| value.parse::<u8>().ok())
            .and_then(|from_monday| Weekday::try_from(from_monday).ok());
        SystemFormat {
            locale,
            twelve_hour: user_info(LOCALE_ITIME).map(|value| value == "0"),
            first_day,
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::SystemFormat;

    /// `LC_TIME` is the date and time format; `LC_ALL` overrides it, `LANG` is the default.
    pub fn read() -> SystemFormat {
        let locale = ["LC_ALL", "LC_TIME", "LANG"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
            .and_then(|raw| super::normalize_locale(&raw));
        SystemFormat {
            locale,
            ..SystemFormat::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn system(locale: &str) -> SystemFormat {
        SystemFormat {
            locale: Some(locale.to_owned()),
            ..SystemFormat::default()
        }
    }

    fn style(prefs: RegionalPrefs, system: &SystemFormat, mailbox: Option<&str>) -> DateStyle {
        resolve(
            &prefs,
            FormatSources {
                system,
                display_locale: None,
                mailbox_locale: mailbox,
            },
        )
    }

    #[test]
    fn the_macos_region_override_wins_over_the_language() {
        assert_eq!(
            apple_locale_tag("en_US@rg=atzzzz").as_deref(),
            Some("en-AT")
        );
        assert_eq!(apple_locale_tag("de_AT").as_deref(), Some("de-AT"));
        assert_eq!(apple_locale_tag("zh-Hans_CN").as_deref(), Some("zh-CN"));
        assert_eq!(apple_locale_tag("en").as_deref(), Some("en"));
    }

    #[test]
    fn os_locale_names_become_language_tags() {
        assert_eq!(normalize_locale("en_GB.UTF-8").as_deref(), Some("en-GB"));
        assert_eq!(normalize_locale("de_DE@euro").as_deref(), Some("de-DE"));
        assert_eq!(normalize_locale("C"), None);
        assert_eq!(normalize_locale(""), None);
    }

    #[test]
    fn english_in_austria_formats_as_austria() {
        let style = style(RegionalPrefs::default(), &system("en-AT"), None);
        let date = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
        assert_eq!(style.clock(), "%H:%M");
        assert_eq!(style.first_day(), Weekday::Mon);
        format::set_date_style(Some(style));
        assert_eq!(format::date_label(date), "9.3.2026");
        assert_eq!(format::decimal_label(1.5, 1), "1,5");
        format::set_date_style(None);
    }

    #[test]
    fn a_chosen_region_ignores_the_system_clock_and_first_day() {
        let forced = SystemFormat {
            twelve_hour: Some(false),
            first_day: Some(Weekday::Sat),
            ..system("de-AT")
        };
        let follows = style(RegionalPrefs::default(), &forced, None);
        assert!(!follows.twelve_hour());
        assert_eq!(follows.first_day(), Weekday::Sat);
        let chosen = RegionalPrefs {
            region: Some(Region::UnitedStates),
            ..RegionalPrefs::default()
        };
        let us = style(chosen, &forced, None);
        assert!(us.twelve_hour());
        assert_eq!(us.first_day(), Weekday::Sun);
    }

    #[test]
    fn the_first_day_is_the_choice_then_the_mailbox_then_the_region() {
        let us = system("en-US");
        assert_eq!(
            style(RegionalPrefs::default(), &us, None).first_day(),
            Weekday::Sun
        );
        assert_eq!(
            style(RegionalPrefs::default(), &us, Some("en-AT")).first_day(),
            Weekday::Mon
        );
        let chosen = RegionalPrefs {
            first_day: Some(Weekday::Sat),
            ..RegionalPrefs::default()
        };
        assert_eq!(style(chosen, &us, Some("en-AT")).first_day(), Weekday::Sat);
    }

    #[test]
    fn the_clock_choice_overrides_the_region() {
        let hours_12 = RegionalPrefs {
            clock: ClockPreference::Hours12,
            ..RegionalPrefs::default()
        };
        assert_eq!(style(hours_12, &system("de-AT"), None).clock(), "%-I:%M %p");
        let hours_24 = RegionalPrefs {
            clock: ClockPreference::Hours24,
            ..RegionalPrefs::default()
        };
        assert_eq!(style(hours_24, &system("en-US"), None).clock(), "%H:%M");
        // A 12-hour region keeps its own spelling.
        assert_eq!(style(hours_12, &system("en-AU"), None).clock(), "%-I:%M %P");
    }

    #[test]
    fn prefs_are_none_until_saved_and_round_trip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load(&store), None);
        let prefs = RegionalPrefs {
            region: Some(Region::Austria),
            clock: ClockPreference::Hours24,
            first_day: Some(Weekday::Mon),
        };
        save(&store, &prefs);
        assert_eq!(load(&store), Some(prefs));
    }
}
