use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::*;

use super::widgets::{icon, menu_icon};
use crate::app_state::{AppState, Selection};
use crate::call::{is_echo_chat, plan_for_chat};
use crate::theme;

const BUTTON_SIZE: f32 = 30.;
const GLYPH_SIZE: f32 = 16.;
const JOIN_HEIGHT: f32 = 30.;
const CHEVRON_WIDTH: f32 = 18.;
const CHEVRON_GLYPH: f32 = 12.;
pub const HEADER_VIDEO_CALL_TAG: &str = "header-video-call";
pub const HEADER_CALL_OPTIONS_TAG: &str = "header-call-options";
const VIDEO_CALL_LABEL: &str = "Video call";
const AUDIO_CALL_LABEL: &str = "Audio call";
const CALL_OPTIONS_LABEL: &str = "Call options";

pub fn header_call_controls(
    app: &Entity<AppState>,
    state: &AppState,
    selection: &Selection,
) -> Vec<AnyElement> {
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
            .is_some_and(|chat| {
                is_echo_chat(chat) || plan_for_chat(chat, state.directory.me.as_ref()).is_some()
            }));
    if callable {
        controls.push(video_split_button(app));
    }
    controls
}

fn start_call(app: &Entity<AppState>, with_video: bool, cx: &mut App) {
    app.update(cx, |state, cx| {
        if let Some(Selection::Chat(chat_id)) = state.selection.clone() {
            if with_video {
                state.start_chat_video_call(&chat_id, cx);
            } else {
                state.start_chat_call(&chat_id, cx);
            }
        }
    });
}

fn video_split_button(app: &Entity<AppState>) -> AnyElement {
    let video_app = app.clone();
    let menu_app = app.clone();
    h_flex()
        .flex_none()
        .items_center()
        .child(
            Button::new(HEADER_VIDEO_CALL_TAG)
                .ghost()
                .p_0()
                .size(px(BUTTON_SIZE))
                .tooltip(VIDEO_CALL_LABEL)
                .debug_selector(|| HEADER_VIDEO_CALL_TAG.into())
                .child(icon(IconName::Video, GLYPH_SIZE, theme::text_soft()))
                .on_click(move |_, _, cx| start_call(&video_app, true, cx)),
        )
        .child(
            Button::new(HEADER_CALL_OPTIONS_TAG)
                .ghost()
                .p_0()
                .w(px(CHEVRON_WIDTH))
                .h(px(BUTTON_SIZE))
                .tooltip(CALL_OPTIONS_LABEL)
                .debug_selector(|| HEADER_CALL_OPTIONS_TAG.into())
                .child(icon(IconName::ChevronDown, CHEVRON_GLYPH, theme::text_soft()))
                .dropdown_menu(move |popup, _, _| {
                    let (video, audio) = (menu_app.clone(), menu_app.clone());
                    popup
                        .item(
                            PopupMenuItem::new(VIDEO_CALL_LABEL)
                                .icon(menu_icon(IconName::Video))
                                .on_click(move |_, _, cx| start_call(&video, true, cx)),
                        )
                        .item(
                            PopupMenuItem::new(AUDIO_CALL_LABEL)
                                .icon(menu_icon(IconName::Phone))
                                .on_click(move |_, _, cx| start_call(&audio, false, cx)),
                        )
                }),
        )
        .into_any_element()
}
