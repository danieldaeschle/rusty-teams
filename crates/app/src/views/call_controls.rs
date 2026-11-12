use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::shortcuts::{StartAudioCall, StartVideoCall};
use super::widgets::{icon, menu_icon};
use crate::app_state::{AppState, Selection};
use crate::call::{is_echo_chat, plan_for_chat};
use crate::i18n::tr;
use crate::theme;

const BUTTON_SIZE: f32 = 30.;
const GLYPH_SIZE: f32 = 16.;
const JOIN_HEIGHT: f32 = 30.;
const CHEVRON_WIDTH: f32 = 18.;
const CHEVRON_GLYPH: f32 = 12.;
pub const HEADER_VIDEO_CALL_TAG: &str = "header-video-call";
pub const HEADER_CALL_OPTIONS_TAG: &str = "header-call-options";
const GROUP_KIND: &str = "group";
const MEETING_KIND: &str = "meeting";

pub fn header_call_controls(
    app: &Entity<AppState>,
    state: &AppState,
    selection: &Selection,
    compact: bool,
) -> Vec<AnyElement> {
    let conversation_id = selection.conversation_id().to_owned();
    let mut controls = Vec::new();
    let chat = match selection {
        Selection::Chat(chat_id) => state.sidebar.chats.iter().find(|chat| &chat.id == chat_id),
        Selection::Channel(_) => None,
    };
    let running = state.running_meeting(&conversation_id).is_some();
    let meeting_chat = chat.is_some_and(|chat| chat.kind.eq_ignore_ascii_case(MEETING_KIND));
    if running || meeting_chat {
        let join_app = app.clone();
        let join_id = conversation_id.clone();
        let (background, foreground) = if running {
            (theme::green(), theme::white())
        } else {
            (theme::accent(), theme::on_accent())
        };
        controls.push(
            h_flex()
                .id("header-join")
                .h(px(JOIN_HEIGHT))
                .px(px(14.))
                .gap(px(6.))
                .flex_none()
                .items_center()
                .rounded_full()
                .bg(background)
                .text_color(foreground)
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|button| button.opacity(0.85))
                .when(running, |button| {
                    button.child(icon(IconName::Phone, 14., foreground))
                })
                .child(tr("common.join"))
                .on_click(move |_, _, cx| {
                    join_app.update(cx, |state, cx| state.join_chat_meeting(&join_id, cx));
                })
                .into_any_element(),
        );
    }
    let callable = chat.is_some_and(|chat| {
        is_echo_chat(chat) || plan_for_chat(chat, state.directory.me.as_ref()).is_some()
    });
    if let Some(chat) = chat.filter(|_| callable) {
        let meet_now = chat.kind == GROUP_KIND && !is_echo_chat(chat);
        controls.push(video_split_button(app, meet_now, compact));
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

fn video_split_button(app: &Entity<AppState>, meet_now: bool, compact: bool) -> AnyElement {
    let video_app = app.clone();
    let menu_app = app.clone();
    let labelled = meet_now && !compact;
    h_flex()
        .flex_none()
        .items_center()
        .child(
            Button::new(HEADER_VIDEO_CALL_TAG)
                .ghost()
                .h(px(BUTTON_SIZE))
                .map(|button| {
                    if labelled {
                        button.px(px(8.))
                    } else {
                        button.p_0().w(px(BUTTON_SIZE))
                    }
                })
                .tooltip(if meet_now {
                    tr("calls.chrome.meet_now")
                } else {
                    tr("calls.chrome.video_call")
                })
                .debug_selector(|| HEADER_VIDEO_CALL_TAG.into())
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(icon(IconName::Video, GLYPH_SIZE, theme::text_soft()))
                        .when(labelled, |row| {
                            row.child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child(tr("calls.chrome.meet_now")),
                            )
                        }),
                )
                .on_click(move |_, _, cx| start_call(&video_app, true, cx)),
        )
        .child(
            Button::new(HEADER_CALL_OPTIONS_TAG)
                .ghost()
                .p_0()
                .w(px(CHEVRON_WIDTH))
                .h(px(BUTTON_SIZE))
                .tooltip(tr("calls.chrome.call_options"))
                .debug_selector(|| HEADER_CALL_OPTIONS_TAG.into())
                .child(icon(
                    IconName::ChevronDown,
                    CHEVRON_GLYPH,
                    theme::text_soft(),
                ))
                .dropdown_menu(move |popup, _, _| {
                    let (video, audio) = (menu_app.clone(), menu_app.clone());
                    let popup = if meet_now {
                        popup
                    } else {
                        popup.item(
                            PopupMenuItem::new(tr("calls.chrome.video_call"))
                                .icon(menu_icon(IconName::Video))
                                .action(Box::new(StartVideoCall))
                                .on_click(move |_, _, cx| start_call(&video, true, cx)),
                        )
                    };
                    popup.item(
                        PopupMenuItem::new(tr("calls.chrome.audio_call"))
                            .icon(menu_icon(IconName::Phone))
                            .action(Box::new(StartAudioCall))
                            .on_click(move |_, _, cx| start_call(&audio, false, cx)),
                    )
                }),
        )
        .into_any_element()
}
