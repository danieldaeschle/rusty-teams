use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

use super::adaptive_card::cards_view;
use super::attachments::{FileActions, ImageActions, attachments_view, message_body};
use super::avatar::{bot_avatar, person_avatar};
use super::call_card::{CallActions, call_card_row};
use super::jumbo_emoji::jumbo_row;
use super::link_preview::{link_preview_card, meeting_join_chip};
use super::message_actions::{MessageMenu, ReactionAdder, message_toolbar};
use super::profile_card::opens_profile;
use super::reaction_picker::PickHandler;
use super::reaction_pills::{ReactionControls, reaction_pills};
use super::reaction_proposal_pill::{ReactionPill, reaction_pill};
use super::scheduled_toolbar::{ScheduledMenu, scheduled_toolbar};
use super::toolbar_placement::{HoverChange, TimelineBounds, placed_toolbar};
use super::translation_line::{TranslationActions, translation_line};
use super::widgets::{icon, symbol};
use crate::agents::{AgentRun, run_chip};
use crate::card_state::BotIdentity;
use crate::data::Directory;
use crate::importance::importance_label;
use crate::render::Block;
use crate::restorable_deletes::{DELETED_TOMBSTONE, UNDO_DELETE_LABEL};
use crate::rows::{
    Corners, Delivery, MessageRow, Receipt, ScheduledState, Skeleton, bubble_corners,
};
use crate::sidebar_model::DELETED_PREVIEW;
use crate::theme;
use app_ui_kit::layout::{COMFY_RUNWAY_WIDTH, ROW_PADDING};

pub type RowAction = Option<Box<dyn Fn(&mut App)>>;
pub type SharedRowAction = Option<std::rc::Rc<dyn Fn(&mut App)>>;

pub const BUBBLE_TAGS: [&str; 12] = [
    "message-bubble/0",
    "message-bubble/1",
    "message-bubble/2",
    "message-bubble/3",
    "message-bubble/4",
    "message-bubble/5",
    "message-bubble/6",
    "message-bubble/7",
    "message-bubble/8",
    "message-bubble/9",
    "message-bubble/10",
    "message-bubble/11",
];
pub const IMPORTANCE_LABEL_TAG: &str = "message-importance-label";
pub(super) const AVATAR_SIZE: f32 = 28.;
pub(super) const AVATAR_GAP: f32 = 8.;
const SERIES_START_GAP: f32 = 8.;
const SERIES_GAP: f32 = 2.;
const COMPACT_SERIES_START_GAP: f32 = 4.;
const COMPACT_SERIES_GAP: f32 = 1.;
const COMPACT_BUBBLE_PADDING_Y: f32 = 4.;
pub(super) const BODY_SIZE: f32 = 13.5;
const MAX_WIDTH_RATIO: f32 = 0.7;
const NARROW_BUBBLE_WIDTH: f32 = 300.;
const BUBBLE_PADDING_Y: f32 = 8.;
const META_BOTTOM_INSET: f32 = 4.;
const META_SIZE: f32 = 11.;
const META_CHECK_SIZE: f32 = 15.;
const SAVED_MARK_SIZE: f32 = 12.;
const FORWARDED_SIZE: f32 = 11.;
const IMPORTANCE_LABEL_SIZE: f32 = 12.;
const IMPORTANCE_LINE_HEIGHT: f32 = 16.;
const COMPACT_IMPORTANCE_LINE_HEIGHT: f32 = 14.;
pub(super) const BUBBLE_PADDING_X: f32 = 12.;
const FIGURE_SPACE_WIDTH: f32 = 7.4;
const PULSE_PERIOD: Duration = Duration::from_millis(1600);
const RECEIPT_FADE_PERIOD: Duration = Duration::from_millis(600);
pub const JUMP_HIGHLIGHT_PERIOD: Duration = Duration::from_millis(2400);
const HIGHLIGHT_FADE_IN: f32 = 0.1;
const HIGHLIGHT_FADE_OUT: f32 = 0.4;
const HIGHLIGHT_TINT: f32 = 0.3;
const SKELETON_LINE_HEIGHT: f32 = 20.;
pub const SELECT_BOX_SIZE: f32 = 16.;
pub const SELECT_GAP: f32 = 12.;
const SELECT_CHECK_SIZE: f32 = 12.;
const SELECT_BOX_RADIUS: f32 = 4.;
const SELECT_BLOCKED_OPACITY: f32 = 0.4;
const SELECTED_TINT: f32 = 0.08;
pub fn select_box_selector(index: usize) -> String {
    format!("forward-check/{index}")
}

pub struct DeliveryActions {
    pub retry: RowAction,
    pub delete: RowAction,
    pub send_now: RowAction,
}

pub struct RowSelect {
    pub checked: bool,
    pub blocked: Option<&'static str>,
    pub toggle: std::rc::Rc<dyn Fn(&mut App)>,
}

pub struct RowActions {
    pub select: Option<RowSelect>,
    pub bot: Option<BotIdentity>,
    pub delivery: DeliveryActions,
    pub reply: RowAction,
    pub hovered: Option<HoverChange>,
    pub menu: Option<MessageMenu>,
    pub scheduled_menu: Option<ScheduledMenu>,
    pub react: Option<PickHandler>,
    pub reaction_controls: Option<ReactionControls>,
    pub add_reaction: Option<ReactionAdder>,
    pub files: Option<FileActions>,
    pub open_image: Option<ImageActions>,
    pub highlighted: bool,
    pub saved: bool,
    pub pinned: bool,
    pub translation: Option<TranslationActions>,
    pub timeline: TimelineBounds,
    pub agent_run: Option<AgentRun>,
    pub call: CallActions,
    pub reaction_proposal: Option<ReactionPill>,
    pub undo_delete: SharedRowAction,
    pub compact: bool,
}

pub fn row_spacing(has_previous: bool, compact: bool) -> f32 {
    match (has_previous, compact) {
        (true, false) => SERIES_GAP,
        (true, true) => COMPACT_SERIES_GAP,
        (false, false) => SERIES_START_GAP,
        (false, true) => COMPACT_SERIES_START_GAP,
    }
}

pub fn bubble_padding_y(compact: bool) -> f32 {
    if compact {
        COMPACT_BUBBLE_PADDING_Y
    } else {
        BUBBLE_PADDING_Y
    }
}

const TOOLBAR_RIGHT_INSET: f32 = 0.;

fn separator_pill(label: &str, fill: Hsla, text_color: Hsla) -> Div {
    h_flex()
        .w_full()
        .justify_center()
        .mt(px(12.))
        .mb(px(6.))
        .child(
            div()
                .h(px(24.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(12.))
                .bg(fill)
                .text_size(px(11.))
                .text_color(text_color)
                .child(label.to_owned()),
        )
}

pub(super) fn day_separator(label: &str) -> Div {
    separator_pill(label, theme::row_hover(), theme::text_muted())
}

pub(super) fn new_marker() -> Div {
    separator_pill(
        "New",
        theme::mention_background(false),
        theme::accent_text(),
    )
}

pub(super) fn deleted_tombstone(index: usize, undo_delete: SharedRowAction) -> Div {
    let line =
        h_flex()
            .gap(px(8.))
            .items_baseline()
            .text_size(px(13.))
            .child(div().italic().text_color(theme::text_muted()).child(
                if undo_delete.is_some() {
                    DELETED_TOMBSTONE
                } else {
                    DELETED_PREVIEW
                },
            ));
    match undo_delete {
        Some(undo) => line.child(
            div()
                .id(ElementId::Name(format!("undo-delete-{index}").into()))
                .cursor_pointer()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::accent_text())
                .hover(|link| link.underline())
                .child(UNDO_DELETE_LABEL)
                .on_click(move |_, _, cx| undo(cx)),
        ),
        None => line,
    }
}

fn bubble_max_width(timeline: &TimelineBounds, reserved: f32, compact: bool) -> Length {
    let Some(bounds) = timeline.get() else {
        return relative(MAX_WIDTH_RATIO).into();
    };
    let runway = runway_width(f32::from(bounds.size.width), compact);
    let row_width = (runway - 2. * ROW_PADDING - reserved).max(0.);
    px((row_width * MAX_WIDTH_RATIO).max(row_width.min(NARROW_BUBBLE_WIDTH))).into()
}

pub fn runway_width(timeline_width: f32, compact: bool) -> f32 {
    if compact {
        timeline_width
    } else {
        timeline_width.min(COMFY_RUNWAY_WIDTH)
    }
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
    let jumbo = row.jumbo.as_deref().filter(|_| {
        !row.deleted && matches!(row.delivery, Delivery::Delivered | Delivery::Sending)
    });
    let mut element = div()
        .id(ElementId::Name(format!("bubble-{index}").into()))
        .when_some(extras.hover.clone(), |element, hover| {
            element.on_hover(move |is_hovered, _, cx| hover(*is_hovered, cx))
        })
        .relative()
        .max_w_full()
        .when_some(BUBBLE_TAGS.get(index).copied(), |element, tag| {
            element.debug_selector(|| tag.into())
        })
        .when(jumbo.is_none(), |element| {
            element
                .rounded_tl(px(corners.top_left))
                .rounded_tr(px(corners.top_right))
                .rounded_br(px(corners.bottom_right))
                .rounded_bl(px(corners.bottom_left))
                .px(px(BUBBLE_PADDING_X))
                .py(px(bubble_padding_y(extras.compact)))
        })
        .text_size(px(BODY_SIZE))
        .line_height(relative(1.45));
    if row.deleted {
        return element
            .border_1()
            .border_color(theme::border_strong())
            .py(px(6.))
            .child(deleted_tombstone(index, extras.undo_delete));
    }
    let failed = matches!(row.delivery, Delivery::Failed(_));
    let scheduled = matches!(row.delivery, Delivery::Scheduled(_));
    element = element
        .when(!scheduled && jumbo.is_none(), |bubble| {
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
            bubble.border_1().border_color(
                if row.delivery == Delivery::Scheduled(ScheduledState::DeliveryFailed) {
                    theme::red()
                } else {
                    theme::accent()
                },
            )
        });
    element = element.when(extras.highlighted, |bubble| {
        bubble.child(jump_highlight(index, corners))
    });
    let has_reactions = !row.reactions.is_empty();
    let saved = extras.saved;
    let mut content = v_flex().gap(px(6.));
    content = content.children(importance_header(row, extras.compact));
    if row.forwarded {
        content = content.child(forwarded_header());
    }
    let attachments = attachments_view(
        &row.blocks,
        &row.images,
        &row.local_images,
        &row.files,
        &format!("message-{index}"),
        directory,
        extras.files.as_ref(),
        extras.open_image.as_ref(),
    );
    let cards = cards_view(&row.adaptive_cards, &row.conversation_id, &row.key, cx);
    let trailing_content = attachments.is_some()
        || row.meeting_link.is_some()
        || row.link_preview.is_some()
        || !cards.is_empty();
    let padded_blocks = if has_reactions || trailing_content || jumbo.is_some() {
        None
    } else {
        with_meta_room(&row.blocks, meta_room(row, own, saved))
    };
    let meta_inline = has_text(row) && padded_blocks.is_some();
    if let Some(jumbo) = jumbo {
        content = content.child(jumbo_row(&row.key, jumbo, own));
    } else if has_text(row) {
        content = content.children(message_body(
            padded_blocks.as_deref().unwrap_or(&row.blocks),
            &row.images,
            &row.local_images,
            &format!("message-{index}"),
            own,
            directory,
            extras.open_image.as_ref(),
            cx,
        ));
    }
    content = content.children(attachments);
    content = content.children(
        row.meeting_link
            .clone()
            .map(|url| meeting_join_chip(format!("message-{index}-join"), url)),
    );
    content = content.children(row.link_preview.as_ref().map(|preview| {
        link_preview_card(
            preview,
            format!("message-{index}-link"),
            directory,
            false,
            None,
        )
    }));
    content = content.children(cards);
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
                        .flex_none()
                        .mb(px(META_BOTTOM_INSET - bubble_padding_y(extras.compact)))
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
                .right(px(8.))
                .bottom(px(META_BOTTOM_INSET))
                .child(bubble_meta(row, own, saved)),
        );
    }
    if let Some(menu) = extras.menu {
        element = element.child(placed_toolbar(
            message_toolbar(menu, cx),
            extras.timeline.clone(),
            TOOLBAR_RIGHT_INSET,
        ));
    }
    if let Some(menu) = extras.scheduled_menu {
        element = element.child(placed_toolbar(
            scheduled_toolbar(menu),
            extras.timeline.clone(),
            TOOLBAR_RIGHT_INSET,
        ));
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
    open_image: Option<ImageActions>,
    saved: bool,
    translation: Option<TranslationActions>,
    timeline: TimelineBounds,
    agent_run: Option<AgentRun>,
    reaction_proposal: Option<ReactionPill>,
    undo_delete: SharedRowAction,
    compact: bool,
    highlighted: bool,
}

fn agent_chip(run: Option<AgentRun>) -> Option<Div> {
    run.map(|run| div().py(px(2.)).child(run_chip(&run, theme::row_hover())))
}

/// Teams: bold "Important!" / "Urgent!" in capitals above the message text.
pub(super) fn importance_header(row: &MessageRow, compact: bool) -> Option<Div> {
    let label = importance_label(row.importance)?;
    Some(
        div()
            .debug_selector(|| IMPORTANCE_LABEL_TAG.into())
            .text_size(px(IMPORTANCE_LABEL_SIZE))
            .line_height(px(if compact {
                COMPACT_IMPORTANCE_LINE_HEIGHT
            } else {
                IMPORTANCE_LINE_HEIGHT
            }))
            .font_weight(FontWeight::BOLD)
            .text_color(theme::red())
            .child(label),
    )
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
        .when(own && row.delivery == Delivery::Delivered, |meta| {
            let check = match row.receipt {
                Receipt::Hidden => return meta,
                Receipt::Sent => symbol("done", META_CHECK_SIZE, tint).into_any_element(),
                Receipt::Read => {
                    symbol("done_all", META_CHECK_SIZE, theme::own_read()).into_any_element()
                }
                Receipt::ReadLive => receipt_fade_in(
                    &row.key,
                    div().child(symbol("done_all", META_CHECK_SIZE, theme::own_read())),
                )
                .into_any_element(),
            };
            let holder = div()
                .id(ElementId::Name(format!("{}-receipt", row.key).into()))
                .child(check);
            meta.child(match row.receipt_label.clone() {
                Some(label) => holder.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(label.clone()).build(window, cx)
                }),
                None => holder,
            })
        })
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

fn jump_highlight(index: usize, corners: Corners) -> AnimationElement<Div> {
    div()
        .absolute()
        .inset_0()
        .rounded_tl(px(corners.top_left))
        .rounded_tr(px(corners.top_right))
        .rounded_br(px(corners.bottom_right))
        .rounded_bl(px(corners.bottom_left))
        .bg(theme::accent().opacity(HIGHLIGHT_TINT))
        .with_animation(
            SharedString::from(format!("jump-highlight/{index}")),
            Animation::new(JUMP_HIGHLIGHT_PERIOD),
            |highlight, delta| highlight.opacity(highlight_opacity(delta)),
        )
}

fn highlight_opacity(delta: f32) -> f32 {
    let fade_in = (delta / HIGHLIGHT_FADE_IN).min(1.);
    let fade_out = ((1. - delta) / HIGHLIGHT_FADE_OUT).min(1.);
    ease_in_out(fade_in.min(fade_out))
}

fn receipt_fade_in(message_key: &str, check: Div) -> AnimationElement<Div> {
    check.with_animation(
        SharedString::from(format!("receipt-fade/{message_key}")),
        Animation::new(RECEIPT_FADE_PERIOD).with_easing(ease_in_out),
        |check, delta| check.opacity(0.25 + 0.75 * delta),
    )
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
        .px(px(ROW_PADDING))
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

fn delivery_link(
    index: usize,
    name: &str,
    label: &'static str,
    action: RowAction,
) -> Stateful<Div> {
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
    mut extras: BubbleExtras,
    cx: &App,
) -> Div {
    let translation = extras.translation.take();
    let agent_run = extras.agent_run.take();
    let reaction_proposal = extras.reaction_proposal.take();
    let first = !row.series.has_prev;
    let author = bot.map_or_else(|| row.author.clone(), |bot| bot.name.clone());
    let profile_user = row.sender_id.as_deref().filter(|_| bot.is_none());
    let mut column = v_flex()
        .gap(px(2.))
        .flex_1()
        .min_w_0()
        .max_w(bubble_max_width(
            &extras.timeline,
            AVATAR_SIZE + AVATAR_GAP,
            extras.compact,
        ))
        .items_start();
    if first {
        column = column.child(
            h_flex().gap(px(8.)).items_baseline().pl(px(2.)).child(
                opens_profile(
                    div().id(ElementId::Name(format!("author-{index}").into())),
                    profile_user,
                )
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_soft())
                .child(author.clone()),
            ),
        );
    }
    column = column.child(bubble(row, index, false, directory, extras, cx));
    column = column.children(agent_chip(agent_run));
    column = column.children(reaction_proposal.map(|pill| reaction_pill(pill, &row.key)));
    column = column.children(translation_line(row, index, translation.as_ref()));
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
        .child(
            opens_profile(
                div().id(ElementId::Name(format!("author-avatar-{index}").into())),
                profile_user.filter(|_| first),
            )
            .w(px(AVATAR_SIZE))
            .flex_none()
            .child(lead),
        )
        .child(column)
}

fn own_row(
    row: &MessageRow,
    index: usize,
    directory: &Directory,
    delivery: DeliveryActions,
    mut extras: BubbleExtras,
    cx: &App,
) -> Div {
    let translation = extras.translation.take();
    let agent_run = extras.agent_run.take();
    let reaction_proposal = extras.reaction_proposal.take();
    let mut column = v_flex().w_full().items_end().gap(px(2.));
    column = column.child(
        h_flex().w_full().justify_end().child(
            div()
                .min_w_0()
                .max_w(bubble_max_width(&extras.timeline, 0., extras.compact))
                .child(bubble(row, index, true, directory, extras, cx)),
        ),
    );
    column = column.children(agent_chip(agent_run));
    column = column.children(reaction_proposal.map(|pill| reaction_pill(pill, &row.key)));
    column = column.children(translation_line(row, index, translation.as_ref()));
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
    if let Some(call) = &row.call {
        let selecting = actions.select.is_some();
        let card = call_card_row(row, call, index, directory, actions.call);
        return if selecting {
            div()
                .w_full()
                .pl(px(SELECT_BOX_SIZE + SELECT_GAP))
                .child(card)
                .into_any_element()
        } else {
            card
        };
    }
    let RowActions {
        select,
        bot,
        delivery,
        reply,
        hovered,
        menu,
        scheduled_menu,
        react,
        reaction_controls,
        add_reaction: _,
        files,
        open_image,
        highlighted,
        saved,
        translation,
        timeline,
        agent_run,
        call: _,
        pinned: _,
        reaction_proposal,
        undo_delete,
        compact,
    } = actions;
    let extras = BubbleExtras {
        hover: hovered,
        menu,
        scheduled_menu,
        react,
        controls: reaction_controls,
        files,
        open_image,
        saved,
        translation,
        timeline,
        agent_run,
        reaction_proposal,
        undo_delete,
        compact,
        highlighted,
    };
    let own = row.own;
    let spacing = px(row_spacing(row.series.has_prev, compact));
    let mut container = v_flex().w_full().px(px(ROW_PADDING));
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
    let checked = select.as_ref().is_some_and(|select| select.checked);
    let body = match select {
        Some(select) => selectable_body(select, index, body).into_any_element(),
        None => body.into_any_element(),
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
                        .when(checked && !highlighted, |element| {
                            element.bg(theme::accent().opacity(SELECTED_TINT))
                        })
                        .child(body),
                ),
        )
        .into_any_element()
}

fn selectable_body(select: RowSelect, index: usize, body: impl IntoElement) -> Stateful<Div> {
    let RowSelect {
        checked,
        blocked,
        toggle,
    } = select;
    h_flex()
        .id(ElementId::Name(format!("forward-select-{index}").into()))
        .w_full()
        .items_center()
        .gap(px(SELECT_GAP))
        .child(select_box(checked, blocked.is_some(), index))
        .child(div().flex_1().min_w_0().child(body))
        .map(|row| match blocked {
            Some(reason) => row.tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(reason).build(window, cx)
            }),
            None => row.cursor_pointer().on_click(move |_, _, cx| {
                cx.stop_propagation();
                toggle(cx)
            }),
        })
}

pub fn select_box(checked: bool, blocked: bool, index: usize) -> Div {
    div()
        .debug_selector(|| select_box_selector(index))
        .size(px(SELECT_BOX_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(SELECT_BOX_RADIUS))
        .map(|element| {
            if checked {
                element.bg(theme::accent()).child(icon(
                    IconName::Check,
                    SELECT_CHECK_SIZE,
                    theme::on_accent(),
                ))
            } else {
                element.bg(theme::border_strong())
            }
        })
        .when(blocked, |element| element.opacity(SELECT_BLOCKED_OPACITY))
}

#[cfg(test)]
mod highlight_tests {
    use super::highlight_opacity;

    #[test]
    fn jump_highlight_fades_in_holds_and_fades_out() {
        assert_eq!(highlight_opacity(0.), 0.);
        assert_eq!(highlight_opacity(0.5), 1.);
        assert!(highlight_opacity(0.8) < 1.);
        assert!(highlight_opacity(0.99) < 0.05);
    }
}

#[cfg(test)]
mod density_tests {
    use super::{META_BOTTOM_INSET, bubble_padding_y, row_spacing, runway_width};

    #[test]
    fn compact_density_tightens_gaps_and_bubble_padding() {
        assert!(row_spacing(false, true) < row_spacing(false, false));
        assert!(row_spacing(true, true) < row_spacing(true, false));
        assert!(bubble_padding_y(true) < bubble_padding_y(false));
        assert!(META_BOTTOM_INSET - bubble_padding_y(true) >= 0.);
    }

    #[test]
    fn comfy_density_caps_the_runway_like_teams_and_compact_uses_the_full_width() {
        assert_eq!(runway_width(2400., false), 1060.);
        assert_eq!(runway_width(800., false), 800.);
        assert_eq!(runway_width(2400., true), 2400.);
    }
}
