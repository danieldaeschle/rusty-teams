use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::GlobalState;
use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    popover::Popover,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::reaction_picker::{DoneHandler, PickHandler, ReactionPicker};
use super::toolbar_placement::{HoverChange, TOOLBAR_HEIGHT, toolbar_slot};
use super::widgets::{icon, symbol};
use crate::forward_selection::{FORWARD_LABEL, MULTIPLE_MESSAGES_LABEL, THIS_MESSAGE_LABEL};
use crate::post_follow::follow_label;
use crate::rows::reaction_glyph;
use crate::share_outlook::SHARE_TO_OUTLOOK_LABEL;
use crate::theme::{self, ElevatedSurface as _};

const TRANSLATION_LABEL: &str = "Translation";
const AGENTS_LABEL: &str = "Agents";
const BAR_BUTTON: f32 = 30.;
const BAR_PADDING: f32 = 3.;
const BAR_ICON: f32 = 16.;
const DELEGATE_ITEM_WIDTH: f32 = 188.;
const DELEGATE_AGENT_SIZE: f32 = 11.;
const ADD_REACTION_ICON: f32 = 15.;
const ADD_REACTION_EXTRA_WIDTH: f32 = 8.;

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
    pub forward_multiple: Option<Action>,
    pub copy_link: Option<Action>,
    pub copy: Action,
    pub translate: Option<Action>,
    pub translate_label: &'static str,
    pub share_to_outlook: Option<Action>,
    pub save: Option<Action>,
    pub saved: bool,
    pub toggle_pinned: Option<Action>,
    pub pinned: bool,
    pub toggle_follow: Option<Action>,
    pub followed: bool,
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

pub(super) fn reaction_button(menu: &MessageMenu, glyph: &str, index: usize) -> AnyElement {
    let (react, done, picked) = (menu.react.clone(), menu.done.clone(), glyph.to_owned());
    let selected = menu.mine.contains(&reaction_glyph(glyph));
    bar_button(element_id(&format!("quick-reaction-{index}"), &menu.key))
        .text_size(px(17.))
        .font_family(theme::EMOJI_FONT_FAMILY)
        .when(selected, |button| button.bg(theme::reaction_on_own()))
        .child(glyph.to_owned())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            react(&picked, window, cx);
            done(window, cx);
        })
        .into_any_element()
}

#[derive(Clone)]
pub struct ReactionAdder {
    pub key: String,
    pub picker: Entity<ReactionPicker>,
    pub react: PickHandler,
    pub done: DoneHandler,
    pub mine: HashSet<String>,
    pub pin: OpenChange,
}

impl MessageMenu {
    pub fn reaction_adder(&self) -> ReactionAdder {
        ReactionAdder {
            key: self.key.clone(),
            picker: self.picker.clone(),
            react: self.react.clone(),
            done: self.done.clone(),
            mine: self.mine.clone(),
            pin: self.pin.clone(),
        }
    }
}

fn picker_popover(
    adder: ReactionAdder,
    prefix: &str,
    trigger: Button,
    anchor: Anchor,
) -> AnyElement {
    let ReactionAdder {
        key,
        picker,
        react,
        done,
        mine,
        pin,
    } = adder;
    let picker_reset = picker.clone();
    Popover::new(element_id(prefix, &key))
        .anchor(anchor)
        .trigger(trigger)
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

pub(super) fn picker_button(menu: &MessageMenu, cx: &App) -> AnyElement {
    let trigger = Button::new(element_id("add-reaction", &menu.key))
        .child(icon(IconName::Plus, BAR_ICON, theme::text_soft()))
        .custom(bar_variant(cx))
        .rounded(px(7.))
        .size(px(BAR_BUTTON));
    picker_popover(
        menu.reaction_adder(),
        "reaction-picker",
        trigger,
        Anchor::TopRight,
    )
}

pub fn add_reaction_pill(adder: ReactionAdder, height: f32, cx: &App) -> AnyElement {
    let trigger = Button::new(element_id("pill-add-reaction", &adder.key))
        .child(symbol("mood", ADD_REACTION_ICON, theme::text_muted()))
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::border_strong())
                .foreground(theme::text_muted())
                .hover(theme::popup_hover())
                .active(theme::popup_hover()),
        )
        .rounded(px(height / 2.))
        .h(px(height))
        .w(px(height + ADD_REACTION_EXTRA_WIDTH));
    picker_popover(adder, "pill-reaction-picker", trigger, Anchor::TopLeft)
}

/// The actions of one message, shared by the toolbar's more menu and the right-click menu.
#[derive(Clone)]
pub struct MenuActions {
    reply: Option<Action>,
    forward: Option<Action>,
    forward_multiple: Option<Action>,
    copy_link: Option<Action>,
    copy: Action,
    translate: Option<Action>,
    translate_label: &'static str,
    save: Option<Action>,
    saved: bool,
    toggle_pinned: Option<Action>,
    pinned: bool,
    toggle_follow: Option<Action>,
    followed: bool,
    edit: Option<Action>,
    delete: Option<Action>,
    mark_unread: Option<Action>,
    refresh_card: Option<Action>,
    share_to_outlook: Option<Action>,
    delegate: Vec<DelegateEntry>,
    manage_agents: Option<Action>,
}

impl MenuActions {
    pub fn of(menu: &MessageMenu) -> Self {
        MenuActions {
            reply: menu.reply.clone(),
            forward: menu.forward.clone(),
            forward_multiple: menu.forward_multiple.clone(),
            copy_link: menu.copy_link.clone(),
            copy: menu.copy.clone(),
            translate: menu.translate.clone(),
            translate_label: menu.translate_label,
            save: menu.save.clone(),
            saved: menu.saved,
            toggle_pinned: menu.toggle_pinned.clone(),
            pinned: menu.pinned,
            toggle_follow: menu.toggle_follow.clone(),
            followed: menu.followed,
            edit: menu.edit.clone(),
            delete: menu.delete.clone(),
            mark_unread: menu.mark_unread.clone(),
            refresh_card: menu.refresh_card.clone(),
            share_to_outlook: menu.share_to_outlook.clone(),
            delegate: menu.delegate.clone(),
            manage_agents: menu.manage_agents.clone(),
        }
    }

    /// The right-click menu adds Reply on top and the agent actions as a submenu; the toolbar has its own buttons for both.
    pub fn fill(
        &self,
        popup: PopupMenu,
        right_click: bool,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let mut popup = popup;
        if right_click {
            if let Some(reply) = self.reply.clone() {
                popup = popup.item(
                    PopupMenuItem::new("Reply")
                        .icon(IconName::Reply)
                        .on_click(move |_, window, cx| reply(window, cx)),
                );
            }
            if !self.delegate.is_empty() {
                let (entries, manage_agents) = (self.delegate.clone(), self.manage_agents.clone());
                popup = popup.submenu_with_icon(
                    Some(IconName::Bot.into()),
                    AGENTS_LABEL,
                    window,
                    cx,
                    move |submenu, _, _| delegate_entries(submenu, &entries, manage_agents.clone()),
                );
            }
        }
        popup = self.fill_rest(popup, window, cx);
        popup
    }

    fn fill_rest(&self, popup: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
        let item = |label: &'static str, glyph: IconName, action: Action| {
            PopupMenuItem::new(label)
                .icon(glyph)
                .on_click(move |_, window, cx| action(window, cx))
        };
        let mut popup = popup;
        match (self.forward.clone(), self.forward_multiple.clone()) {
            (Some(forward), Some(forward_multiple)) => {
                popup = popup.submenu_with_icon(
                    Some(IconName::Forward.into()),
                    FORWARD_LABEL,
                    window,
                    cx,
                    move |submenu, _, _| {
                        let (single, multiple) = (forward.clone(), forward_multiple.clone());
                        submenu
                            .item(
                                PopupMenuItem::new(THIS_MESSAGE_LABEL)
                                    .on_click(move |_, window, cx| single(window, cx)),
                            )
                            .item(
                                PopupMenuItem::new(MULTIPLE_MESSAGES_LABEL)
                                    .on_click(move |_, window, cx| multiple(window, cx)),
                            )
                    },
                );
            }
            (Some(forward), None) => {
                popup = popup.item(item(FORWARD_LABEL, IconName::Forward, forward));
            }
            (None, _) => {}
        }
        if let Some(copy_link) = self.copy_link.clone() {
            popup = popup.item(item("Copy link", IconName::Link, copy_link));
        }
        popup = popup.item(item("Copy text", IconName::Copy, self.copy.clone()));
        if let Some(save) = self.save.clone() {
            popup = popup.item(if self.saved {
                item("Unsave this message", IconName::BookmarkOff, save)
            } else {
                item("Save this message", IconName::Bookmark, save)
            });
        }
        if let Some(toggle_pinned) = self.toggle_pinned.clone() {
            popup = popup.item(if self.pinned {
                item("Unpin", IconName::PinOff, toggle_pinned)
            } else {
                item("Pin for everyone", IconName::Pin, toggle_pinned)
            });
        }
        if let Some(edit) = self.edit.clone() {
            popup = popup.item(item("Edit", IconName::Pencil, edit));
        }
        if let Some(toggle_follow) = self.toggle_follow.clone() {
            popup = popup.item(if self.followed {
                item(follow_label(true), IconName::BellOff, toggle_follow)
            } else {
                item(follow_label(false), IconName::Bell, toggle_follow)
            });
        }
        if let Some(refresh_card) = self.refresh_card.clone() {
            popup = popup.item(item("Refresh card", IconName::RefreshCw, refresh_card));
        }
        popup = popup.separator();
        if let Some(mark_unread) = self.mark_unread.clone() {
            popup = popup.item(item("Mark as unread", IconName::EyeOff, mark_unread));
        }
        if let Some(share_to_outlook) = self.share_to_outlook.clone() {
            popup = popup.item(item(
                SHARE_TO_OUTLOOK_LABEL,
                IconName::Mail,
                share_to_outlook,
            ));
        }
        let translate_label = self.translate_label;
        if let Some(translate) = self.translate.clone() {
            popup = popup.submenu_with_icon(
                Some(IconName::Languages.into()),
                TRANSLATION_LABEL,
                window,
                cx,
                move |submenu, _, _| {
                    let translate = translate.clone();
                    submenu.item(
                        PopupMenuItem::new(translate_label)
                            .on_click(move |_, window, cx| translate(window, cx)),
                    )
                },
            );
        }
        if let Some(delete) = self.delete.clone() {
            popup = popup
                .separator()
                .item(item("Delete", IconName::Trash, delete));
        }
        popup
    }
}

fn more_button(menu: &MessageMenu, cx: &App) -> AnyElement {
    let actions = MenuActions::of(menu);
    let pin = menu.pin.clone();
    Button::new(element_id("message-more", &menu.key))
        .child(icon(IconName::Ellipsis, BAR_ICON, theme::text_soft()))
        .custom(bar_variant(cx))
        .rounded(px(7.))
        .size(px(BAR_BUTTON))
        .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, window, cx| {
            actions.fill(popup, false, window, cx)
        })
        .on_open_change(move |open, window, cx| pin(*open, window, cx))
        .into_any_element()
}

fn delegate_entries(popup: PopupMenu, entries: &[DelegateEntry], manage_agents: Option<Action>) -> PopupMenu {
    let mut popup = popup;
    for entry in entries {
        popup = popup.item(delegate_item(entry));
    }
    if let Some(manage_agents) = manage_agents {
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
            delegate_entries(popup, &entries, manage_agents.clone())
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
