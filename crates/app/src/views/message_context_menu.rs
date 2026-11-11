use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::message_actions::{Action, MessageMenu, picker_button, reaction_button};
use super::widgets::icon;
use crate::forward_selection::FORWARD_LABEL;
use crate::post_follow::follow_label;
use crate::share_outlook::SHARE_TO_OUTLOOK_LABEL;
use crate::theme::{self, ElevatedSurface as _};

pub const CONTEXT_MENU_TAG: &str = "message-context-menu";
const MENU_WIDTH: f32 = 220.;
const PANEL_GAP: f32 = 6.;
const PILL_PADDING: f32 = 4.;
const LIST_PADDING: f32 = 4.;
const ITEM_HEIGHT: f32 = 32.;
const ITEM_ICON: f32 = 16.;
const WINDOW_MARGIN: f32 = 8.;
const SELECT_LABEL: &str = "Select";

pub type Close = Rc<dyn Fn(&mut Window, &mut App)>;

struct Entry {
    label: SharedString,
    glyph: IconName,
    action: Action,
    danger: bool,
}

fn entry(label: impl Into<SharedString>, glyph: IconName, action: Action) -> Entry {
    Entry {
        label: label.into(),
        glyph,
        action,
        danger: false,
    }
}

fn entries(menu: &MessageMenu) -> Vec<Entry> {
    let mut list = Vec::new();
    if let Some(reply) = menu.reply.clone() {
        list.push(entry("Reply", IconName::Reply, reply));
    }
    if let Some(edit) = menu.edit.clone() {
        list.push(entry("Edit", IconName::Pencil, edit));
    }
    if let Some(toggle_pinned) = menu.toggle_pinned.clone() {
        list.push(if menu.pinned {
            entry("Unpin", IconName::PinOff, toggle_pinned)
        } else {
            entry("Pin", IconName::Pin, toggle_pinned)
        });
    }
    list.push(entry("Copy text", IconName::Copy, menu.copy.clone()));
    if let Some(copy_link) = menu.copy_link.clone() {
        list.push(entry("Copy link", IconName::Link, copy_link));
    }
    if let Some(forward) = menu.forward.clone() {
        list.push(entry(FORWARD_LABEL, IconName::Forward, forward));
    }
    if let Some(save) = menu.save.clone() {
        list.push(if menu.saved {
            entry("Unsave", IconName::BookmarkOff, save)
        } else {
            entry("Save", IconName::Bookmark, save)
        });
    }
    if let Some(toggle_follow) = menu.toggle_follow.clone() {
        let glyph = if menu.followed { IconName::BellOff } else { IconName::Bell };
        list.push(entry(follow_label(menu.followed), glyph, toggle_follow));
    }
    if let Some(mark_unread) = menu.mark_unread.clone() {
        list.push(entry("Mark as unread", IconName::EyeOff, mark_unread));
    }
    if let Some(translate) = menu.translate.clone() {
        list.push(entry(menu.translate_label, IconName::Languages, translate));
    }
    if let Some(share_to_outlook) = menu.share_to_outlook.clone() {
        list.push(entry(SHARE_TO_OUTLOOK_LABEL, IconName::Mail, share_to_outlook));
    }
    if let Some(refresh_card) = menu.refresh_card.clone() {
        list.push(entry("Refresh card", IconName::RefreshCw, refresh_card));
    }
    for delegate in &menu.delegate {
        list.push(entry(delegate.label.clone(), IconName::Bot, delegate.action.clone()));
    }
    if let Some(delete) = menu.delete.clone() {
        list.push(Entry {
            danger: true,
            ..entry("Delete", IconName::Trash, delete)
        });
    }
    if let Some(select) = menu.forward_multiple.clone() {
        list.push(entry(SELECT_LABEL, IconName::CircleCheck, select));
    }
    list
}

fn item(key: &str, index: usize, entry: Entry, close: Close) -> AnyElement {
    let color = if entry.danger { theme::red() } else { theme::text() };
    let action = entry.action;
    h_flex()
        .id(ElementId::Name(format!("context-item-{index}-{key}").into()))
        .debug_selector(|| format!("{CONTEXT_MENU_TAG}/item").into())
        .h(px(ITEM_HEIGHT))
        .px(px(10.))
        .gap(px(12.))
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(13.))
        .text_color(color)
        .hover(|row| row.bg(theme::popup_hover()))
        .child(icon(entry.glyph, ITEM_ICON, if entry.danger { theme::red() } else { theme::text_soft() }))
        .child(div().flex_1().min_w_0().truncate().child(entry.label))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            close(window, cx);
            action(window, cx);
        })
        .into_any_element()
}

/// Telegram-style message menu: opens at the cursor, quick reactions on top, the actions below.
pub fn message_context_menu(
    mut menu: MessageMenu,
    position: Point<Pixels>,
    close: Close,
    close_from_outside: Close,
    cx: &App,
) -> AnyElement {
    let picked = menu.done.clone();
    let after_pick = close.clone();
    menu.done = Rc::new(move |window, cx| {
        picked(window, cx);
        after_pick(window, cx);
    });
    let key = menu.key.clone();
    let mut reactions: Vec<AnyElement> = menu
        .quick
        .iter()
        .enumerate()
        .map(|(index, glyph)| reaction_button(&menu, glyph, index))
        .collect();
    reactions.push(picker_button(&menu, cx));
    let items: Vec<AnyElement> = entries(&menu)
        .into_iter()
        .enumerate()
        .map(|(index, entry)| item(&key, index, entry, close.clone()))
        .collect();
    deferred(
        anchored()
            .position(position)
            .snap_to_window_with_margin(px(WINDOW_MARGIN))
            .child(
                v_flex()
                    .id(ElementId::Name(format!("{CONTEXT_MENU_TAG}-{key}").into()))
                    .debug_selector(|| CONTEXT_MENU_TAG.into())
                    .w(px(MENU_WIDTH))
                    .gap(px(PANEL_GAP))
                    .occlude()
                    .on_mouse_down_out(move |_, window, cx| close_from_outside(window, cx))
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(
                        h_flex()
                            .debug_selector(|| format!("{CONTEXT_MENU_TAG}/reactions").into())
                            .self_start()
                            .p(px(PILL_PADDING))
                            .gap(px(1.))
                            .items_center()
                            .rounded_full()
                            .flyout_surface()
                            .children(reactions),
                    )
                    .child(
                        v_flex()
                            .debug_selector(|| format!("{CONTEXT_MENU_TAG}/list").into())
                            .w_full()
                            .p(px(LIST_PADDING))
                            .rounded(px(10.))
                            .flyout_surface()
                            .children(items),
                    ),
            ),
    )
    .with_priority(POPUP_PRIORITY)
    .into_any_element()
}
