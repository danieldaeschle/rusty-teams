use gpui_kit::assets::IconName;
use gpui_kit::base::TextSelection;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

use super::attachments::attachments_view;
use super::avatar::{member_stack, person_avatar};
use super::widgets::{icon, symbol};
use crate::data::Directory;
use crate::render::{Block, render_blocks};
use crate::rows::{Delivery, MessageRow, ReactionChip, Receipt, Skeleton, bubble_corners};
use crate::sidebar_model::DELETED_PREVIEW;
use crate::theme;

pub type RowAction = Option<Box<dyn Fn(&mut App)>>;

const AVATAR_SIZE: f32 = 28.;
const AVATAR_GAP: f32 = 8.;
const SERIES_START_GAP: f32 = 8.;
const BODY_SIZE: f32 = 13.5;
const MAX_WIDTH_RATIO: f32 = 0.7;
const SENDING_OPACITY: f32 = 0.6;
const REACTION_OVERHANG: f32 = 12.;
const CARD_AVATAR_SIZE: f32 = 32.;
const CARD_GAP: f32 = 12.;
const META_SIZE: f32 = 11.;
const META_CHECK_SIZE: f32 = 15.;
const FIGURE_SPACE_WIDTH: f32 = 7.4;
const PULSE_PERIOD: Duration = Duration::from_millis(1600);
const SKELETON_LINE_HEIGHT: f32 = 20.;

pub struct RowActions {
    pub open_thread: RowAction,
    pub retry: RowAction,
    pub reply: RowAction,
    pub hovered: RowAction,
    pub highlighted: bool,
}

fn reaction_chip(chip: &ReactionChip) -> Div {
    let (background, border, foreground) = if chip.mine {
        (theme::bubble_own(), theme::accent(), theme::accent_tint())
    } else {
        (theme::surface(), theme::border_strong(), theme::text_soft())
    };
    h_flex()
        .h(px(22.))
        .px(px(8.))
        .gap(px(4.))
        .items_center()
        .rounded(px(11.))
        .bg(background)
        .border_1()
        .border_color(border)
        .text_size(px(12.))
        .text_color(foreground)
        .child(chip.label.clone())
        .child(chip.count.to_string())
}

fn labeled_divider(label: &str, line_color: Hsla, text_color: Hsla) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .gap(px(12.))
        .mt(px(14.))
        .mb(px(6.))
        .child(div().flex_1().h(px(1.)).bg(line_color))
        .child(
            div()
                .text_size(px(11.))
                .text_color(text_color)
                .child(label.to_owned()),
        )
        .child(div().flex_1().h(px(1.)).bg(line_color))
}

fn day_separator(label: &str) -> Div {
    labeled_divider(label, theme::border(), theme::text_muted())
}

fn new_marker() -> Div {
    labeled_divider("New", theme::accent(), theme::accent_text())
}

fn bubble(row: &MessageRow, index: usize, own: bool, directory: &Directory, cx: &App) -> Div {
    let corners = bubble_corners(own, row.series);
    let mut element = div()
        .relative()
        .rounded_tl(px(corners.top_left))
        .rounded_tr(px(corners.top_right))
        .rounded_br(px(corners.bottom_right))
        .rounded_bl(px(corners.bottom_left))
        .px(px(11.))
        .py(px(7.))
        .text_size(px(BODY_SIZE))
        .line_height(relative(1.45));
    if row.deleted {
        return element
            .border_1()
            .border_dashed()
            .border_color(theme::border_strong())
            .py(px(6.))
            .text_size(px(13.))
            .italic()
            .text_color(theme::text_muted())
            .child(DELETED_PREVIEW);
    }
    let failed = matches!(row.delivery, Delivery::Failed(_));
    element = element
        .bg(if own && !failed {
            theme::bubble_own()
        } else {
            theme::bubble_other()
        })
        .when(failed, |bubble| {
            bubble.border_1().border_color(theme::red())
        })
        .when(row.delivery == Delivery::Sending, |bubble| {
            bubble.opacity(SENDING_OPACITY)
        })
        .when(!row.reactions.is_empty(), |bubble| {
            bubble.mb(px(REACTION_OVERHANG))
        });
    let mut content = v_flex().gap(px(6.));
    let padded_blocks = with_meta_room(&row.blocks, meta_room(row, own));
    let meta_inline = has_text(row) && padded_blocks.is_some();
    if has_text(row) {
        content = content.child(render_blocks(
            padded_blocks.as_deref().unwrap_or(&row.blocks),
            &format!("message-{index}"),
            own,
            cx,
        ));
    }
    content = content.children(attachments_view(
        &row.images,
        &row.files,
        &format!("message-{index}"),
        directory,
    ));
    if !meta_inline {
        content = content.child(h_flex().justify_end().child(bubble_meta(row, own)));
    }
    element = element.child(content);
    if meta_inline {
        element = element.child(
            div()
                .absolute()
                .right(px(9.))
                .bottom(px(5.))
                .child(bubble_meta(row, own)),
        );
    }
    if !row.reactions.is_empty() {
        element = element.child(
            h_flex()
                .absolute()
                .when(own, |chips| chips.right(px(11.)))
                .when(!own, |chips| chips.left(px(11.)))
                .bottom(px(-14.))
                .gap(px(4.))
                .children(row.reactions.iter().map(reaction_chip)),
        );
    }
    element
}

fn bubble_meta(row: &MessageRow, own: bool) -> Div {
    let tint = if own {
        theme::own_meta()
    } else {
        theme::text_muted()
    };
    h_flex()
        .gap(px(4.))
        .items_center()
        .text_size(px(META_SIZE))
        .line_height(px(14.))
        .italic()
        .text_color(tint)
        .when(row.edited, |meta| meta.child("Edited"))
        .child(row.time.clone())
        .when(
            own && row.delivery == Delivery::Delivered,
            |meta| match row.receipt {
                Receipt::Hidden => meta,
                Receipt::Pending => meta.child(receipt_slot(tint)),
                Receipt::Sent => meta.child(symbol("done", META_CHECK_SIZE, tint)),
                Receipt::Read => meta.child(symbol("done_all", META_CHECK_SIZE, theme::own_read())),
            },
        )
}

fn pulse<E: IntoElement + Styled + 'static>(id: &'static str, element: E) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(PULSE_PERIOD)
            .repeat_synced()
            .with_easing(pulsating_between(0.35, 1.)),
        |element, opacity| element.opacity(opacity),
    )
}

fn receipt_slot(tint: Hsla) -> Div {
    h_flex()
        .size(px(META_CHECK_SIZE))
        .flex_none()
        .items_center()
        .justify_center()
        .child(pulse(
            "receipt-slot",
            div()
                .w(px(13.))
                .h(px(4.))
                .rounded(px(2.))
                .bg(tint.opacity(0.5)),
        ))
}

pub fn render_skeleton_row(skeleton: &Skeleton, index: usize) -> AnyElement {
    let corners = bubble_corners(skeleton.own, Default::default());
    let height = 14. + SKELETON_LINE_HEIGHT * f32::from(skeleton.lines);
    let shape = div()
        .w(relative(skeleton.width_ratio))
        .h(px(height))
        .rounded_tl(px(corners.top_left))
        .rounded_tr(px(corners.top_right))
        .rounded_br(px(corners.bottom_right))
        .rounded_bl(px(corners.bottom_left))
        .bg(if skeleton.own {
            theme::bubble_own().opacity(0.55)
        } else {
            theme::bubble_other()
        });
    let line = h_flex()
        .w_full()
        .when(skeleton.own, |line| line.justify_end())
        .when(!skeleton.own, |line| {
            line.gap(px(AVATAR_GAP)).child(
                div()
                    .size(px(AVATAR_SIZE))
                    .flex_none()
                    .rounded_full()
                    .bg(theme::bubble_other()),
            )
        })
        .child(shape);
    div()
        .id(ElementId::Name(format!("row-{index}").into()))
        .w_full()
        .px(px(24.))
        .pt(px(SERIES_START_GAP))
        .child(pulse("skeleton", line))
        .into_any_element()
}

fn meta_room(row: &MessageRow, own: bool) -> usize {
    let mut width = 12. + row.time.chars().count() as f32 * 6.;
    if row.edited {
        width += 56.;
    }
    if own {
        width += META_CHECK_SIZE + 4.;
    }
    (width / FIGURE_SPACE_WIDTH).ceil() as usize
}

fn with_meta_room(blocks: &[Block], room: usize) -> Option<Vec<Block>> {
    let mut padded = blocks.to_vec();
    match padded.last_mut()? {
        Block::Paragraph(inline) | Block::ListItem(inline) => {
            *inline = inline.with_trailing_room(room);
            Some(padded)
        }
        Block::Code(_) | Block::Quote(_) => None,
    }
}

fn has_text(row: &MessageRow) -> bool {
    !row.blocks.is_empty() || (row.images.is_empty() && row.files.is_empty())
}

fn delivery_note(row: &MessageRow, retry: RowAction, index: usize) -> Option<AnyElement> {
    match &row.delivery {
        Delivery::Delivered => None,
        Delivery::Sending => Some(
            h_flex()
                .gap(px(4.))
                .items_center()
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(icon(IconName::Loader, 12., theme::text_muted()))
                .child("Sending")
                .into_any_element(),
        ),
        Delivery::Failed(_) => Some(
            h_flex()
                .gap(px(4.))
                .text_size(px(11.))
                .text_color(theme::red_soft())
                .child("Failed to send.")
                .child(
                    div()
                        .id(ElementId::Name(format!("retry-{index}").into()))
                        .cursor_pointer()
                        .text_color(theme::red_tint())
                        .underline()
                        .child("Retry")
                        .when_some(retry, |link, retry| {
                            link.on_click(move |_, _, cx| retry(cx))
                        }),
                )
                .into_any_element(),
        ),
    }
}

fn edited_marker() -> Div {
    div()
        .text_size(px(11.))
        .italic()
        .text_color(theme::text_muted())
        .child("Edited")
}

fn post_card(row: &MessageRow, index: usize, directory: &Directory, cx: &App) -> Div {
    let header = h_flex()
        .gap(px(10.))
        .items_center()
        .child(person_avatar(
            directory,
            row.sender_id.as_deref(),
            &row.author,
            CARD_AVATAR_SIZE,
        ))
        .child(
            v_flex()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(row.author.clone()),
                )
                .child(
                    h_flex()
                        .gap(px(6.))
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(row.time.clone())
                        .when(row.edited, |line| line.child(edited_marker())),
                ),
        );
    let count = row.reply_count.unwrap_or(0);
    let footer = h_flex()
        .gap(px(8.))
        .items_center()
        .pt(px(8.))
        .border_t_1()
        .border_color(theme::border())
        .when(count > 0, |line| {
            line.child(member_stack(
                directory,
                &row.reply_faces,
                row.reply_faces.len(),
            ))
        })
        .child(
            div()
                .text_size(px(12.5))
                .font_weight(if count > 0 {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if count > 0 {
                    theme::accent_text()
                } else {
                    theme::text_muted()
                })
                .child(match count {
                    0 => "No replies".to_owned(),
                    1 => "1 reply".to_owned(),
                    count => format!("{count} replies"),
                }),
        )
        .children(row.last_reply_time.as_ref().map(|time| {
            div()
                .text_size(px(11.5))
                .text_color(theme::text_muted())
                .child(format!("Last reply {time}"))
        }))
        .child(div().flex_1())
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme::text_soft())
                .child("Reply"),
        );
    let mut body = v_flex().gap(px(8.)).child(header);
    if row.deleted {
        body = body.child(
            div()
                .text_size(px(13.))
                .italic()
                .text_color(theme::text_muted())
                .child(DELETED_PREVIEW),
        );
    } else {
        if has_text(row) {
            body = body.child(
                div()
                    .text_size(px(BODY_SIZE))
                    .line_height(relative(1.45))
                    .text_color(theme::text_strong())
                    .child(render_blocks(
                        &row.blocks,
                        &format!("message-{index}"),
                        false,
                        cx,
                    )),
            );
        }
        body = body.children(attachments_view(
            &row.images,
            &row.files,
            &format!("message-{index}"),
            directory,
        ));
        if !row.reactions.is_empty() {
            body = body.child(
                h_flex()
                    .gap(px(4.))
                    .flex_wrap()
                    .children(row.reactions.iter().map(reaction_chip)),
            );
        }
    }
    v_flex()
        .w_full()
        .gap(px(8.))
        .p(px(12.))
        .rounded(px(10.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border())
        .hover(|card| {
            card.border_color(theme::border_strong())
                .bg(theme::row_hover())
        })
        .child(body)
        .child(footer)
}

fn others_row(
    row: &MessageRow,
    index: usize,
    directory: &Directory,
    retry: RowAction,
    cx: &App,
) -> Div {
    let first = !row.series.has_prev;
    let mut column = v_flex()
        .gap(px(2.))
        .max_w(relative(MAX_WIDTH_RATIO))
        .items_start();
    if first {
        column = column.child(
            h_flex().gap(px(8.)).items_baseline().pl(px(2.)).child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_soft())
                    .child(row.author.clone()),
            ),
        );
    }
    column = column.child(bubble(row, index, false, directory, cx));
    if let Some(note) = delivery_note(row, retry, index) {
        column = column.child(note);
    }
    let lead = if first || row.card {
        person_avatar(
            directory,
            row.sender_id.as_deref(),
            &row.author,
            AVATAR_SIZE,
        )
    } else {
        div().size(px(AVATAR_SIZE)).into_any_element()
    };
    h_flex()
        .w_full()
        .gap(px(AVATAR_GAP))
        .items_start()
        .child(div().w(px(AVATAR_SIZE)).flex_none().child(lead))
        .child(column)
}

fn own_row(
    row: &MessageRow,
    index: usize,
    directory: &Directory,
    retry: RowAction,
    cx: &App,
) -> Div {
    let mut column = v_flex().w_full().items_end().gap(px(2.));
    column = column.child(
        div()
            .max_w(relative(MAX_WIDTH_RATIO))
            .child(bubble(row, index, true, directory, cx)),
    );
    if let Some(note) = delivery_note(row, retry, index) {
        column = column.child(note);
    }
    column
}

pub fn render_message_row(
    row: &MessageRow,
    index: usize,
    actions: RowActions,
    directory: &Directory,
    cx: &App,
) -> AnyElement {
    let RowActions {
        open_thread,
        retry,
        reply,
        hovered,
        highlighted,
    } = actions;
    let own = row.own && !row.card;
    let spacing = if row.card {
        px(CARD_GAP)
    } else if row.series.has_prev {
        px(2.)
    } else {
        px(SERIES_START_GAP)
    };
    let mut container = v_flex().w_full().px(px(24.));
    if let Some(day) = &row.day_header {
        container = container.child(day_separator(day));
    }
    if row.new_marker {
        container = container.child(new_marker());
    }
    let body = if row.card {
        post_card(row, index, directory, cx)
    } else if own {
        own_row(row, index, directory, retry, cx)
    } else {
        others_row(row, index, directory, retry, cx)
    };
    container
        .child(
            div()
                .id(ElementId::Name(format!("row-{index}").into()))
                .w_full()
                .pt(spacing)
                .when_some(open_thread, |element, open| {
                    element.cursor_pointer().on_click(move |_, window, cx| {
                        if TextSelection::selected_text(window, cx).is_empty() {
                            open(cx);
                        }
                    })
                })
                .when_some(hovered, |element, hovered| {
                    element.on_hover(move |is_hovered, _, cx| {
                        if *is_hovered {
                            hovered(cx);
                        }
                    })
                })
                .when_some(reply, |element, reply| {
                    element.on_mouse_down(MouseButton::Right, move |_, _, cx| reply(cx))
                })
                .child(
                    div()
                        .w_full()
                        .rounded(px(10.))
                        .when(highlighted, |element| {
                            element.bg(theme::accent().opacity(0.2))
                        })
                        .child(body),
                ),
        )
        .into_any_element()
}
