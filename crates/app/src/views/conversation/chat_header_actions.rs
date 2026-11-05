use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ConversationView;
use crate::app_state::Selection;
use crate::data::Directory;
use crate::find_in_chat::FIND_IN_CHAT;
use crate::theme;
use crate::views::chat_pop_out::{self, POP_OUT_CHAT_LABEL};
use crate::views::avatar::status_glyph;
use crate::views::widgets::{icon, symbol};

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

    pub(super) fn pop_out_button(&self) -> Option<AnyElement> {
        let chat_id = self
            .header_chat_id()
            .filter(|_| !self.popped_out && !self.pinned)?
            .to_owned();
        let app = self.app.clone();
        Some(
            Button::new("chat-pop-out")
                .icon(IconName::ExternalLink)
                .ghost()
                .size(px(BUTTON_SIZE))
                .rounded(px(8.))
                .tooltip(POP_OUT_CHAT_LABEL)
                .on_click(move |_, _, cx| chat_pop_out::pop_out_chat(&app, &chat_id, cx))
                .into_any_element(),
        )
    }
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
