use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::adaptive_card::card_view;
use super::widgets::symbol;
use crate::app_state::{AppHandle, AppState};
use crate::card_state::TaskDialogState;
use crate::theme;

const BACKDROP_OPACITY: f32 = 0.55;
const PANEL_MARGIN: f32 = 32.;
const PANEL_RADIUS: f32 = 12.;
const PANEL_PADDING: f32 = 16.;
const HEADER_GAP: f32 = 12.;
const TITLE_SIZE: f32 = 15.;
const CLOSE_SIZE: f32 = 28.;
const CLOSE_ICON_SIZE: f32 = 16.;
const MIN_WIDTH: f32 = 360.;
const CARD_CHROME: f32 = 2. * 12. + 2.;

pub fn render_task_dialog(state: &AppState, cx: &App) -> Option<AnyElement> {
    let dialog = state.task_dialog.as_ref()?;
    Some(
        div()
            .id("task-dialog-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("task-dialog-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(|_, _, cx| close(cx)),
            )
            .child(panel(dialog, cx))
            .into_any_element(),
    )
}

fn panel(dialog: &TaskDialogState, cx: &App) -> Div {
    let width = (dialog.width as f32 + CARD_CHROME).max(MIN_WIDTH);
    v_flex()
        .w(px(width))
        .max_w(relative(1.))
        .max_h(relative(1.))
        .m(px(PANEL_MARGIN))
        .p(px(PANEL_PADDING))
        .gap(px(HEADER_GAP))
        .rounded(px(PANEL_RADIUS))
        .bg(theme::background())
        .border_1()
        .border_color(theme::border_strong())
        .text_color(theme::text())
        .occlude()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(HEADER_GAP))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(TITLE_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(dialog.title.clone()),
                )
                .child(
                    div()
                        .id("task-dialog-close")
                        .size(px(CLOSE_SIZE))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::row_hover()))
                        .on_click(|_, _, cx| close(cx))
                        .child(symbol("close", CLOSE_ICON_SIZE, theme::text_muted())),
                ),
        )
        .child(
            div()
                .id("task-dialog-body")
                .w_full()
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .child(card_view(&dialog.card, &dialog.scope, cx)),
        )
}

fn close(cx: &mut App) {
    cx.global::<AppHandle>()
        .0
        .clone()
        .update(cx, |state, cx| state.close_task_dialog(cx));
}
