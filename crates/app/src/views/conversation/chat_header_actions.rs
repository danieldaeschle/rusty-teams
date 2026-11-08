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
use crate::views::chat_menu::{ChatMenuTarget, MenuContext, chat_menu};
use crate::views::composer::plus_menu::chat_invitees;
use crate::views::new_meeting_dialog::open_new_meeting_with;
use crate::views::avatar::status_glyph;
use crate::views::widgets::{icon, menu_icon, symbol};

pub const CHAT_HEADER_MORE_TAG: &str = "chat-header-more";
const SCHEDULE_MEETING_LABEL: &str = "Schedule meeting";
const BUTTON_SIZE: f32 = 32.;
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

    pub(super) fn chat_more_button(&self, cx: &App) -> Option<AnyElement> {
        let chat_id = self
            .header_chat_id()
            .filter(|_| !self.popped_out && !self.pinned)?;
        let target = ChatMenuTarget::for_chat(self.app.read(cx), chat_id)?;
        let context = MenuContext::new(&self.app, cx);
        Some(
            Button::new(CHAT_HEADER_MORE_TAG)
                .icon(IconName::Ellipsis)
                .ghost()
                .size(px(BUTTON_SIZE))
                .rounded(px(8.))
                .tooltip("More chat options")
                .debug_selector(|| CHAT_HEADER_MORE_TAG.into())
                .dropdown_menu(move |popup, window, cx| {
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
