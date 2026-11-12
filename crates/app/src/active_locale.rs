use app_base::ui_locale::{self, LocalePreference};
use gpui_kit::*;
use store::Store;

#[derive(Default)]
pub struct ActiveLocale {
    preference: LocalePreference,
}

impl Global for ActiveLocale {}

pub fn install(store: &Store, cx: &mut App) {
    let preference = ui_locale::load(store);
    ui_locale::apply_locale(ui_locale::effective_locale(preference));
    cx.set_global(ActiveLocale { preference });
}

pub fn current_preference(cx: &App) -> LocalePreference {
    cx.try_global::<ActiveLocale>()
        .map(|active| active.preference)
        .unwrap_or_default()
}

#[allow(dead_code)]
pub fn effective(cx: &App) -> &'static str {
    ui_locale::effective_locale(current_preference(cx))
}

pub fn set_preference(pref: LocalePreference, store: &Store, cx: &mut App) {
    ui_locale::save(store, pref);
    ui_locale::apply_locale(ui_locale::effective_locale(pref));
    cx.set_global(ActiveLocale { preference: pref });
    cx.refresh_windows();
}
