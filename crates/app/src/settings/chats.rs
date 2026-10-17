use gpui_kit::component::v_flex;
use gpui_kit::*;
use teams_core::ChatSection;

use super::rows::{caption, toggle_row};
use crate::app_state::AppState;
use crate::theme;

pub fn render(app: &Entity<AppState>, cx: &App) -> Div {
    let enabled = app
        .read(cx)
        .directory
        .section_settings
        .enabled(ChatSection::Muted);
    let toggle_app = app.clone();
    v_flex().w_full().child(caption("CHAT LIST")).child(
        toggle_row(
            "settings-muted-section",
            "Show muted items in their own section",
            "Read from your Teams account",
            theme::text_muted(),
            enabled,
        )
        .on_click(move |_, _, cx| {
            toggle_app.update(cx, |state, cx| {
                state.set_chat_section(ChatSection::Muted, !enabled, cx)
            })
        }),
    )
}
