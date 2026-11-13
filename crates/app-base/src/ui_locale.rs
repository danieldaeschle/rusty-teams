use std::collections::HashMap;

use store::Store;

const META_KEY: &str = "ui.locale";
const PREFERENCE_KEY: &str = "preference";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocalePreference {
    #[default]
    Auto,
    English,
    German,
}

impl LocalePreference {
    pub const ALL: [LocalePreference; 3] = [
        LocalePreference::Auto,
        LocalePreference::English,
        LocalePreference::German,
    ];

    pub fn key(self) -> &'static str {
        match self {
            LocalePreference::Auto => "auto",
            LocalePreference::English => "en",
            LocalePreference::German => "de",
        }
    }

    pub fn from_key(key: &str) -> Option<LocalePreference> {
        Self::ALL
            .into_iter()
            .find(|preference| preference.key() == key)
    }
}

pub fn os_language_tag() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en".to_owned())
}

pub fn effective_locale(pref: LocalePreference) -> &'static str {
    effective_locale_for(pref, &os_language_tag())
}

pub(crate) fn effective_locale_for(pref: LocalePreference, os_tag: &str) -> &'static str {
    match pref {
        LocalePreference::German => "de",
        LocalePreference::English => "en",
        LocalePreference::Auto => {
            if os_tag.to_lowercase().starts_with("de") {
                "de"
            } else {
                "en"
            }
        }
    }
}

pub fn apply_locale(locale: &str) {
    rust_i18n::set_locale(locale);
    gpui_kit::component::set_locale(locale);
}

pub fn load(store: &Store) -> LocalePreference {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save(store: &Store, pref: LocalePreference) {
    let _ = store.set_meta(META_KEY, &serialize(pref));
}

fn serialize(pref: LocalePreference) -> String {
    format!("{PREFERENCE_KEY}={}", pref.key())
}

fn parse(text: &str) -> LocalePreference {
    for line in text.lines() {
        if let Some((PREFERENCE_KEY, value)) = line.split_once('=') {
            if let Some(pref) = LocalePreference::from_key(value.trim()) {
                return pref;
            }
        }
    }
    LocalePreference::default()
}

pub fn tr(key: &str) -> String {
    rust_i18n::t!(key).to_string()
}

pub fn tr_args(key: &str, args: &HashMap<String, String>) -> String {
    if args.is_empty() {
        return tr(key);
    }
    let patterns: Vec<&str> = args.keys().map(String::as_str).collect();
    let values: Vec<String> = patterns.iter().map(|name| args[*name].clone()).collect();
    rust_i18n::replace_patterns(&tr(key), &patterns, &values)
}

/// Separator between an i18n key and a detail payload in `CallUpdate::Notice`.
pub const NOTICE_DETAIL_SEP: char = '\u{1f}';

/// Resolve a call/UI notice payload: bare key, or `key` + detail after [`NOTICE_DETAIL_SEP`].
pub fn localize_notice(payload: &str) -> String {
    if let Some((key, detail)) = payload.split_once(NOTICE_DETAIL_SEP) {
        let mut args = HashMap::new();
        args.insert("detail".to_owned(), detail.to_owned());
        return tr_args(key, &args);
    }
    if payload.contains('.') {
        return tr(payload);
    }
    payload.to_owned()
}

pub fn notice_payload(key: &str, detail: impl std::fmt::Display) -> String {
    format!("{key}{NOTICE_DETAIL_SEP}{detail}")
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;
    use tempfile::tempdir;

    fn locale_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn ui_locale_effective_prefers_explicit_english_and_german() {
        let _guard = locale_test_lock();
        assert_eq!(
            effective_locale_for(LocalePreference::English, "de-DE"),
            "en"
        );
        assert_eq!(
            effective_locale_for(LocalePreference::German, "en-US"),
            "de"
        );
    }

    #[test]
    fn ui_locale_effective_auto_follows_os_tag() {
        let _guard = locale_test_lock();
        assert_eq!(effective_locale_for(LocalePreference::Auto, "de-DE"), "de");
        assert_eq!(effective_locale_for(LocalePreference::Auto, "de"), "de");
        assert_eq!(effective_locale_for(LocalePreference::Auto, "en-US"), "en");
        assert_eq!(effective_locale_for(LocalePreference::Auto, "fr-FR"), "en");
    }

    #[test]
    fn ui_locale_load_save_roundtrip_in_store() {
        let _guard = locale_test_lock();
        let store = Store::open_in_memory().expect("in-memory store");
        assert_eq!(load(&store), LocalePreference::Auto);
        save(&store, LocalePreference::German);
        assert_eq!(load(&store), LocalePreference::German);
        save(&store, LocalePreference::English);
        assert_eq!(load(&store), LocalePreference::English);
    }

    #[test]
    fn ui_locale_persists_via_tempfile_store() {
        let _guard = locale_test_lock();
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("locale.sqlite3");
        let store = Store::open(&path).expect("store on disk");
        save(&store, LocalePreference::German);
        drop(store);
        let store = Store::open(&path).expect("reopen store");
        assert_eq!(load(&store), LocalePreference::German);
    }

    #[test]
    fn ui_locale_unknown_preference_falls_back_to_auto() {
        let _guard = locale_test_lock();
        let store = Store::open_in_memory().expect("in-memory store");
        let _ = store.set_meta(META_KEY, "preference=fr");
        assert_eq!(load(&store), LocalePreference::Auto);
    }

    #[test]
    fn ui_locale_tr_resolves_catalog_in_en_and_de() {
        let _guard = locale_test_lock();
        apply_locale("en");
        assert_eq!(tr("common.cancel"), "Cancel");
        apply_locale("de");
        assert_eq!(tr("common.cancel"), "Abbrechen");
    }

    #[test]
    fn ui_locale_tr_args_substitutes_detail() {
        let _guard = locale_test_lock();
        apply_locale("en");
        let mut args = HashMap::new();
        args.insert("detail".to_owned(), "timeout".to_owned());
        assert_eq!(
            tr_args("notice.generic_detail", &args),
            "Something went wrong: timeout"
        );
    }

    #[test]
    fn ui_locale_notice_localizes_call_on_hold_in_de() {
        let _guard = locale_test_lock();
        apply_locale("de");
        let localized = localize_notice("calls.notice.on_hold");
        assert_eq!(localized, "Setzen Sie den Anruf zuerst fort");
    }

    #[test]
    fn ui_locale_sidebar_and_status_menu_in_de() {
        let _guard = locale_test_lock();
        apply_locale("de");
        assert_eq!(tr("sidebar.quick_views"), "Schnellansichten");
        assert_eq!(tr("status_menu.view_account"), "Konto anzeigen");
        assert_eq!(tr("shell.settings_and_more"), "Einstellungen und mehr");
    }

    #[test]
    fn ui_locale_auth_banner_and_splash_keys() {
        let _guard = locale_test_lock();
        apply_locale("en");
        assert_eq!(tr("auth.banner.connecting"), "Connecting...");
        assert_ne!(tr("auth.banner.connecting"), "auth.banner.connecting");
        apply_locale("de");
        assert!(tr("auth.banner.connecting").contains("Verbindung"));
        assert!(tr("auth.splash.awaiting_browser").contains("Browser"));
    }

    #[test]
    fn ui_locale_tray_keys_resolve() {
        let _guard = locale_test_lock();
        apply_locale("en");
        assert_eq!(tr("tray.open"), "Open");
        assert_eq!(tr("tray.quit"), "Quit");
        assert_eq!(tr("tray.mute_sound"), "Mute sound");
        apply_locale("de");
        assert_eq!(tr("tray.do_not_disturb"), "Nicht stören");
        assert_eq!(tr("tray.notifications"), "Benachrichtigungen");
    }

    #[test]
    fn ui_locale_fallback_missing_key_returns_english_or_key() {
        let _guard = locale_test_lock();
        apply_locale("de");
        assert_eq!(tr("common.cancel"), "Abbrechen");
        let missing = tr("catalog.does_not_exist_yet");
        assert!(!missing.is_empty());
    }
}
