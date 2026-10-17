use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::GlobalState;
use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    popover::Popover,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::reaction_picker::{DoneHandler, PickHandler, ReactionPicker};
use super::toolbar_placement::{HoverChange, TOOLBAR_HEIGHT, toolbar_slot};
use super::widgets::icon;
use crate::rows::reaction_glyph;
use crate::theme::{self, ElevatedSurface as _};

const BAR_BUTTON: f32 = 30.;
const BAR_PADDING: f32 = 3.;
const BAR_ICON: f32 = 16.;
const DELEGATE_ITEM_WIDTH: f32 = 188.;
const DELEGATE_AGENT_SIZE: f32 = 11.;

pub type Action = Rc<dyn Fn(&mut Window, &mut App)>;
pub type OpenChange = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct DelegateEntry {
    pub label: String,
    pub agent_name: String,
    pub action: Action,
}

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
    pub delegate: Vec<DelegateEntry>,
    pub manage_agents: Option<Action>,
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

fn bar_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(theme::text_soft())
        .hover(theme::popup_hover())
        .active(theme::popup_hover())
}

pub(super) fn bar_button(id: ElementId) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(BAR_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .cursor_pointer()
        .hover(|button| button.bg(theme::popup_hover()))
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

fn picker_button(menu: &MessageMenu, cx: &App) -> AnyElement {
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
                .child(icon(IconName::Plus, BAR_ICON, theme::text_soft()))
                .custom(bar_variant(cx))
                .rounded(px(7.))
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

fn more_button(menu: &MessageMenu, cx: &App) -> AnyElement {
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
        .child(icon(IconName::Ellipsis, BAR_ICON, theme::text_soft()))
        .custom(bar_variant(cx))
        .rounded(px(7.))
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

fn delegate_item(entry: &DelegateEntry) -> PopupMenuItem {
    let (label, agent_name, action) = (
        entry.label.clone(),
        entry.agent_name.clone(),
        entry.action.clone(),
    );
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .w(px(DELEGATE_ITEM_WIDTH))
            .gap(px(12.))
            .items_center()
            .justify_between()
            .child(div().flex_1().min_w_0().truncate().child(label.clone()))
            .child(
                div()
                    .flex_none()
                    .text_size(px(DELEGATE_AGENT_SIZE))
                    .text_color(theme::text_faint())
                    .child(agent_name.clone()),
            )
    })
    .icon(IconName::Bot)
    .on_click(move |_, window, cx| action(window, cx))
}

fn delegate_button(menu: &MessageMenu, cx: &App) -> AnyElement {
    let (entries, manage_agents, pin) = (
        menu.delegate.clone(),
        menu.manage_agents.clone(),
        menu.pin.clone(),
    );
    Button::new(element_id("message-delegate", &menu.key))
        .child(icon(IconName::Bot, BAR_ICON, theme::text_soft()))
        .custom(bar_variant(cx))
        .rounded(px(7.))
        .size(px(BAR_BUTTON))
        .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, _, _| {
            let mut popup = popup;
            for entry in &entries {
                popup = popup.item(delegate_item(entry));
            }
            if let Some(manage_agents) = manage_agents.clone() {
                popup = popup.separator().item(
                    PopupMenuItem::element(|_, _| {
                        div()
                            .text_color(theme::text_muted())
                            .child("Manage actions...")
                    })
                    .on_click(move |_, window, cx| manage_agents(window, cx)),
                );
            }
            popup
        })
        .on_open_change(move |open, window, cx| pin(*open, window, cx))
        .into_any_element()
}

pub fn message_toolbar(menu: MessageMenu, cx: &App) -> AnyElement {
    let hover = menu.hover.clone();
    let mut children: Vec<AnyElement> = menu
        .quick
        .iter()
        .enumerate()
        .map(|(index, glyph)| reaction_button(&menu, glyph, index))
        .collect();
    children.push(picker_button(&menu, cx));
    children.push(
        div()
            .w(px(1.))
            .h_full()
            .mx(px(4.))
            .flex_none()
            .bg(theme::popup_hover())
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
    if !menu.delegate.is_empty() {
        children.push(delegate_button(&menu, cx));
    }
    children.push(more_button(&menu, cx));
    let bar = h_flex()
        .h(px(TOOLBAR_HEIGHT))
        .px(px(BAR_PADDING))
        .gap(px(1.))
        .items_center()
        .rounded(px(10.))
        .flyout_surface()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            GlobalState::suppress_text_selection(cx)
        })
        .children(children);
    toolbar_slot(element_id("message-toolbar", &menu.key), hover, bar).into_any_element()
}
