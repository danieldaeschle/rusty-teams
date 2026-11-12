use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_notice::{RecapActions, notice_row};
use super::message_row::RowAction;
use super::widgets::symbol;
use crate::data::Directory;
use crate::rows::{CallCard, CallIcon, CallParticipant, MessageRow};
use crate::theme;

const CARD_RADIUS: f32 = 12.;
const CARD_MIN_WIDTH: f32 = 200.;
const ICON_CIRCLE: f32 = 30.;
const ICON_SIZE: f32 = 18.;
const TITLE_SIZE: f32 = 13.;
const DETAIL_SIZE: f32 = 11.;
const JOIN_HEIGHT: f32 = 24.;
const PARTICIPANT_AVATAR: f32 = 20.;
const PARTICIPANT_GAP: f32 = 2.;

pub struct CallActions {
    pub join: RowAction,
    pub recap: RecapActions,
}

impl CallActions {
    pub fn none() -> Self {
        CallActions {
            join: None,
            recap: RecapActions {
                view_recap: None,
                open_recording: None,
                transcript: None,
                share: None,
            },
        }
    }
}

fn icon_circle(call: &CallCard) -> Div {
    let (fill, glyph, tint) = match call.icon {
        CallIcon::Missed => (theme::red().opacity(0.2), "call_missed", theme::red_soft()),
        CallIcon::Call => (theme::popup(), "call", theme::text()),
        CallIcon::Video => (theme::popup(), "videocam", theme::text()),
        CallIcon::Record | CallIcon::Saved => {
            (theme::popup(), "radio_button_checked", theme::accent_text())
        }
        CallIcon::RecordStop => (theme::popup(), "stop_circle", theme::text()),
        CallIcon::Roster => (theme::popup(), "group", theme::text()),
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

fn detail_line(call: &CallCard) -> Div {
    div()
        .text_size(px(DETAIL_SIZE))
        .text_color(theme::text_muted())
        .child(call.detail.clone())
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

fn participant_stack(
    participants: &[CallParticipant],
    extra: usize,
    directory: &Directory,
) -> Option<Div> {
    if participants.is_empty() {
        return None;
    }
    let tint = theme::text_muted();
    Some(
        h_flex()
            .pt(px(4.))
            .gap(px(PARTICIPANT_GAP))
            .items_center()
            .children(participants.iter().map(|person| {
                person_avatar(
                    directory,
                    Some(&person.user_id),
                    &person.name,
                    PARTICIPANT_AVATAR,
                )
            }))
            .when(extra > 0, |stack| {
                stack.child(
                    div()
                        .pl(px(4.))
                        .text_size(px(DETAIL_SIZE))
                        .text_color(tint)
                        .child(format!("+{extra}")),
                )
            }),
    )
}

fn card(call: &CallCard, index: usize, actions: CallActions, directory: &Directory) -> Div {
    let CallActions { join, .. } = actions;
    let show_join = call.started && join.is_some();
    let summary = h_flex()
        .gap(px(8.))
        .items_center()
        .child(icon_circle(call))
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
                        .child(call.title.clone()),
                )
                .child(detail_line(call))
                .children(participant_stack(
                    &call.participants,
                    call.extra_participants,
                    directory,
                )),
        )
        .when(show_join, |card| card.child(join_button(index, join)));
    v_flex()
        .min_w(px(CARD_MIN_WIDTH))
        .gap(px(4.))
        .pt(px(8.))
        .pr(px(12.))
        .pb(px(8.))
        .pl(px(8.))
        .rounded(px(CARD_RADIUS))
        .bg(theme::bubble_other())
        .child(summary)
}

pub(super) fn post_call_card(
    call: &CallCard,
    index: usize,
    actions: CallActions,
    directory: &Directory,
) -> Div {
    h_flex().child(card(call, index, actions, directory))
}

pub(super) fn call_card_row(
    row: &MessageRow,
    call: &CallCard,
    index: usize,
    directory: &Directory,
    actions: CallActions,
) -> AnyElement {
    notice_row(row, call, index, directory, actions.recap)
}
