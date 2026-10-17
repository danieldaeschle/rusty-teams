use chatsvc::{Reply, ResponseType};
use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::join_dialog::open_join_with_id;
use super::new_meeting_dialog::open_new_meeting;
use super::scroll_edge::with_top_edge;
use super::sidebar::SIDEBAR_SCROLL_DIVIDER;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{
    AGENDA_ROW_HEIGHT, AgendaRow, CALL_ROW_HEIGHT, CallRow, DAY_HEIGHT, EMPTY_HEIGHT, ListEntry,
    SECTION_HEIGHT, join_available, response_label,
};
use crate::theme;

const FALLBACK_VIEWPORT: f32 = 900.;
const VIEWPORT_MARGIN: f32 = 300.;
const TIME_COLUMN_WIDTH: f32 = 96.;
const AVATAR_SIZE: f32 = 36.;
const MENU_ICON_SIZE: f32 = 15.;

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
        .mx(px(6.))
        .h(px(CALL_ROW_HEIGHT))
        .flex_none()
        .px(px(10.))
        .gap(px(12.))
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

fn join_button(id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    let (fill, hover_fill, text_color) = if primary {
        (theme::accent(), theme::accent_text(), theme::on_accent())
    } else {
        (
            theme::surface_raised(),
            theme::popup_hover(),
            theme::text_strong(),
        )
    };
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(7.))
        .bg(fill)
        .cursor_pointer()
        .hover(move |button| button.bg(hover_fill))
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(text_color)
        .child(label)
}

fn join_buttons(state: &Entity<AppState>, meet_now_pending: bool) -> impl IntoElement {
    let meet_state = state.clone();
    let new_state = state.clone();
    let id_state = state.clone();
    let paste_state = state.clone();
    h_flex()
        .debug_selector(|| "shell/islands/sidebar/calls-actions".into())
        .w_full()
        .px(px(16.))
        .pb(px(8.))
        .gap(px(8.))
        .flex_wrap()
        .child(
            join_button("meet-now", "Meet now", true)
                .debug_selector(|| "shell/islands/sidebar/calls-actions/meet-now".into())
                .when(meet_now_pending, |button| button.opacity(0.5))
                .when(!meet_now_pending, |button| {
                    button.on_click(move |_, _, cx| {
                        meet_state.update(cx, |state, cx| state.start_meet_now(cx));
                    })
                }),
        )
        .child(
            join_button("new-meeting", "New meeting", false)
                .debug_selector(|| "shell/islands/sidebar/calls-actions/new-meeting".into())
                .on_click(move |_, window, cx| open_new_meeting(new_state.clone(), window, cx)),
        )
        .child(
            join_button("join-with-id", "Join with ID", false)
                .on_click(move |_, window, cx| open_join_with_id(id_state.clone(), window, cx)),
        )
        .child(
            join_button("paste-meeting-link", "Paste a meeting link", false).on_click(
                move |_, _, cx| {
                    paste_state.update(cx, |state, cx| state.join_meeting_from_clipboard(cx));
                },
            ),
        )
}

fn section_header(title: &'static str) -> AnyElement {
    div()
        .h(px(SECTION_HEIGHT))
        .px(px(16.))
        .flex()
        .items_end()
        .pb(px(4.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(title)
        .into_any_element()
}

fn day_header(label: &str) -> AnyElement {
    div()
        .h(px(DAY_HEIGHT))
        .px(px(16.))
        .flex()
        .items_center()
        .text_size(px(11.5))
        .text_color(theme::text_faint())
        .child(label.to_owned())
        .into_any_element()
}

fn empty_row(text: &'static str) -> AnyElement {
    div()
        .h(px(EMPTY_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.))
        .text_color(theme::text_muted())
        .child(text)
        .into_any_element()
}

fn rsvp_menu_item(
    state: &Entity<AppState>,
    event_key: &str,
    label: &'static str,
    reply: Reply,
    chosen: bool,
) -> PopupMenuItem {
    let state = state.clone();
    let event_key = event_key.to_owned();
    PopupMenuItem::new(label)
        .checked(chosen)
        .on_click(move |_, _, cx| {
            state.update(cx, |state, cx| {
                state.respond_to_event(&event_key, reply, cx)
            });
        })
}

fn rsvp_control(state: &Entity<AppState>, row: &AgendaRow, send_response: bool) -> AnyElement {
    let menu_state = state.clone();
    let key = row.key.clone();
    let response = row.response;
    div()
        .id(SharedString::from(format!("agenda-rsvp-{}", row.key)))
        .flex_none()
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            Button::new(SharedString::from(format!(
                "agenda-rsvp-button-{}",
                row.key
            )))
            .xsmall()
            .outline()
            .label(response_label(response))
            .icon(IconName::ChevronDown)
            .dropdown_menu(move |menu, _, _| {
                let toggle_state = menu_state.clone();
                menu.item(rsvp_menu_item(
                    &menu_state,
                    &key,
                    "Accept",
                    Reply::Accept,
                    response == ResponseType::Accepted,
                ))
                .item(rsvp_menu_item(
                    &menu_state,
                    &key,
                    "Tentative",
                    Reply::Tentative,
                    response == ResponseType::Tentative,
                ))
                .item(rsvp_menu_item(
                    &menu_state,
                    &key,
                    "Decline",
                    Reply::Decline,
                    response == ResponseType::Declined,
                ))
                .separator()
                .item(
                    PopupMenuItem::new("Send a response")
                        .checked(send_response)
                        .on_click(move |_, _, cx| {
                            toggle_state
                                .update(cx, |state, cx| state.toggle_rsvp_send_response(cx));
                        }),
                )
            }),
        )
        .into_any_element()
}

fn agenda_row(
    state: &Entity<AppState>,
    row: &AgendaRow,
    joinable: bool,
    send_response: bool,
) -> AnyElement {
    let click_state = state.clone();
    let click_row = row.clone();
    let join_state = state.clone();
    let join_row = row.clone();
    h_flex()
        .id(SharedString::from(format!("agenda-{}", row.key)))
        .mx(px(6.))
        .h(px(AGENDA_ROW_HEIGHT))
        .flex_none()
        .px(px(10.))
        .gap(px(12.))
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .child(
            div()
                .w(px(TIME_COLUMN_WIDTH))
                .flex_none()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child(row.time_label.clone()),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(14.))
                        .text_color(theme::text())
                        .child(row.subject.clone()),
                )
                .when(!row.detail.is_empty() || row.can_respond, |column| {
                    column.child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .text_color(theme::text_muted())
                                    .child(row.detail.clone()),
                            )
                            .when(row.can_respond, |line| {
                                line.child(rsvp_control(state, row, send_response))
                            }),
                    )
                }),
        )
        .when(joinable, |line| {
            line.child(
                Button::new(SharedString::from(format!("agenda-join-{}", row.key)))
                    .small()
                    .primary()
                    .label("Join")
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        join_state.update(cx, |state, cx| state.join_agenda_row(&join_row, cx));
                    }),
            )
        })
        .on_click(move |_, _, cx| {
            click_state.update(cx, |state, cx| state.open_agenda_chat(&click_row, cx));
        })
        .into_any_element()
}

pub fn calls_body(
    state: &Entity<AppState>,
    scroll: &ScrollHandle,
    window_height: f32,
    cx: &mut App,
) -> AnyElement {
    let now = Utc::now();
    let offset = Local::now().offset().fix();
    if state.read(cx).calls_list.needs_rebuild(now, offset) {
        let rebuild_state = state.clone();
        cx.defer(move |cx| rebuild_state.update(cx, |state, cx| state.rebuild_calls_list(cx)));
    }
    let top = -f32::from(scroll.offset().y);
    let measured = f32::from(scroll.bounds().size.height);
    let height = if measured > 1. {
        measured
    } else {
        window_height.max(FALLBACK_VIEWPORT)
    };
    let app = state.read(cx);
    let window = app
        .calls_list
        .window(top - VIEWPORT_MARGIN, top + height + VIEWPORT_MARGIN);
    let entries = &app.calls_list.entries()[window.range.clone()];
    let mut list = v_flex().w_full().pb(px(12.));
    if window.before > 0. {
        list = list.child(div().h(px(window.before)).flex_none());
    }
    for entry in entries {
        list = list.child(match entry {
            ListEntry::Section(title) => section_header(title),
            ListEntry::Day(label) => day_header(label),
            ListEntry::Empty(text) => empty_row(text),
            ListEntry::Agenda(row) => agenda_row(
                state,
                row,
                join_available(row.start, row.end, now) && row.join_url.is_some(),
                app.rsvp_send_response,
            ),
            ListEntry::Call(row) => call_row(
                state,
                row,
                app.can_open_chat_of_call_row(row),
                &app.directory,
            ),
        });
    }
    if window.after > 0. {
        list = list.child(div().h(px(window.after)).flex_none());
    }
    v_flex()
        .flex_1()
        .min_h_0()
        .child(join_buttons(state, app.meet_now_pending))
        .child(with_top_edge(
            div()
                .id("calls-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(list),
            scroll,
            SIDEBAR_SCROLL_DIVIDER,
        ))
        .into_any_element()
}
