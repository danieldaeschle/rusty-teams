//! Regional format on `AppState`: the saved choice and the date style every label reads.

use app_base::regional::{self, FormatSources, RegionalPrefs, SystemFormat};
use gpui_kit::Context;
use store::Store;

use crate::app_state::AppState;
use crate::call::calendar_prefs::{CALENDAR_PREFS_KEY, CalendarPrefs};
use crate::format::{self, DateStyle};

/// The saved choice. Before the first save, a first day set in the calendar settings carries over.
pub(crate) fn load_regional_prefs(store: &Store) -> RegionalPrefs {
    if let Some(prefs) = regional::load(store) {
        return prefs;
    }
    let first_day = store
        .meta(CALENDAR_PREFS_KEY)
        .ok()
        .flatten()
        .as_deref()
        .and_then(CalendarPrefs::legacy_first_day);
    let prefs = RegionalPrefs {
        first_day,
        ..RegionalPrefs::default()
    };
    if first_day.is_some() {
        regional::save(store, &prefs);
    }
    prefs
}

impl AppState {
    /// The style `prefs` resolve to with the OS, account and mailbox as they are now.
    pub fn regional_style(&self, prefs: &RegionalPrefs, system: &SystemFormat) -> DateStyle {
        let display_locale = self
            .translation
            .settings
            .as_ref()
            .and_then(|settings| settings.display_locale.as_deref());
        regional::resolve(
            prefs,
            FormatSources {
                system,
                display_locale,
                mailbox_locale: self.calendar_prefs.mailbox_locale.as_deref(),
            },
        )
    }

    /// Set the date style that labels read; call after any of its sources changed.
    pub fn apply_regional_format(&self) {
        let style = self.regional_style(&self.regional_prefs, &SystemFormat::read());
        format::set_date_style(Some(style));
    }

    pub fn set_regional_prefs(&mut self, prefs: RegionalPrefs, cx: &mut Context<Self>) {
        if prefs == self.regional_prefs {
            return;
        }
        self.regional_prefs = prefs;
        regional::save(&self.store, &prefs);
        self.apply_regional_format();
        self.reanchor_calendar(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use chrono::Weekday;

    use super::*;

    #[test]
    fn the_calendar_first_day_carries_over_once() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_meta(CALENDAR_PREFS_KEY, r#"{"first_day":"Sat"}"#)
            .unwrap();
        assert_eq!(load_regional_prefs(&store).first_day, Some(Weekday::Sat));
        store.set_meta(CALENDAR_PREFS_KEY, "{}").unwrap();
        assert_eq!(load_regional_prefs(&store).first_day, Some(Weekday::Sat));
    }
}
