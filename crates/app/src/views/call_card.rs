use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::message_row::{AVATAR_GAP, AVATAR_SIZE, RowAction, day_separator, new_marker};
use super::profile_card::opens_profile;
use super::widgets::symbol;
use crate::data::Directory;
use crate::rows::{CallCard, CallIcon, CallParticipant, MessageRow, RecordingCard};
use crate::theme;

const SERIES_START_GAP: f32 = 8.;
const CARD_RADIUS: f32 = 12.;
const CARD_MIN_WIDTH: f32 = 200.;
const ICON_CIRCLE: f32 = 30.;
const ICON_SIZE: f32 = 18.;
const TITLE_SIZE: f32 = 13.;
const DETAIL_SIZE: f32 = 11.;
const JOIN_HEIGHT: f32 = 24.;
const PARTICIPANT_AVATAR: f32 = 20.;
const PARTICIPANT_GAP: f32 = 2.;
const TILE_WIDTH: f32 = 288.;
const TILE_HEIGHT: f32 = 162.;
const TILE_RADIUS: f32 = 8.;
const PLAY_CIRCLE: f32 = 40.;
const PLAY_GLYPH: f32 = 24.;
const DURATION_INSET: f32 = 6.;
const CHIP_HEIGHT: f32 = 26.;

pub struct CallActions {
    pub call_back: RowAction,
    pub join: RowAction,
    pub open_recording: RowAction,
    pub open_transcript: RowAction,
}

impl CallActions {
    pub fn none() -> Self {
        CallActions {
            call_back: None,
            join: None,
            open_recording: None,
            open_transcript: None,
        }
    }
}

fn icon_circle(call: &CallCard, own: bool) -> Div {
    let (fill, glyph, tint) = match call.icon {
        CallIcon::Missed => (theme::red().opacity(0.2), "call_missed", theme::red_soft()),
        CallIcon::Call => (circle_fill(own), "call", theme::text()),
        CallIcon::Video => (circle_fill(own), "videocam", theme::text()),
        CallIcon::Record => (
            circle_fill(own),
            "radio_button_checked",
            theme::accent_text(),
        ),
        CallIcon::RecordStop => (circle_fill(own), "stop_circle", theme::text()),
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

fn participant_stack(
    participants: &[CallParticipant],
    extra: usize,
    directory: &Directory,
    own: bool,
) -> Option<Div> {
    if participants.is_empty() {
        return None;
    }
    let tint = if own {
        theme::own_meta()
    } else {
        theme::text_muted()
    };
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

fn recording_thumbnail(recording: &RecordingCard, directory: &Directory) -> AnyElement {
    let image = recording
        .thumbnail
        .as_ref()
        .and_then(|thumbnail| directory.image(&thumbnail.url));
    match image {
        Some(entry) => img(entry.path.clone())
            .size_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .size_full()
            .bg(theme::code_surface())
            .into_any_element(),
    }
}

fn recording_tile(
    recording: &RecordingCard,
    index: usize,
    directory: &Directory,
    open_recording: RowAction,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("call-recording-{index}").into()))
        .relative()
        .w(px(TILE_WIDTH))
        .h(px(TILE_HEIGHT))
        .flex_none()
        .rounded(px(TILE_RADIUS))
        .overflow_hidden()
        .cursor_pointer()
        .hover(|tile| tile.opacity(0.92))
        .child(recording_thumbnail(recording, directory))
        .child(
            h_flex()
                .absolute()
                .inset_0()
                .items_center()
                .justify_center()
                .child(
                    h_flex()
                        .size(px(PLAY_CIRCLE))
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(black().opacity(0.55))
                        .child(symbol("play_arrow", PLAY_GLYPH, theme::white())),
                ),
        )
        .children(recording.duration.clone().map(|duration| {
            div()
                .absolute()
                .bottom(px(DURATION_INSET))
                .right(px(DURATION_INSET))
                .px(px(6.))
                .py(px(1.))
                .rounded(px(4.))
                .bg(black().opacity(0.65))
                .text_size(px(DETAIL_SIZE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::white())
                .child(duration)
        }))
        .when_some(open_recording, |tile, open| {
            tile.on_click(move |_, _, cx| open(cx))
        })
}

fn transcript_chip(index: usize, open_transcript: RowAction) -> Stateful<Div> {
    h_flex()
        .id(ElementId::Name(format!("call-transcript-{index}").into()))
        .h(px(CHIP_HEIGHT))
        .px(px(10.))
        .gap(px(6.))
        .flex_none()
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(theme::border())
        .text_size(px(DETAIL_SIZE + 1.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .cursor_pointer()
        .hover(|chip| chip.bg(theme::popup_hover()))
        .child(symbol("description", 15., theme::accent_text()))
        .child("Transcript")
        .when_some(open_transcript, |chip, open| {
            chip.on_click(move |_, _, cx| open(cx))
        })
}

fn recording_block(
    recording: &RecordingCard,
    index: usize,
    directory: &Directory,
    open_recording: RowAction,
    open_transcript: RowAction,
) -> Div {
    v_flex()
        .pt(px(4.))
        .gap(px(8.))
        .child(recording_tile(recording, index, directory, open_recording))
        .child(
            h_flex()
                .w(px(TILE_WIDTH))
                .gap(px(8.))
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(TITLE_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(recording.title.clone()),
                )
                .when(recording.transcript, |line| {
                    line.child(transcript_chip(index, open_transcript))
                }),
        )
}

fn card(
    call: &CallCard,
    index: usize,
    own: bool,
    actions: CallActions,
    directory: &Directory,
) -> Div {
    let CallActions {
        call_back,
        join,
        open_recording,
        open_transcript,
    } = actions;
    let show_join = call.started && join.is_some();
    let summary = h_flex()
        .gap(px(8.))
        .items_center()
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
                        .child(call.title.clone()),
                )
                .child(detail_line(call, index, own, call_back))
                .children(participant_stack(
                    &call.participants,
                    call.extra_participants,
                    directory,
                    own,
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
        .bg(if own {
            theme::bubble_own()
        } else {
            theme::bubble_other()
        })
        .child(summary)
        .children(call.recording.as_ref().map(|recording| {
            recording_block(recording, index, directory, open_recording, open_transcript)
        }))
}

pub(super) fn post_call_card(
    call: &CallCard,
    index: usize,
    actions: CallActions,
    directory: &Directory,
) -> Div {
    h_flex().child(card(call, index, false, actions, directory))
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
            .child(card(call, index, true, actions, directory))
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
            .child(card(call, index, false, actions, directory))
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
