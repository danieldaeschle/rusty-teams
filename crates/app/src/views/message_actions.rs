use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::GlobalState;
use gpui_kit::component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    popover::Popover,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::reaction_picker::{DoneHandler, PickHandler, ReactionPicker};
use super::widgets::icon;
use crate::rows::reaction_glyph;
use crate::theme;

const BAR_BUTTON: f32 = 28.;
pub const QUICK_REACTION_COUNT: usize = 4;

pub type Action = Rc<dyn Fn(&mut Window, &mut App)>;
pub type OpenChange = Rc<dyn Fn(bool, &mut Window, &mut App)>;
pub type HoverChange = Rc<dyn Fn(bool, &mut App)>;

pub struct MessageMenu {
    pub key: String,
    pub react: PickHandler,
    pub reply: Option<Action>,
    pub forward: Option<Action>,
    pub copy_link: Option<Action>,
    pub copy: Action,
    pub translate: Option<Action>,
    pub translate_label: &'static str,
    pub save: Option<Action>,
    pub saved: bool,
    pub toggle_pinned: Option<Action>,
    pub pinned: bool,
    pub edit: Option<Action>,
    pub delete: Option<Action>,
    pub mark_unread: Option<Action>,
    pub refresh_card: Option<Action>,
    pub pin: OpenChange,
    pub hover: HoverChange,
    pub picker: Entity<ReactionPicker>,
    pub quick: Vec<String>,
    pub mine: HashSet<String>,
    pub done: DoneHandler,
}

fn element_id(prefix: &str, key: &str) -> ElementId {
    ElementId::Name(format!("{prefix}-{key}").into())
}

pub(super) fn bar_button(id: ElementId) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(BAR_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|button| button.bg(theme::border_strong()))
}

fn reaction_button(menu: &MessageMenu, glyph: &str, index: usize) -> AnyElement {
    let (react, done, picked) = (menu.react.clone(), menu.done.clone(), glyph.to_owned());
    let selected = menu.mine.contains(&reaction_glyph(glyph));
    bar_button(element_id(&format!("quick-reaction-{index}"), &menu.key))
        .text_size(px(17.))
        .when(selected, |button| button.bg(theme::reaction_on_own()))
        .child(glyph.to_owned())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            react(&picked, window, cx);
            done(window, cx);
        })
        .into_any_element()
}

fn picker_button(menu: &MessageMenu) -> AnyElement {
    let (picker, picker_reset, react, done, mine, pin) = (
        menu.picker.clone(),
        menu.picker.clone(),
        menu.react.clone(),
        menu.done.clone(),
        menu.mine.clone(),
        menu.pin.clone(),
    );
    Popover::new(element_id("reaction-picker", &menu.key))
        .anchor(Anchor::TopRight)
        .trigger(
            Button::new(element_id("add-reaction", &menu.key))
                .icon(IconName::Plus)
                .ghost()
                .xsmall()
                .size(px(BAR_BUTTON)),
        )
        .on_open_change(move |open, window, cx| {
            pin(*open, window, cx);
            if *open {
                picker_reset.update(cx, |picker, cx| picker.reset(window, cx));
            }
        })
        .content(move |_, _, cx| {
            let popover = cx.entity().downgrade();
            picker.update(cx, |picker, _| {
                picker.set_target(react.clone(), done.clone(), popover, mine.clone())
            });
            picker.clone()
        })
        .into_any_element()
}

fn more_button(menu: &MessageMenu) -> AnyElement {
    let (
        reply,
        forward,
        copy_link,
        copy,
        translate,
        save,
        toggle_pinned,
        edit,
        delete,
        mark_unread,
        refresh_card,
        pin,
    ) = (
        menu.reply.clone(),
        menu.forward.clone(),
        menu.copy_link.clone(),
        menu.copy.clone(),
        menu.translate.clone(),
        menu.save.clone(),
        menu.toggle_pinned.clone(),
        menu.edit.clone(),
        menu.delete.clone(),
        menu.mark_unread.clone(),
        menu.refresh_card.clone(),
        menu.pin.clone(),
    );
    let (saved, pinned, translate_label) = (menu.saved, menu.pinned, menu.translate_label);
    Button::new(element_id("message-more", &menu.key))
        .icon(IconName::Ellipsis)
        .ghost()
        .xsmall()
        .size(px(BAR_BUTTON))
        .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, _, _| {
            let item = |label: &'static str, glyph: IconName, action: Action| {
                PopupMenuItem::new(label)
                    .icon(glyph)
                    .on_click(move |_, window, cx| action(window, cx))
            };
            let mut popup = popup;
            if let Some(reply) = reply.clone() {
                popup = popup.item(item("Reply with quote", IconName::Reply, reply));
            }
            if let Some(forward) = forward.clone() {
                popup = popup.item(item("Forward", IconName::Forward, forward));
            }
            if let Some(copy_link) = copy_link.clone() {
                popup = popup.item(item("Copy link", IconName::Link, copy_link));
            }
            if let Some(refresh_card) = refresh_card.clone() {
                popup = popup.item(item("Refresh card", IconName::RefreshCw, refresh_card));
            }
            popup = popup.item(item("Copy text", IconName::Copy, copy.clone()));
            if let Some(translate) = translate.clone() {
                popup = popup.item(item(translate_label, IconName::Languages, translate));
            }
            if let Some(save) = save.clone() {
                popup = popup.item(if saved {
                    item("Unsave message", IconName::BookmarkOff, save)
                } else {
                    item("Save message", IconName::Bookmark, save)
                });
            }
            if let Some(toggle_pinned) = toggle_pinned.clone() {
                popup = popup.item(if pinned {
                    item("Unpin", IconName::PinOff, toggle_pinned)
                } else {
                    item("Pin", IconName::Pin, toggle_pinned)
                });
            }
            if let Some(edit) = edit.clone() {
                popup = popup.item(item("Edit", IconName::Pencil, edit));
            }
            if let Some(delete) = delete.clone() {
                popup = popup
                    .separator()
                    .item(item("Delete", IconName::Trash, delete));
            }
            if let Some(mark_unread) = mark_unread.clone() {
                let entry = item("Mark as unread", IconName::EyeOff, mark_unread);
                popup = popup.separator().item(entry);
            }
            popup
        })
        .on_open_change(move |open, window, cx| pin(*open, window, cx))
        .into_any_element()
}

pub fn message_toolbar(menu: MessageMenu) -> AnyElement {
    let hover = menu.hover.clone();
    let mut children: Vec<AnyElement> = menu
        .quick
        .iter()
        .enumerate()
        .map(|(index, glyph)| reaction_button(&menu, glyph, index))
        .collect();
    children.push(picker_button(&menu));
    children.push(
        div()
            .w(px(1.))
            .h(px(18.))
            .mx(px(2.))
            .bg(theme::border_strong())
            .into_any_element(),
    );
    if let Some(reply) = menu.reply.clone() {
        children.push(
            bar_button(element_id("toolbar-reply", &menu.key))
                .child(icon(IconName::Reply, 16., theme::text_soft()))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    reply(window, cx);
                })
                .into_any_element(),
        );
    }
    children.push(more_button(&menu));
    h_flex()
        .id(element_id("message-toolbar", &menu.key))
        .p(px(2.))
        .gap(px(2.))
        .items_center()
        .rounded(px(8.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border_strong())
        .occlude()
        .on_hover(move |hovered, _, cx| hover(*hovered, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            GlobalState::suppress_text_selection(cx)
        })
        .children(children)
        .into_any_element()
}
