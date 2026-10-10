use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::call_view::{leave_button, mute_button, status_line};
use super::widgets::icon;
use crate::app_state::AppState;
use crate::theme;

pub const MINI_WIDTH: f32 = 220.;
const MINI_RIGHT: f32 = 16.;
const MINI_BOTTOM: f32 = 40.;
const MUTE_SIZE: f32 = 28.;

pub fn render_call_mini(app: &Entity<AppState>, state: &AppState) -> Option<AnyElement> {
    let call = state.call.as_ref().filter(|call| !call.viewing)?;
    let model = &call.model;
    let (status, status_color) = status_line(model, Instant::now());
    let open_app = app.clone();
    let speaking = model.remote_speaking;
    Some(
        v_flex()
            .id("call-mini")
            .occlude()
            .absolute()
            .right(px(MINI_RIGHT))
            .bottom(px(MINI_BOTTOM))
            .w(px(MINI_WIDTH))
            .p(px(10.))
            .gap(px(8.))
            .rounded(px(10.))
            .bg(theme::surface_raised())
            .border_1()
            .border_color(if speaking {
                theme::green()
            } else {
                theme::border_strong()
            })
            .shadow_lg()
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                open_app.update(cx, |state, cx| state.show_call(true, cx));
            })
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(icon(IconName::Phone, 14., theme::accent_text()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(model.title.clone()),
                    )
                    .child(div().text_size(px(12.)).text_color(status_color).child(status)),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .justify_between()
                    .child(mute_button(app, model, "call-mini-mute", MUTE_SIZE))
                    .child(leave_button(app, model, "call-mini-leave", true)),
            )
            .into_any_element(),
    )
}
