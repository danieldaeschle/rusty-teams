use chrono::{DateTime, Local, Utc};
use gpui_kit::component::{h_flex, tooltip::Tooltip};
use gpui_kit::*;

use crate::app_state::{AppState, Mode};
use crate::format;
use crate::backend::{ConnectionState, LiveState};
use crate::theme;
use crate::updater::{self, UpdateStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Warn,
    Bad,
}

pub fn connection_text(connection: &ConnectionState, mode: Mode) -> (String, Tone) {
    if mode.demo {
        return ("Demo data".to_owned(), Tone::Warn);
    }
    match connection {
        ConnectionState::Connecting => ("Connecting".to_owned(), Tone::Warn),
        ConnectionState::Online => ("Connected".to_owned(), Tone::Good),
        ConnectionState::NoBrowser => ("Browser not running".to_owned(), Tone::Bad),
        ConnectionState::NoAppTab => ("Teams not loaded".to_owned(), Tone::Bad),
        ConnectionState::LoginRequired => ("Sign-in required".to_owned(), Tone::Bad),
        ConnectionState::Failed(message) => (format!("Error: {message}"), Tone::Bad),
    }
}

pub fn live_text(live: LiveState) -> Option<(&'static str, Tone)> {
    match live {
        LiveState::Off => None,
        LiveState::Connecting => Some(("Live: connecting", Tone::Warn)),
        LiveState::Live => Some(("Live", Tone::Good)),
        LiveState::Reconnecting => Some(("Live: reconnecting", Tone::Warn)),
        LiveState::MessageLoss => Some(("Live: missed messages, syncing", Tone::Warn)),
        LiveState::Failed => Some(("Live: unavailable", Tone::Bad)),
    }
}

pub fn sync_text(last_sync: Option<DateTime<Utc>>) -> String {
    match last_sync {
        Some(time) => format!("Synced {}", time.with_timezone(&Local).format("%H:%M")),
        None => "Not synced yet".to_owned(),
    }
}

fn tone_color(tone: Tone) -> Hsla {
    match tone {
        Tone::Good => theme::green(),
        Tone::Warn => theme::amber(),
        Tone::Bad => theme::red(),
    }
}

pub fn render_status_bar(
    state: &AppState,
    update: &UpdateStatus,
    on_restart: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (connection, connection_tone) = connection_text(&state.connection, state.mode);
    let item = |label: String, tone: Tone| {
        h_flex()
            .gap(px(6.))
            .items_center()
            .child(div().size(px(7.)).rounded_full().bg(tone_color(tone)))
            .child(label)
    };
    let live = live_text(state.live);
    let mut bar = h_flex()
        .w_full()
        .h(px(24.))
        .flex_none()
        .px(px(12.))
        .gap(px(14.))
        .items_center()
        .text_size(px(11.))
        .bg(theme::background())
        .border_t_1()
        .border_color(theme::border())
        .text_color(theme::text_muted());
    match live {
        Some((label, tone)) if connection_tone == Tone::Good => {
            bar = bar.child(item(label.to_owned(), tone))
        }
        _ => bar = bar.child(item(connection, connection_tone)),
    }
    bar = bar.child(div().child(sync_text(state.last_sync)));
    if state.mode.read_only {
        bar = bar.child(div().text_color(theme::amber()).child("Read-only"));
    }
    match update {
        UpdateStatus::UpToDate => {}
        UpdateStatus::Ready => {
            bar = bar.child(
                div()
                    .id("update-restart")
                    .px(px(8.))
                    .rounded(px(4.))
                    .bg(theme::accent())
                    .text_color(theme::on_accent())
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::accent_text()))
                    .child("Update ready - Restart")
                    .on_click(on_restart),
            )
        }
        UpdateStatus::Failed(message) => {
            bar = bar.child(
                div()
                    .text_color(theme::red())
                    .child(format!("Update failed: {message}")),
            )
        }
    }
    let full_version = updater::running_version();
    bar.child(
        div()
            .id("build-version")
            .ml_auto()
            .tooltip(move |window, cx| Tooltip::new(full_version).build(window, cx))
            .child(format::short_version(
                full_version,
                env!("CARGO_PKG_VERSION"),
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::{Tone, connection_text, live_text, sync_text};
    use crate::app_state::Mode;
    use crate::backend::{ConnectionState, LiveState};
    use chrono::Utc;

    #[test]
    fn states_map_to_text_and_tone() {
        let mode = Mode::default();
        assert_eq!(
            connection_text(&ConnectionState::Online, mode).1,
            Tone::Good
        );
        assert_eq!(
            connection_text(&ConnectionState::LoginRequired, mode).0,
            "Sign-in required"
        );
        assert_eq!(
            connection_text(&ConnectionState::Connecting, mode).1,
            Tone::Warn
        );
        assert!(
            connection_text(&ConnectionState::Failed("x".into()), mode)
                .0
                .contains('x')
        );
    }

    #[test]
    fn demo_mode_overrides_the_connection() {
        let mode = Mode {
            demo: true,
            read_only: false,
        };
        assert_eq!(
            connection_text(&ConnectionState::NoBrowser, mode).0,
            "Demo data"
        );
    }

    #[test]
    fn live_off_has_no_label() {
        assert_eq!(live_text(LiveState::Off), None);
        assert_eq!(live_text(LiveState::Live).unwrap().1, Tone::Good);
    }

    #[test]
    fn sync_text_without_sync() {
        assert_eq!(sync_text(None), "Not synced yet");
        assert!(sync_text(Some(Utc::now())).starts_with("Synced "));
    }
}
