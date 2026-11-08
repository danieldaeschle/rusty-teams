use chrono::{DateTime, Local, Utc};
use gpui_kit::Context;
use store::Store;

use crate::app_state::AppState;
use crate::own_status::until_label;

const META_KEY: &str = "notify_paused_until";
pub const PAUSE_LABEL: &str = "Pause notifications";

pub fn load(store: &Store) -> Option<DateTime<Utc>> {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
        .map(|until| until.with_timezone(&Utc))
}

pub fn is_paused(until: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    until.is_some_and(|until| now < until)
}

pub fn menu_label(until: Option<DateTime<Utc>>, now: DateTime<Local>) -> String {
    match until.filter(|until| now.with_timezone(&Utc) < *until) {
        Some(until) => format!("Paused {}", until_label(until, &now)),
        None => PAUSE_LABEL.to_owned(),
    }
}

impl AppState {
    pub fn notifications_paused(&self, now: DateTime<Utc>) -> bool {
        is_paused(self.notifications_paused_until, now)
    }

    pub fn pause_notifications(&mut self, until: DateTime<Utc>, cx: &mut Context<Self>) {
        let _ = self.store.set_meta(META_KEY, &until.to_rfc3339());
        self.notifications_paused_until = Some(until);
        cx.notify();
    }

    pub fn resume_notifications(&mut self, cx: &mut Context<Self>) {
        let _ = self.store.set_meta(META_KEY, "");
        self.notifications_paused_until = None;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn a_pause_ends_at_its_time() {
        let until = Utc.with_ymd_and_hms(2026, 11, 8, 15, 0, 0).unwrap();
        let before = Utc.with_ymd_and_hms(2026, 11, 8, 14, 59, 0).unwrap();
        assert!(is_paused(Some(until), before));
        assert!(!is_paused(Some(until), until));
        assert!(!is_paused(None, before));
        let later = Local.from_utc_datetime(&until.naive_utc());
        assert_eq!(menu_label(Some(until), later), PAUSE_LABEL);
    }
}
