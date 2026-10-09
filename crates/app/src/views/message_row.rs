use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

use super::adaptive_card::cards_view;
use super::attachments::{FileActions, attachments_view, message_body};
use super::avatar::{bot_avatar, person_avatar};
use super::link_preview::link_preview_card;
use super::message_actions::{HoverChange, MessageMenu, message_toolbar};
use super::reaction_picker::PickHandler;
use super::reaction_pills::{ReactionControls, reaction_pills};
use super::scheduled_toolbar::{ScheduledMenu, scheduled_toolbar};
use super::widgets::{icon, symbol};
use crate::card_state::BotIdentity;
use crate::data::Directory;
use crate::render::Block;
use crate::rows::{Delivery, MessageRow, Receipt, ScheduledState, Skeleton, bubble_corners};
use crate::sidebar_model::DELETED_PREVIEW;
use crate::theme;

pub type RowAction = Option<Box<dyn Fn(&mut App)>>;

const AVATAR_SIZE: f32 = 28.;
const AVATAR_GAP: f32 = 8.;
const SERIES_START_GAP: f32 = 8.;
pub(super) const BODY_SIZE: f32 = 13.5;
const MAX_WIDTH_RATIO: f32 = 0.7;
const REACTION_FOOTER_HEIGHT: f32 = 24.;
const META_SIZE: f32 = 11.;
const META_CHECK_SIZE: f32 = 15.;
const SAVED_MARK_SIZE: f32 = 12.;
const FORWARDED_SIZE: f32 = 11.;
const FIGURE_SPACE_WIDTH: f32 = 7.4;
const PULSE_PERIOD: Duration = Duration::from_millis(1600);
const SKELETON_LINE_HEIGHT: f32 = 20.;

pub struct DeliveryActions {
    pub retry: RowAction,
    pub delete: RowAction,
    pub send_now: RowAction,
}

pub struct RowActions {
    pub bot: Option<BotIdentity>,
    pub delivery: DeliveryActions,
    pub reply: RowAction,
    pub hovered: Option<HoverChange>,
    pub menu: Option<MessageMenu>,
    pub scheduled_menu: Option<ScheduledMenu>,
    pub react: Option<PickHandler>,
    pub reaction_controls: Option<ReactionControls>,
    pub files: Option<FileActions>,
    pub highlighted: bool,
    pub saved: bool,
}

const TOOLBAR_LIFT: f32 = 22.;

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

fn bubble(
    row: &MessageRow,
    index: usize,
    own: bool,
    directory: &Directory,
    extras: BubbleExtras,
    cx: &App,
) -> Stateful<Div> {
    let corners = bubble_corners(own, row.series);
    let mut element = div()
        .id(ElementId::Name(format!("bubble-{index}").into()))
        .when_some(extras.hover.clone(), |element, hover| {
            element.on_hover(move |is_hovered, _, cx| hover(*is_hovered, cx))
        })
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
    let scheduled = matches!(row.delivery, Delivery::Scheduled(_));
    element = element
        .when(!scheduled, |bubble| {
            bubble.bg(if own && !failed {
                theme::bubble_own()
            } else {
                theme::bubble_other()
            })
        })
        .when(failed, |bubble| {
            bubble.border_1().border_color(theme::red())
        })
        .when(scheduled, |bubble| {
            bubble
                .border_1()
                .border_dashed()
                .border_color(if row.delivery == Delivery::Scheduled(ScheduledState::DeliveryFailed) {
                    theme::red()
                } else {
                    theme::accent()
                })
        });
    let has_reactions = !row.reactions.is_empty();
    let saved = extras.saved;
    let mut content = v_flex().gap(px(6.));
    if row.forwarded {
        content = content.child(forwarded_header());
    }
    let padded_blocks = if has_reactions {
        None
    } else {
        with_meta_room(&row.blocks, meta_room(row, own, saved))
    };
    let meta_inline = has_text(row) && padded_blocks.is_some();
    if has_text(row) {
        content = content.children(message_body(
            padded_blocks.as_deref().unwrap_or(&row.blocks),
            &row.images,
            &row.local_images,
            &format!("message-{index}"),
            own,
            directory,
            cx,
        ));
    }
    content = content.children(attachments_view(
        &row.blocks,
        &row.images,
        &row.local_images,
        &row.files,
        &format!("message-{index}"),
        directory,
        extras.files.as_ref(),
    ));
    content = content.children(row.link_preview.as_ref().map(|preview| {
        link_preview_card(
            preview,
            format!("message-{index}-link"),
            directory,
            false,
            None,
        )
    }));
    content = content.children(cards_view(
        &row.adaptive_cards,
        &row.conversation_id,
        &row.key,
        cx,
    ));
    if has_reactions {
        content = content.child(
            h_flex()
                .gap(px(8.))
                .items_end()
                .child(
                    reaction_pills(
                        &row.reactions,
                        index,
                        own,
                        directory,
                        extras.react.clone(),
                        extras.controls.as_ref(),
                    )
                    .flex_auto(),
                )
                .child(
                    h_flex()
                        .h(px(REACTION_FOOTER_HEIGHT))
                        .flex_none()
                        .items_center()
                        .child(bubble_meta(row, own, saved)),
                ),
        );
    } else if !meta_inline {
        content = content.child(h_flex().justify_end().child(bubble_meta(row, own, saved)));
    }
    element = element.child(content);
    if meta_inline {
        element = element.child(
            div()
                .absolute()
                .right(px(9.))
                .bottom(px(5.))
                .child(bubble_meta(row, own, saved)),
        );
    }
    if let Some(menu) = extras.menu {
        element = element.child(
            div()
                .absolute()
                .top(px(-TOOLBAR_LIFT))
                .right(px(8.))
                .child(deferred(message_toolbar(menu)).with_priority(1)),
        );
    }
    if let Some(menu) = extras.scheduled_menu {
        element = element.child(
            div()
                .absolute()
                .top(px(-TOOLBAR_LIFT))
                .right(px(8.))
                .child(deferred(scheduled_toolbar(menu)).with_priority(1)),
        );
    }
    element
}

#[derive(Default)]
struct BubbleExtras {
    hover: Option<HoverChange>,
    menu: Option<MessageMenu>,
    scheduled_menu: Option<ScheduledMenu>,
    react: Option<PickHandler>,
    controls: Option<ReactionControls>,
    files: Option<FileActions>,
    saved: bool,
}

pub(super) fn forwarded_header() -> Div {
    h_flex()
        .gap(px(4.))
        .items_center()
        .text_size(px(FORWARDED_SIZE))
        .text_color(theme::text_muted())
        .child(icon(IconName::Forward, FORWARDED_SIZE, theme::text_muted()))
        .child("Forwarded")
}

fn bubble_meta(row: &MessageRow, own: bool, saved: bool) -> Div {
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
        .when(saved, |meta| {
            meta.child(icon(
                IconName::Bookmark,
                SAVED_MARK_SIZE,
                theme::accent_text(),
            ))
        })
        .child(row.time.clone())
        .when(own && row.delivery == Delivery::Sending, |meta| {
            meta.child(symbol("schedule", META_CHECK_SIZE, tint))
        })
        .when(matches!(row.delivery, Delivery::Scheduled(_)), |meta| {
            meta.child(symbol("schedule", META_SIZE + 3., tint))
        })
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

fn meta_room(row: &MessageRow, own: bool, saved: bool) -> usize {
    let mut width = 12. + row.time.chars().count() as f32 * 6.;
    if saved {
        width += SAVED_MARK_SIZE + 4.;
    }
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
    let inline = padded.last_mut()?.last_inline_mut()?;
    *inline = inline.with_trailing_room(room);
    Some(padded)
}

pub(super) fn has_text(row: &MessageRow) -> bool {
    !row.blocks.is_empty()
        || (row.images.is_empty()
            && row.local_images.is_empty()
            && row.files.is_empty()
            && row.adaptive_cards.is_empty())
}

fn delivery_link(index: usize, name: &str, label: &'static str, action: RowAction) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("{name}-{index}").into()))
        .cursor_pointer()
        .text_color(theme::red_tint())
        .underline()
        .child(label)
        .when_some(action, |link, action| {
            link.on_click(move |_, _, cx| action(cx))
        })
}

pub(super) fn delivery_note(
    row: &MessageRow,
    delivery: DeliveryActions,
    index: usize,
) -> Option<AnyElement> {
    let DeliveryActions {
        retry,
        delete,
        send_now,
    } = delivery;
    let note = |text: &'static str| {
        h_flex()
            .gap(px(4.))
            .text_size(px(11.))
            .text_color(theme::red_soft())
            .child(text)
    };
    match &row.delivery {
        Delivery::Delivered | Delivery::Sending | Delivery::Scheduled(ScheduledState::Waiting) => {
            None
        }
        Delivery::Failed(_) => Some(
            note("Failed to send.")
                .child(delivery_link(index, "retry", "Retry", retry))
                .child(delivery_link(index, "delete", "Delete", delete))
                .into_any_element(),
        ),
        Delivery::Scheduled(ScheduledState::DeliveryFailed) => Some(
            note("Couldn't send at the scheduled time.")
                .child(delivery_link(index, "send-now", "Send now", send_now))
                .child(delivery_link(index, "delete", "Delete", delete))
                .into_any_element(),
        ),
        Delivery::Scheduled(ScheduledState::ChangeFailed) => Some(
            note("Couldn't change the scheduled message.")
                .child(delivery_link(index, "retry", "Retry", retry))
                .into_any_element(),
        ),
    }
}

fn others_row(
    row: &MessageRow,
    index: usize,
    directory: &Directory,
    bot: Option<&BotIdentity>,
    delivery: DeliveryActions,
    extras: BubbleExtras,
    cx: &App,
) -> Div {
    let first = !row.series.has_prev;
    let author = bot.map_or_else(|| row.author.clone(), |bot| bot.name.clone());
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
                    .child(author.clone()),
            ),
        );
    }
    column = column.child(bubble(row, index, false, directory, extras, cx));
    if let Some(note) = delivery_note(row, delivery, index) {
        column = column.child(note);
    }
    let lead = if let Some(bot) = bot.filter(|_| first) {
        bot_avatar(bot, AVATAR_SIZE)
    } else if first {
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
    delivery: DeliveryActions,
    extras: BubbleExtras,
    cx: &App,
) -> Div {
    let mut column = v_flex().w_full().items_end().gap(px(2.));
    column = column.child(
        div()
            .max_w(relative(MAX_WIDTH_RATIO))
            .child(bubble(row, index, true, directory, extras, cx)),
    );
    if let Some(note) = delivery_note(row, delivery, index) {
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
        bot,
        delivery,
        reply,
        hovered,
        menu,
        scheduled_menu,
        react,
        reaction_controls,
        files,
        highlighted,
        saved,
    } = actions;
    let extras = BubbleExtras {
        hover: hovered,
        menu,
        scheduled_menu,
        react,
        controls: reaction_controls,
        files,
        saved,
    };
    let own = row.own;
    let spacing = if row.series.has_prev {
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
    let body = if own {
        own_row(row, index, directory, delivery, extras, cx)
    } else {
        others_row(row, index, directory, bot.as_ref(), delivery, extras, cx)
    };
    container
        .child(
            div()
                .id(ElementId::Name(format!("row-{index}").into()))
                .w_full()
                .pt(spacing)
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
