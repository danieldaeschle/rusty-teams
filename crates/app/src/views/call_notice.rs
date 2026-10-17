use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::message_row::{AVATAR_GAP, AVATAR_SIZE, RowAction, day_separator, new_marker};
use super::widgets::symbol;
use crate::data::Directory;
use crate::rows::{CallCard, CallIcon, CallNotice, MessageRow, RecapCard, RecordingCard};
use crate::theme;

const ROW_TOP: f32 = 8.;
const LINE_HEIGHT: f32 = 20.;
const TEXT_SIZE: f32 = 12.;
const ICON_SIZE: f32 = 20.;
const STAMP_GAP: &str = "\u{2003}";
const CARD_TOP: f32 = 14.;
const CARD_MAX_WIDTH: f32 = 646.;
const CARD_RADIUS: f32 = 8.;
const CARD_PADDING_X: f32 = 20.;
const CARD_PADDING_TOP: f32 = 14.;
const TITLE_SIZE: f32 = 16.;
const BUTTON_HEIGHT: f32 = 32.;
const BUTTON_RADIUS: f32 = 4.;
const VIEW_RECAP_WIDTH: f32 = 108.;
const VIEW_RECAP_SIZE: f32 = 14.;
const DIVIDER_GAP: f32 = 12.;
const CONTENT_GAP_TOP: f32 = 12.;
const CONTENT_GAP_BOTTOM: f32 = 16.;
const CHIP_MIN_WIDTH: f32 = 96.;
const CHIP_ICON_GAP: f32 = 6.;
const THUMB_WIDTH: f32 = 108.;
const THUMB_HEIGHT: f32 = 60.;
const THUMB_RADIUS: f32 = 6.;
const BADGE_INSET_RIGHT: f32 = 6.;
const BADGE_INSET_BOTTOM: f32 = 8.;
const EXPIRY_SIZE: f32 = 10.;
const EXPIRY_LINE: f32 = 24.;
const EXPIRY_ICON_GAP: f32 = 4.;
const CONTENT_LABEL: &str = "Content";

pub struct RecapActions {
    pub view_recap: RowAction,
    pub open_recording: RowAction,
    pub transcript: RowAction,
    pub share: RowAction,
}

fn notice_icon(icon: CallIcon) -> (&'static str, Hsla) {
    match icon {
        CallIcon::Record => ("fluent_record", theme::accent_text()),
        CallIcon::RecordStop => ("fluent_record_stop", theme::text_muted()),
        CallIcon::Saved => ("fluent_cloud_arrow_up", theme::text_muted()),
        CallIcon::Roster => ("group", theme::text_muted()),
        CallIcon::Video | CallIcon::Call | CallIcon::Missed => {
            ("fluent_video", theme::text_muted())
        }
    }
}

fn notice_text(notice: &CallNotice) -> StyledText {
    let mut text = format!("{}{STAMP_GAP}{}", notice.stamp, notice.text);
    let mut highlights = Vec::new();
    if let Some(emphasis) = &notice.emphasis {
        text.push('\u{a0}');
        let start = text.len();
        text.push_str(emphasis);
        highlights.push((
            start..text.len(),
            HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                ..HighlightStyle::default()
            },
        ));
    }
    StyledText::new(text).with_highlights(highlights)
}

fn plain_button(id: String, action: RowAction) -> Stateful<Div> {
    h_flex()
        .id(ElementId::Name(id.into()))
        .h(px(BUTTON_HEIGHT))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .when_some(action, |button, action| {
            button.on_click(move |_, _, cx| action(cx))
        })
}

fn recap_header(recap: &RecapCard, index: usize, open: RowAction, share: RowAction) -> Div {
    h_flex()
        .w_full()
        .items_start()
        .gap(px(4.))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .line_height(px(LINE_HEIGHT))
                .child(
                    div()
                        .truncate()
                        .text_size(px(TITLE_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(recap.title.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_SIZE))
                        .text_color(theme::text_muted())
                        .child(recap.date_line.clone()),
                ),
        )
        .child(
            h_flex()
                .pt(px(2.))
                .gap(px(4.))
                .child(
                    plain_button(format!("recap-share-{index}"), share)
                        .w(px(BUTTON_HEIGHT))
                        .hover(|button| button.bg(theme::popup_hover()))
                        .child(symbol("fluent_share", ICON_SIZE, theme::text_soft())),
                )
                .child(
                    plain_button(format!("recap-view-{index}"), open)
                        .w(px(VIEW_RECAP_WIDTH))
                        .bg(theme::accent())
                        .hover(|button| button.opacity(0.85))
                        .text_size(px(VIEW_RECAP_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::white())
                        .child("View recap"),
                ),
        )
}

fn divider() -> Div {
    div()
        .w_full()
        .py(px(DIVIDER_GAP))
        .child(div().w_full().h(px(1.)).bg(theme::border_strong()))
}

fn transcript_chip(index: usize, transcript: RowAction) -> Stateful<Div> {
    plain_button(format!("recap-transcript-{index}"), transcript)
        .min_w(px(CHIP_MIN_WIDTH))
        .px(px(8.))
        .gap(px(CHIP_ICON_GAP))
        .border_1()
        .border_color(theme::border_strong())
        .bg(theme::bubble_other())
        .hover(|chip| chip.bg(theme::popup_hover()))
        .text_size(px(TEXT_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .child(symbol(
            "fluent_document_text",
            ICON_SIZE,
            theme::accent_text(),
        ))
        .child("Transcript")
}

fn thumbnail_image(recording: &RecordingCard, directory: &Directory) -> AnyElement {
    let image = recording
        .thumbnail
        .as_ref()
        .and_then(|thumbnail| directory.image(&thumbnail.url));
    match image {
        Some(entry) => img(entry.path.clone())
            .size_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => h_flex()
            .size_full()
            .items_center()
            .justify_center()
            .bg(theme::code_surface())
            .when(recording.duration.is_none(), |tile| {
                tile.child(symbol("fluent_video", ICON_SIZE, theme::text()))
            })
            .into_any_element(),
    }
}

fn recording_thumbnail(
    recording: &RecordingCard,
    index: usize,
    directory: &Directory,
    open: RowAction,
) -> Div {
    v_flex()
        .w(px(THUMB_WIDTH))
        .flex_none()
        .child(
            div()
                .id(ElementId::Name(format!("recap-recording-{index}").into()))
                .relative()
                .w(px(THUMB_WIDTH))
                .h(px(THUMB_HEIGHT))
                .rounded(px(THUMB_RADIUS))
                .overflow_hidden()
                .cursor_pointer()
                .hover(|tile| tile.opacity(0.92))
                .child(thumbnail_image(recording, directory))
                .children(recording.duration.clone().map(|duration| {
                    div()
                        .absolute()
                        .right(px(BADGE_INSET_RIGHT))
                        .bottom(px(BADGE_INSET_BOTTOM))
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(2.))
                        .bg(black().opacity(0.5))
                        .text_size(px(TEXT_SIZE))
                        .line_height(px(16.))
                        .text_color(theme::white())
                        .child(duration)
                }))
                .when_some(open, |tile, open| tile.on_click(move |_, _, cx| open(cx))),
        )
        .children(recording.expires_in_days.map(|days| {
            h_flex()
                .h(px(EXPIRY_LINE))
                .justify_center()
                .items_center()
                .gap(px(EXPIRY_ICON_GAP))
                .text_size(px(EXPIRY_SIZE))
                .text_color(theme::text_muted())
                .child(symbol("fluent_clock", EXPIRY_SIZE, theme::text_muted()))
                .child(if days == 1 {
                    "Expires in 1 day".to_owned()
                } else {
                    format!("Expires in {days} days")
                })
        }))
}

fn recap_card(
    recap: &RecapCard,
    index: usize,
    directory: &Directory,
    actions: RecapActions,
) -> Div {
    let RecapActions {
        view_recap,
        open_recording,
        transcript,
        share,
    } = actions;
    v_flex()
        .mt(px(CARD_TOP))
        .w_full()
        .max_w(px(CARD_MAX_WIDTH))
        .pt(px(CARD_PADDING_TOP))
        .px(px(CARD_PADDING_X))
        .rounded(px(CARD_RADIUS))
        .bg(theme::bubble_other())
        .shadow(card_shadow())
        .child(recap_header(recap, index, view_recap, share))
        .child(divider())
        .child(
            h_flex()
                .w_full()
                .items_start()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(TEXT_SIZE))
                                .line_height(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text_soft())
                                .child(CONTENT_LABEL),
                        )
                        .child(
                            h_flex()
                                .mt(px(CONTENT_GAP_TOP))
                                .mb(px(CONTENT_GAP_BOTTOM))
                                .when(recap.recording.transcript, |chips| {
                                    chips.child(transcript_chip(index, transcript))
                                }),
                        ),
                )
                .child(recording_thumbnail(
                    &recap.recording,
                    index,
                    directory,
                    open_recording,
                )),
        )
}

fn card_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., 0.24),
            offset: point(px(0.), px(0.)),
            blur_radius: px(1.),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: hsla(0., 0., 0., 0.28),
            offset: point(px(0.), px(2.)),
            blur_radius: px(2.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

pub(super) fn notice_line(
    call: &CallCard,
    notice: &CallNotice,
    index: usize,
    directory: &Directory,
    actions: RecapActions,
) -> Div {
    let (glyph, tint) = notice_icon(call.icon);
    h_flex()
        .w_full()
        .pt(px(ROW_TOP))
        .items_start()
        .gap(px(AVATAR_GAP))
        .child(
            h_flex()
                .w(px(AVATAR_SIZE))
                .h(px(LINE_HEIGHT))
                .flex_none()
                .justify_center()
                .items_center()
                .child(symbol(glyph, ICON_SIZE, tint)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(TEXT_SIZE))
                        .line_height(px(LINE_HEIGHT))
                        .text_color(theme::text_muted())
                        .child(notice_text(notice)),
                )
                .children(
                    call.recap
                        .as_ref()
                        .map(|recap| recap_card(recap, index, directory, actions)),
                ),
        )
}

pub(super) fn notice_row(
    row: &MessageRow,
    call: &CallCard,
    notice: &CallNotice,
    index: usize,
    directory: &Directory,
    actions: RecapActions,
) -> AnyElement {
    v_flex()
        .w_full()
        .px(px(20.))
        .children(row.day_header.as_deref().map(day_separator))
        .when(row.new_marker, |column| column.child(new_marker()))
        .child(
            div()
                .id(ElementId::Name(format!("row-{index}").into()))
                .w_full()
                .child(notice_line(call, notice, index, directory, actions)),
        )
        .into_any_element()
}
