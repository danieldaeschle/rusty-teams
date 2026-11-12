pub use app_base::ui_locale::{tr, tr_args};

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use app_base::ui_locale::{apply_locale, tr};

    fn locale_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn ui_locale_settings_common_actions() {
        let _guard = locale_test_lock();
        apply_locale("de");
        assert_eq!(tr("common.cancel"), "Abbrechen");
        apply_locale("en");
        assert_eq!(tr("settings.nav.general"), "General");
    }

    #[test]
    fn ui_locale_chrome_rail_and_nav_keys() {
        let _guard = locale_test_lock();
        apply_locale("en");
        assert_eq!(tr("rail.activity"), "Activity");
        assert_eq!(tr("rail.chat"), "Chat");
        assert_eq!(tr("settings.nav.notifications"), "Notifications and activity");
        apply_locale("de");
        assert_eq!(tr("rail.calendar"), "Kalender");
        assert_eq!(tr("common.save"), "Speichern");
    }

    #[test]
    fn ui_locale_calls_page_keys_resolve_in_de() {
        let _guard = locale_test_lock();
        apply_locale("de");
        assert_eq!(tr("calls.page.title"), "Anrufe");
        assert_eq!(tr("auth.sign_in"), "Anmelden");
    }

    #[test]
    fn ui_locale_chat_sidebar_and_filters_in_de() {
        let _guard = locale_test_lock();
        apply_locale("de");
        assert_eq!(tr("sidebar.pinned"), "Angepinnt");
        assert_eq!(tr("sidebar.filter.no_matches"), "Keine Treffer gefunden.");
        assert_eq!(tr("sidebar.pill.meetings"), "Besprechungschats");
    }

    #[test]
    fn ui_locale_call_chrome_in_de() {
        let _guard = locale_test_lock();
        apply_locale("de");
        assert_eq!(tr("calls.chrome.stop_sharing"), "Freigabe beenden");
        assert_eq!(tr("calls.prejoin.join_now"), "Jetzt beitreten");
    }

    #[test]
    fn ui_locale_calendar_meeting_dialog_in_de() {
        let _guard = locale_test_lock();
        apply_locale("de");
        assert_eq!(tr("calls.meeting.new_title"), "Neue Besprechung");
        assert_eq!(tr("calls.meeting.teams_link_added"), "Teams-Besprechungslink wird hinzugefügt");
    }
}
