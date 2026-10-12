use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::center::NotificationCenter;
use super::layout::{PREVIEW_MAX_LINES, PREVIEW_SIZE};
use super::stack::AgentToast;
use crate::theme;
use crate::views::widgets::{icon, symbol};

const BUTTON_HEIGHT: f32 = 30.;
const CLOSE_SIZE: f32 = 24.;

fn button(id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(BUTTON_HEIGHT))
        .px(px(12.))
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .when(primary, |button| {
            button
                .bg(theme::accent())
                .text_color(theme::on_accent())
                .hover(|button| button.bg(theme::accent_text()))
        })
        .when(!primary, |button| {
            button
                .bg(theme::popup())
                .text_color(theme::text_soft())
                .hover(|button| button.bg(theme::popup_hover()))
        })
        .child(label)
}

pub(super) fn agent_toast_content(
    center: &Entity<NotificationCenter>,
    id: u64,
    agent: &AgentToast,
    hovered: bool,
) -> Div {
    let (open_chat_center, follow_up_center, close_center) =
        (center.clone(), center.clone(), center.clone());
    let follow_up_label = if agent.failed {
        "Retry"
    } else {
        "Open session"
    };
    let tint = if agent.failed {
        theme::red_soft()
    } else {
        theme::accent_text()
    };
    v_flex()
        .w_full()
        .gap(px(8.))
        .child(
            h_flex()
                .w_full()
                .h(px(16.))
                .gap(px(8.))
                .items_center()
                .child(icon(IconName::Bot, 14., tint))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(agent.header.clone()),
                )
                .when(hovered, |row| {
                    row.child(
                        div()
                            .id("agent-toast-close")
                            .size(px(CLOSE_SIZE))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme::border_strong())
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::text_faint()))
                            .child(symbol("close", 16., theme::text()))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                close_center.update(cx, |center, cx| center.dismiss(id, cx));
                            }),
                    )
                }),
        )
        .child(
            div()
                .w_full()
                .h(px(20.))
                .truncate()
                .text_size(px(14.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(agent.title.clone()),
        )
        .child(
            div()
                .w_full()
                .text_size(px(PREVIEW_SIZE))
                .line_height(px(18.))
                .text_color(theme::text_soft())
                .line_clamp(PREVIEW_MAX_LINES)
                .child(agent.body.clone()),
        )
        .child(
            h_flex()
                .gap(px(6.))
                .child(button("agent-toast-open-chat", "Open chat", true).on_click(
                    move |_, _, cx| {
                        cx.stop_propagation();
                        open_chat_center.update(cx, |center, cx| center.activate(id, cx));
                    },
                ))
                .child(
                    button("agent-toast-follow-up", follow_up_label, false).on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            follow_up_center
                                .update(cx, |center, cx| center.agent_toast_follow_up(id, cx));
                        },
                    ),
                ),
        )
}
