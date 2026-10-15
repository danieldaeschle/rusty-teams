use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::message_row::{AVATAR_GAP, AVATAR_SIZE, RowAction, day_separator, new_marker};
use super::profile_card::opens_profile;
use super::widgets::symbol;
use crate::data::Directory;
use crate::rows::{CallCard, CallIcon, MessageRow};
use crate::theme;

const SERIES_START_GAP: f32 = 8.;
const CARD_RADIUS: f32 = 12.;
const CARD_MIN_WIDTH: f32 = 200.;
const ICON_CIRCLE: f32 = 30.;
const ICON_SIZE: f32 = 18.;
const TITLE_SIZE: f32 = 13.;
const DETAIL_SIZE: f32 = 11.;
const JOIN_HEIGHT: f32 = 24.;

pub struct CallActions {
    pub call_back: RowAction,
    pub join: RowAction,
}

fn icon_circle(call: &CallCard, own: bool) -> Div {
    let (fill, glyph, tint) = match call.icon {
        CallIcon::Missed => (theme::red().opacity(0.2), "call_missed", theme::red_soft()),
        CallIcon::Call => (circle_fill(own), "call", theme::text()),
        CallIcon::Video => (circle_fill(own), "videocam", theme::text()),
    };
    h_flex()
        .size(px(ICON_CIRCLE))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(fill)
        .child(symbol(glyph, ICON_SIZE, tint))
}

fn circle_fill(own: bool) -> Hsla {
    if own {
        theme::reaction_on_own()
    } else {
        theme::popup()
    }
}

fn detail_line(call: &CallCard, index: usize, own: bool, call_back: RowAction) -> Div {
    let tint = if own {
        theme::own_meta()
    } else {
        theme::text_muted()
    };
    h_flex()
        .gap(px(4.))
        .items_center()
        .text_size(px(DETAIL_SIZE))
        .text_color(tint)
        .child(call.detail.clone())
        .when(call.missed, |line| {
            line.child("\u{b7}").child(
                div()
                    .id(ElementId::Name(format!("call-back-{index}").into()))
                    .cursor_pointer()
                    .text_color(theme::accent_text())
                    .child("Call back")
                    .when_some(call_back, |link, call_back| {
                        link.on_click(move |_, _, cx| call_back(cx))
                    }),
            )
        })
}

fn join_button(index: usize, join: RowAction) -> Stateful<Div> {
    h_flex()
        .id(ElementId::Name(format!("call-join-{index}").into()))
        .h(px(JOIN_HEIGHT))
        .px(px(12.))
        .flex_none()
        .items_center()
        .rounded(px(CARD_RADIUS))
        .bg(theme::accent())
        .text_color(theme::white())
        .text_size(px(DETAIL_SIZE + 1.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child("Join")
        .when_some(join, |button, join| {
            button.on_click(move |_, _, cx| join(cx))
        })
}

fn card(call: &CallCard, index: usize, own: bool, actions: CallActions) -> Div {
    let CallActions { call_back, join } = actions;
    let show_join = call.started && join.is_some();
    h_flex()
        .min_w(px(CARD_MIN_WIDTH))
        .gap(px(8.))
        .items_center()
        .pt(px(8.))
        .pr(px(12.))
        .pb(px(8.))
        .pl(px(8.))
        .rounded(px(CARD_RADIUS))
        .bg(if own {
            theme::bubble_own()
        } else {
            theme::bubble_other()
        })
        .child(icon_circle(call, own))
        .child(
            v_flex()
                .flex_1()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(TITLE_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if call.missed {
                            theme::red_soft()
                        } else {
                            theme::text()
                        })
                        .child(call.title),
                )
                .child(detail_line(call, index, own, call_back)),
        )
        .when(show_join, |card| card.child(join_button(index, join)))
}

pub(super) fn call_card_row(
    row: &MessageRow,
    call: &CallCard,
    index: usize,
    directory: &Directory,
    actions: CallActions,
) -> AnyElement {
    let own = row.own;
    let body = if own {
        h_flex()
            .w_full()
            .justify_end()
            .child(card(call, index, true, actions))
    } else {
        h_flex()
            .w_full()
            .gap(px(AVATAR_GAP))
            .items_start()
            .child(
                opens_profile(
                    div().id(ElementId::Name(format!("author-avatar-{index}").into())),
                    row.sender_id.as_deref(),
                )
                .w(px(AVATAR_SIZE))
                .flex_none()
                .child(person_avatar(
                    directory,
                    row.sender_id.as_deref(),
                    &row.author,
                    AVATAR_SIZE,
                )),
            )
            .child(card(call, index, false, actions))
    };
    v_flex()
        .w_full()
        .px(px(20.))
        .children(row.day_header.as_deref().map(day_separator))
        .when(row.new_marker, |column| column.child(new_marker()))
        .child(
            div()
                .id(ElementId::Name(format!("row-{index}").into()))
                .w_full()
                .pt(px(SERIES_START_GAP))
                .child(body),
        )
        .into_any_element()
}
