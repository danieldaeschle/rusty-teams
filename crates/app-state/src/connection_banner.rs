use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use gpui_kit::*;

use crate::app_state::AppState;
use crate::backend::{ConnectionState, LiveState};

pub const SHOW_AFTER: Duration = Duration::from_secs(4);
pub const MIN_VISIBLE: Duration = Duration::from_secs(3);
const OFFLINE_TEXT: &str =
    "Something went wrong while connecting to Teams. Reconnect to keep the conversation going.";
const DEMO_OFFLINE_VARIABLE: &str = "TEAMS_DEMO_OFFLINE";
const DEMO_LAST_SYNC_AGO: chrono::Duration = chrono::Duration::minutes(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    Connecting,
    Reconnecting,
    Offline,
    SignIn,
    LiveUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerText {
    pub text: String,
    pub detail: Option<String>,
    pub retry: bool,
}

/// What is wrong right now; `None` while live or while only missed messages are being synced.
pub fn banner_kind(connection: &ConnectionState, live: LiveState) -> Option<BannerKind> {
    match connection {
        ConnectionState::Connecting => return Some(BannerKind::Connecting),
        ConnectionState::LoginRequired => return Some(BannerKind::SignIn),
        ConnectionState::NoBrowser | ConnectionState::NoAppTab | ConnectionState::Failed(_) => {
            return Some(BannerKind::Offline);
        }
        ConnectionState::Online => {}
    }
    match live {
        LiveState::Off | LiveState::Live | LiveState::MessageLoss => None,
        LiveState::Connecting => Some(BannerKind::Connecting),
        LiveState::Reconnecting => Some(BannerKind::Reconnecting),
        LiveState::Failed => Some(BannerKind::LiveUnavailable),
    }
}

pub fn banner_text(kind: BannerKind, last_sync: Option<DateTime<Utc>>) -> BannerText {
    let text = |text: &str| BannerText {
        text: text.to_owned(),
        detail: None,
        retry: false,
    };
    match kind {
        BannerKind::Connecting => text("Connecting..."),
        BannerKind::Reconnecting => text("Reconnecting..."),
        BannerKind::Offline => BannerText {
            detail: last_sync
                .map(|time| format!("Last synced {}", time.with_timezone(&Local).format("%H:%M"))),
            ..text(OFFLINE_TEXT)
        },
        BannerKind::SignIn => text("Sign-in required. Sign in to Teams in the window that opened."),
        BannerKind::LiveUnavailable => BannerText {
            retry: true,
            ..text("Live updates unavailable")
        },
    }
}

/// Shows a bad state only once it lasted `SHOW_AFTER`, and keeps a shown banner at least `MIN_VISIBLE`.
#[derive(Debug, Default)]
pub struct BannerTiming {
    bad_since: Option<Instant>,
    shown: Option<(BannerKind, Instant)>,
}

impl BannerTiming {
    pub fn visible(&self) -> Option<BannerKind> {
        self.shown.map(|(kind, _)| kind)
    }

    /// Returns when to check again, if the visible banner may change without a new state.
    pub fn update(&mut self, wanted: Option<BannerKind>, now: Instant) -> Option<Duration> {
        match wanted {
            Some(kind) => {
                let bad_since = *self.bad_since.get_or_insert(now);
                if let Some((shown_kind, _)) = self.shown.as_mut() {
                    *shown_kind = kind;
                    return None;
                }
                let waited = now.duration_since(bad_since);
                if waited >= SHOW_AFTER {
                    self.shown = Some((kind, now));
                    return None;
                }
                Some(SHOW_AFTER - waited)
            }
            None => {
                self.bad_since = None;
                let (_, shown_at) = self.shown?;
                let visible_for = now.duration_since(shown_at);
                if visible_for >= MIN_VISIBLE {
                    self.shown = None;
                    return None;
                }
                Some(MIN_VISIBLE - visible_for)
            }
        }
    }
}

impl AppState {
    pub fn refresh_connection_banner(&mut self, cx: &mut Context<Self>) {
        let wanted = if self.mode.demo {
            let offline = std::env::var_os(DEMO_OFFLINE_VARIABLE).is_some();
            if offline {
                self.last_sync
                    .get_or_insert_with(|| Utc::now() - DEMO_LAST_SYNC_AGO);
            }
            offline.then_some(BannerKind::Offline)
        } else {
            banner_kind(&self.connection, self.live)
        };
        let before = self.connection_banner.visible();
        let recheck = self.connection_banner.update(wanted, Instant::now());
        if let Some(wait) = recheck {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |state, cx| state.refresh_connection_banner(cx))
                    .ok();
            })
            .detach();
        }
        if self.connection_banner.visible() != before {
            cx.emit(crate::app_state::AppEvent::Status);
            cx.notify();
        }
    }

    pub fn connection_banner_text(&self) -> Option<BannerText> {
        self.connection_banner
            .visible()
            .map(|kind| banner_text(kind, self.last_sync))
    }

    pub fn retry_live(&mut self) {
        if let Some(retry) = &self.live_retry {
            retry.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use chrono::Utc;

    use super::{BannerKind, BannerTiming, MIN_VISIBLE, SHOW_AFTER, banner_kind, banner_text};
    use crate::backend::{ConnectionState, LiveState};

    #[test]
    fn a_short_bad_state_never_shows() {
        let mut timing = BannerTiming::default();
        let start = Instant::now();
        assert_eq!(
            timing.update(Some(BannerKind::Reconnecting), start),
            Some(SHOW_AFTER)
        );
        assert_eq!(timing.update(None, start + Duration::from_secs(2)), None);
        assert_eq!(timing.visible(), None);
    }

    #[test]
    fn a_lasting_bad_state_shows_after_the_delay() {
        let mut timing = BannerTiming::default();
        let start = Instant::now();
        timing.update(Some(BannerKind::Reconnecting), start);
        let wait = timing.update(Some(BannerKind::Offline), start + Duration::from_secs(1));
        assert_eq!(wait, Some(SHOW_AFTER - Duration::from_secs(1)));
        assert_eq!(
            timing.update(Some(BannerKind::Offline), start + SHOW_AFTER),
            None
        );
        assert_eq!(timing.visible(), Some(BannerKind::Offline));
    }

    #[test]
    fn a_shown_banner_stays_for_the_minimum_then_goes() {
        let mut timing = BannerTiming::default();
        let start = Instant::now();
        timing.update(Some(BannerKind::Offline), start);
        let shown_at = start + SHOW_AFTER;
        timing.update(Some(BannerKind::Offline), shown_at);
        let back = shown_at + Duration::from_secs(1);
        assert_eq!(
            timing.update(None, back),
            Some(MIN_VISIBLE - Duration::from_secs(1))
        );
        assert_eq!(timing.visible(), Some(BannerKind::Offline));
        assert_eq!(timing.update(None, shown_at + MIN_VISIBLE), None);
        assert_eq!(timing.visible(), None);
    }

    #[test]
    fn a_new_bad_state_while_shown_changes_the_text_at_once() {
        let mut timing = BannerTiming::default();
        let start = Instant::now();
        timing.update(Some(BannerKind::Offline), start);
        timing.update(Some(BannerKind::Offline), start + SHOW_AFTER);
        timing.update(None, start + SHOW_AFTER);
        timing.update(Some(BannerKind::Reconnecting), start + SHOW_AFTER);
        assert_eq!(timing.visible(), Some(BannerKind::Reconnecting));
    }

    #[test]
    fn live_and_missed_messages_need_no_banner() {
        let online = ConnectionState::Online;
        assert_eq!(banner_kind(&online, LiveState::Live), None);
        assert_eq!(banner_kind(&online, LiveState::MessageLoss), None);
        assert_eq!(
            banner_kind(&online, LiveState::Failed),
            Some(BannerKind::LiveUnavailable)
        );
        assert_eq!(
            banner_kind(&ConnectionState::LoginRequired, LiveState::Live),
            Some(BannerKind::SignIn)
        );
        assert_eq!(
            banner_kind(&ConnectionState::Failed("x".into()), LiveState::Off),
            Some(BannerKind::Offline)
        );
    }

    #[test]
    fn the_offline_banner_names_the_last_sync() {
        let text = banner_text(BannerKind::Offline, Some(Utc::now()));
        assert!(text.detail.unwrap().starts_with("Last synced "));
        assert!(banner_text(BannerKind::Offline, None).detail.is_none());
        assert!(banner_text(BannerKind::LiveUnavailable, None).retry);
    }
}
