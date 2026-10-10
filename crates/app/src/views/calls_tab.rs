use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::join_dialog::open_join_with_id;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{CallRow, call_rows};
use crate::theme;

const ROW_HEIGHT: f32 = 54.;
const AVATAR_SIZE: f32 = 36.;
const MENU_ICON_SIZE: f32 = 15.;
const EMPTY_TEXT: &str = "No calls yet";

fn meeting_avatar() -> Div {
    div()
        .size(px(AVATAR_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::border_strong())
        .child(icon(IconName::Video, 16., theme::text()))
}

fn call_row(
    state: &Entity<AppState>,
    row: &CallRow,
    can_open_chat: bool,
    directory: &crate::data::Directory,
) -> AnyElement {
    let avatar = if row.meeting {
        meeting_avatar().into_any_element()
    } else {
        person_avatar(directory, row.user_id.as_deref(), &row.title, AVATAR_SIZE)
    };
    let title_color = if row.missed {
        theme::red_soft()
    } else {
        theme::text()
    };
    let click_state = state.clone();
    let click_row = row.clone();
    let menu_state = state.clone();
    let menu_row = row.clone();
    let can_call_back = !row.meeting && row.user_id.is_some();
    h_flex()
        .id(SharedString::from(format!("call-{}", row.key)))
        .mx(px(8.))
        .h(px(ROW_HEIGHT))
        .px(px(8.))
        .gap(px(10.))
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .child(avatar)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_baseline()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(14.))
                                .text_color(title_color)
                                .child(row.title.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::text_muted())
                                .child(row.time_label.clone()),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(if row.missed {
                            theme::red_soft()
                        } else {
                            theme::text_muted()
                        })
                        .child(row.subline.clone()),
                ),
        )
        .on_click(move |_, _, cx| {
            click_state.update(cx, |state, cx| state.activate_call_row(&click_row, cx));
        })
        .context_menu(move |popup, _, _| {
            let call_state = menu_state.clone();
            let call_row = menu_row.clone();
            let chat_state = menu_state.clone();
            let chat_row = menu_row.clone();
            popup
                .when(can_call_back, |popup| {
                    popup.item(
                        PopupMenuItem::new("Call back")
                            .icon(icon(IconName::Phone, MENU_ICON_SIZE, theme::text_muted()))
                            .on_click(move |_, _, cx| {
                                call_state.update(cx, |state, cx| {
                                    state.call_back_from_history(&call_row, cx)
                                });
                            }),
                    )
                })
                .when(can_open_chat, |popup| {
                    popup.item(
                        PopupMenuItem::new("Open chat")
                            .icon(icon(
                                IconName::MessageSquare,
                                MENU_ICON_SIZE,
                                theme::text_muted(),
                            ))
                            .on_click(move |_, _, cx| {
                                chat_state.update(cx, |state, cx| {
                                    state.open_chat_of_call_row(&chat_row, cx)
                                });
                            }),
                    )
                })
        })
        .into_any_element()
}

fn join_buttons(state: &Entity<AppState>) -> impl IntoElement {
    let id_state = state.clone();
    let paste_state = state.clone();
    h_flex()
        .w_full()
        .px(px(12.))
        .pb(px(8.))
        .gap(px(8.))
        .child(
            Button::new("join-with-id")
                .small()
                .label("Join with ID")
                .on_click(move |_, window, cx| open_join_with_id(id_state.clone(), window, cx)),
        )
        .child(
            Button::new("paste-meeting-link")
                .small()
                .label("Paste a meeting link")
                .on_click(move |_, _, cx| {
                    paste_state.update(cx, |state, cx| state.join_meeting_from_clipboard(cx));
                }),
        )
}

pub fn calls_body(state: &Entity<AppState>, scroll: &ScrollHandle, cx: &App) -> AnyElement {
    let app = state.read(cx);
    let offset = Local::now().offset().fix();
    let rows = call_rows(&app.call_history, &app.sidebar.chats, Utc::now(), offset);
    let list = if rows.is_empty() {
        div()
            .w_full()
            .py(px(24.))
            .flex()
            .justify_center()
            .text_size(px(13.))
            .text_color(theme::text_muted())
            .child(EMPTY_TEXT)
            .into_any_element()
    } else {
        v_flex()
            .w_full()
            .pb(px(12.))
            .children(rows.iter().map(|row| {
                call_row(
                    state,
                    row,
                    app.can_open_chat_of_call_row(row),
                    &app.directory,
                )
            }))
            .into_any_element()
    };
    v_flex()
        .flex_1()
        .min_h_0()
        .child(join_buttons(state))
        .child(
            div()
                .id("calls-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(list),
        )
        .into_any_element()
}
