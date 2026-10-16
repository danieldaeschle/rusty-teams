use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::end_screen::{CallEndScreen, DISMISS_LABEL};
use crate::theme;

const CONTENT_WIDTH: f32 = 400.;
const AVATAR_SIZE: f32 = 96.;
const ICON_SIZE: f32 = 40.;
const PANE_PADDING: f32 = 12.;
const SECTION_GAP: f32 = 16.;
const TEXT_GAP: f32 = 8.;
const BUTTON_GAP: f32 = 8.;

pub fn render_call_end(app: &Entity<AppState>, state: &AppState) -> Option<AnyElement> {
    let screen = state.call_end.as_ref()?;
    let dismiss_app = app.clone();
    let retry_app = app.clone();
    let actions = h_flex()
        .debug_selector(|| "shell/islands/main/call-end/actions".into())
        .flex_none()
        .gap(px(BUTTON_GAP))
        .justify_center()
        .child(
            Button::new("call-end-dismiss")
                .outline()
                .label(DISMISS_LABEL)
                .on_click(move |_, _, cx| {
                    dismiss_app.update(cx, |state, cx| state.dismiss_call_end(cx))
                }),
        )
        .when(screen.can_retry(), |row| {
            row.child(
                Button::new("call-end-retry")
                    .primary()
                    .label(screen.retry_label())
                    .on_click(move |_, _, cx| {
                        retry_app.update(cx, |state, cx| state.retry_call_end(cx))
                    }),
            )
        });
    Some(
        v_flex()
            .debug_selector(|| "shell/islands/main".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(PANE_PADDING))
            .items_center()
            .justify_center()
            .rounded(px(10.))
            .bg(theme::surface())
            .overflow_hidden()
            .child(
                v_flex()
                    .debug_selector(|| "shell/islands/main/call-end".into())
                    .w(px(CONTENT_WIDTH))
                    .max_w_full()
                    .items_center()
                    .gap(px(SECTION_GAP))
                    .child(badge(state, screen))
                    .child(
                        v_flex()
                            .w_full()
                            .items_center()
                            .gap(px(TEXT_GAP))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_strong())
                                    .text_center()
                                    .child(screen.heading()),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .text_size(px(14.))
                                    .text_color(theme::text_muted())
                                    .text_center()
                                    .child(screen.description()),
                            ),
                    )
                    .child(actions),
            )
            .into_any_element(),
    )
}

fn badge(state: &AppState, screen: &CallEndScreen) -> AnyElement {
    match &screen.peer {
        Some(peer) => person_avatar(
            &state.directory,
            peer.user_id.as_deref(),
            &peer.name,
            AVATAR_SIZE,
        ),
        None => div()
            .size(px(AVATAR_SIZE))
            .flex_none()
            .rounded_full()
            .bg(theme::surface_raised())
            .flex()
            .items_center()
            .justify_center()
            .child(icon(IconName::PhoneOff, ICON_SIZE, theme::text_muted()))
            .into_any_element(),
    }
}
