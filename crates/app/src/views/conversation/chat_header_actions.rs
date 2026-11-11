use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use teams_core::is_notes_chat;

use super::ConversationView;
use crate::app_state::Selection;
use crate::data::Directory;
use crate::find_in_chat::FIND_IN_CHAT;
use crate::theme;
use crate::views::avatar::status_glyph;
use crate::views::chat_menu::{ChatMenuTarget, MenuContext, chat_menu};
use crate::views::composer::plus_menu::chat_invitees;
use crate::views::new_meeting_dialog::open_new_meeting_with;
use crate::views::sidebar::channel_menu;
use crate::views::widgets::{icon, menu_icon, symbol};

pub const CHAT_HEADER_MORE_TAG: &str = "chat-header-more";
pub const CHANNEL_HEADER_MORE_TAG: &str = "channel-header-more";
const SCHEDULE_MEETING_LABEL: &str = "Schedule meeting";
const BUTTON_SIZE: f32 = 32.;
const HEADER_COMPACT_WIDTH: f32 = 480.;
const COUNT_GLYPH: f32 = 16.;

impl ConversationView {
    fn header_chat_id(&self) -> Option<&str> {
        match &self.current.as_ref()?.selection {
            Selection::Chat(chat_id) if !self.draft_active => Some(chat_id),
            _ => None,
        }
    }

    pub(super) fn chat_find_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.header_chat_id().filter(|_| !self.pinned)?;
        Some(
            Button::new("chat-find")
                .icon(IconName::Search)
                .ghost()
                .size(px(BUTTON_SIZE))
                .rounded(px(8.))
                .tooltip(FIND_IN_CHAT)
                .on_click(cx.listener(|this, _, window, cx| this.open_find(window, cx)))
                .into_any_element(),
        )
    }

    pub(super) fn channel_more_button(&self, cx: &App) -> Option<AnyElement> {
        let Selection::Channel(channel_id) = &self.current.as_ref()?.selection else {
            return None;
        };
        if self.pinned || self.popped_out {
            return None;
        }
        let notifications = self
            .app
            .read(cx)
            .sidebar
            .teams
            .iter()
            .find(|entry| {
                entry
                    .channels
                    .iter()
                    .any(|channel| &channel.id == channel_id)
            })?
            .channel_notifications(channel_id);
        let (state, channel_id) = (self.app.clone(), channel_id.clone());
        Some(
            Button::new(CHANNEL_HEADER_MORE_TAG)
                .icon(IconName::Ellipsis)
                .ghost()
                .size(px(BUTTON_SIZE))
                .rounded(px(8.))
                .tooltip("More channel options")
                .debug_selector(|| CHANNEL_HEADER_MORE_TAG.into())
                .dropdown_menu(move |popup, window, cx| {
                    channel_menu(popup, window, cx, state.clone(), &channel_id, notifications)
                })
                .into_any_element(),
        )
    }

    pub(super) fn header_overflows(&self) -> bool {
        let width = self.header_width.get();
        width > 0.
            && width < HEADER_COMPACT_WIDTH
            && !self.popped_out
            && !self.pinned
            && self.header_chat_id().is_some()
    }

    pub(super) fn chat_more_button(
        &self,
        overflow: bool,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let chat_id = self
            .header_chat_id()
            .filter(|_| !self.popped_out && !self.pinned)?;
        let target = ChatMenuTarget::for_chat(self.app.read(cx), chat_id)?;
        let context = MenuContext::new(&self.app, cx);
        let view = cx.entity().downgrade();
        let (open_details, close_details) = self.details_labels();
        let details_label = if self.channel_details_open {
            close_details
        } else {
            open_details
        };
        let copilot_label = self.copilot_label(cx);
        Some(
            Button::new(CHAT_HEADER_MORE_TAG)
                .icon(IconName::Ellipsis)
                .ghost()
                .size(px(BUTTON_SIZE))
                .rounded(px(8.))
                .tooltip("More chat options")
                .debug_selector(|| CHAT_HEADER_MORE_TAG.into())
                .dropdown_menu(move |popup, window, cx| {
                    let popup = if overflow {
                        overflow_items(popup, view.clone(), copilot_label.clone(), details_label)
                    } else {
                        popup
                    };
                    let popup = if is_notes_chat(&target.id) {
                        popup
                    } else {
                        schedule_meeting_item(popup, &context, &target.id)
                    };
                    chat_menu(popup, window, cx, &context, &target)
                })
                .into_any_element(),
        )
    }
}

fn overflow_items(
    popup: PopupMenu,
    view: WeakEntity<ConversationView>,
    copilot_label: String,
    details_label: &'static str,
) -> PopupMenu {
    let (find_view, details_view) = (view.clone(), view.clone());
    popup
        .item(
            PopupMenuItem::new(FIND_IN_CHAT)
                .icon(menu_icon(IconName::Search))
                .on_click(move |_, window, cx| {
                    find_view
                        .update(cx, |this, cx| this.open_find(window, cx))
                        .ok();
                }),
        )
        .item(
            PopupMenuItem::new(copilot_label)
                .icon(menu_icon(IconName::Bot))
                .on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| this.toggle_copilot(window, cx))
                        .ok();
                }),
        )
        .item(
            PopupMenuItem::new(details_label)
                .icon(menu_icon(IconName::PanelRight))
                .on_click(move |_, _, cx| {
                    details_view
                        .update(cx, |this, cx| this.toggle_details(cx))
                        .ok();
                }),
        )
        .separator()
}

fn schedule_meeting_item(popup: PopupMenu, context: &MenuContext, chat_id: &str) -> PopupMenu {
    let (state, chat_id) = (context.state.clone(), chat_id.to_owned());
    popup
        .item(
            PopupMenuItem::new(SCHEDULE_MEETING_LABEL)
                .icon(menu_icon(IconName::Calendar))
                .on_click(move |_, window, cx| {
                    let invitees = chat_invitees(state.read(cx), &chat_id);
                    open_new_meeting_with(state.clone(), invitees, window, cx);
                }),
        )
        .separator()
}

pub fn member_count(count: usize) -> Div {
    h_flex()
        .gap(px(4.))
        .items_center()
        .text_size(px(13.))
        .text_color(theme::text_soft())
        .child(icon(IconName::Users, COUNT_GLYPH, theme::text_soft()))
        .child(count.to_string())
}

const TITLE_NAMES: usize = 3;
const NAME_DOT: f32 = 10.;

pub fn group_title(directory: &Directory, faces: &[(Option<String>, String)]) -> Div {
    let hidden = faces.len().saturating_sub(TITLE_NAMES);
    h_flex()
        .min_w_0()
        .gap(px(6.))
        .items_center()
        .children(faces.iter().take(TITLE_NAMES).map(|(user_id, name)| {
            let first = name.split_whitespace().next().unwrap_or(name).to_owned();
            let dot = user_id
                .as_deref()
                .and_then(|id| status_glyph(directory.presence_of(id).kind()))
                .map(|(glyph, color)| symbol(glyph, NAME_DOT, color));
            h_flex()
                .flex_none()
                .gap(px(3.))
                .items_center()
                .child(first)
                .children(dot)
        }))
        .when(hidden > 0, |row| row.child(format!("+{hidden}")))
}
