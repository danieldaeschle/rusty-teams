use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
};
use gpui_kit::*;

use super::widgets::icon;
use crate::app_state::{AppState, Selection};
use crate::call::plan_for_chat;
use crate::theme;

const BUTTON_SIZE: f32 = 30.;
const GLYPH_SIZE: f32 = 16.;
const JOIN_HEIGHT: f32 = 30.;

pub fn header_call_controls(app: &Entity<AppState>, state: &AppState, selection: &Selection) -> Vec<AnyElement> {
    let conversation_id = selection.conversation_id().to_owned();
    let mut controls = Vec::new();
    if state.running_meeting(&conversation_id).is_some() {
        let join_app = app.clone();
        let join_id = conversation_id.clone();
        controls.push(
            h_flex()
                .id("header-join")
                .h(px(JOIN_HEIGHT))
                .px(px(14.))
                .gap(px(6.))
                .flex_none()
                .items_center()
                .rounded_full()
                .bg(theme::green())
                .text_color(theme::white())
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|button| button.opacity(0.85))
                .child(icon(IconName::Phone, 14., theme::white()))
                .child("Join")
                .on_click(move |_, _, cx| {
                    join_app.update(cx, |state, cx| state.join_meeting_call(&join_id, cx));
                })
                .into_any_element(),
        );
    }
    let callable = matches!(selection, Selection::Chat(chat_id)
        if state
            .sidebar
            .chats
            .iter()
            .find(|chat| &chat.id == chat_id)
            .is_some_and(|chat| plan_for_chat(chat, state.directory.me.as_ref()).is_some()));
    if callable {
        let call_app = app.clone();
        controls.push(
            Button::new("header-call")
                .ghost()
                .p_0()
                .size(px(BUTTON_SIZE))
                .tooltip("Call")
                .child(icon(IconName::Phone, GLYPH_SIZE, theme::text_soft()))
                .on_click(move |_, _, cx| {
                    call_app.update(cx, |state, cx| {
                        if let Some(Selection::Chat(chat_id)) = state.selection.clone() {
                            state.start_chat_call(&chat_id, cx);
                        }
                    });
                })
                .into_any_element(),
        );
    }
    controls
}
