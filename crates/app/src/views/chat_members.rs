use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::{person_avatar, with_presence};
use super::profile_card::opens_profile;
use super::sidebar::confirm_leave;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::chat_actions::TITLE_LIMIT;
use crate::chat_members::{MemberRow, can_leave, member_rows};
use crate::data::Directory;
use crate::notice::truncated;
use crate::theme;

const POPUP_WIDTH: f32 = 246.;
const LIST_MAX_HEIGHT: f32 = 300.;
const POPUP_OFFSET: f32 = 16.;
const SECTION_HEIGHT: f32 = 24.;
const ROW_HEIGHT: f32 = 40.;
const OWN_ROW_HEIGHT: f32 = 43.;
const AVATAR_SIZE: f32 = 32.;
const EDGE_PADDING: f32 = 12.;
const ACTION_HEIGHT: f32 = 36.;
const ACTION_ICON: f32 = 20.;
const REMOVE_BUTTON: f32 = 24.;

pub fn members_popover(app: &Entity<AppState>, chat_id: &str, trigger: Div) -> AnyElement {
    let app = app.clone();
    let chat_id = chat_id.to_owned();
    Popover::new("chat-members")
        .anchor(Anchor::TopRight)
        .offset(px(POPUP_OFFSET))
        .p(px(0.))
        .trigger(
            Button::new("chat-members-button")
                .ghost()
                .compact()
                .tooltip("View and add participants")
                .child(trigger),
        )
        .content(move |_, _, cx| members_panel(&app, &chat_id, cx))
        .into_any_element()
}

fn members_panel(
    app: &Entity<AppState>,
    chat_id: &str,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let popover = cx.entity();
    let state = app.read(cx);
    let Some(chat) = state.sidebar.chats.iter().find(|chat| chat.id == chat_id) else {
        return div().into_any_element();
    };
    let me = state.directory.me.as_ref();
    let rows = member_rows(chat, me);
    let leave_allowed = can_leave(chat, me);
    let is_member = me.is_some_and(|person| {
        chat.members
            .iter()
            .any(|member| member.user_id.as_deref() == Some(person.user_id.as_str()))
    });
    let title = chat.display_title();
    let member_count = chat.members.len();
    let list =
        v_flex()
            .id("chat-members-list")
            .max_h(px(LIST_MAX_HEIGHT))
            .overflow_y_scroll()
            .py(px(4.))
            .child(
                div()
                    .h(px(SECTION_HEIGHT))
                    .flex_none()
                    .px(px(EDGE_PADDING))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted())
                    .child(format!("People ({})", rows.len())),
            )
            .children(rows.iter().enumerate().map(|(index, row)| {
                member_row(app, &popover, &state.directory, chat_id, index, row)
            }));
    v_flex()
        .debug_selector(|| "chat-members-popup".into())
        .w(px(POPUP_WIDTH))
        .child(list)
        .when(is_member, |panel| {
            panel
                .child(div().h(px(1.)).w_full().bg(theme::border()))
                .child(
                    v_flex()
                        .py(px(4.))
                        .child(
                            action_row("chat-members-add", IconName::UserPlus, "Add people")
                                .opacity(0.5)
                                .cursor_default(),
                        )
                        .when(leave_allowed, |actions| {
                            let (app, popover, chat_id) =
                                (app.clone(), popover.clone(), chat_id.to_owned());
                            actions.child(
                                action_row("chat-members-leave", IconName::LogOut, "Leave")
                                    .cursor_pointer()
                                    .hover(|row| row.bg(theme::popup_hover()))
                                    .on_click(move |_, window, cx| {
                                        popover.update(cx, |state, cx| state.dismiss(window, cx));
                                        confirm_leave(
                                            &app,
                                            &chat_id,
                                            &title,
                                            member_count,
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                        }),
                )
        })
        .into_any_element()
}

fn member_row(
    app: &Entity<AppState>,
    popover: &Entity<PopoverState>,
    directory: &Directory,
    chat_id: &str,
    index: usize,
    row: &MemberRow,
) -> AnyElement {
    let group = SharedString::from(format!("chat-member-{index}"));
    let user_id = row.user_id.as_deref();
    let avatar = person_avatar(directory, user_id, &row.name, AVATAR_SIZE);
    let avatar = match user_id {
        Some(id) => with_presence(
            avatar,
            directory.presence_of(id),
            AVATAR_SIZE,
            theme::popup(),
        )
        .into_any_element(),
        None => avatar,
    };
    let opens_chat = user_id.filter(|_| !row.is_self).map(str::to_owned);
    let row_height = if row.is_self {
        OWN_ROW_HEIGHT
    } else {
        ROW_HEIGHT
    };
    h_flex()
        .id(("chat-member", index))
        .group(group.clone())
        .h(px(row_height))
        .flex_none()
        .px(px(EDGE_PADDING))
        .gap(px(8.))
        .items_center()
        .hover(|element| element.bg(theme::popup_hover()))
        .child(
            opens_profile(div().id(("chat-member-avatar", index)).flex_none(), user_id)
                .child(avatar),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(14.))
                        .text_color(theme::text())
                        .child(row.name.clone()),
                )
                .when(row.is_self, |column| {
                    column.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child("You"),
                    )
                }),
        )
        .when(row.removable, |element| {
            element.child(remove_button(app, popover, chat_id, index, row, group))
        })
        .when_some(opens_chat, |element, user_id| {
            let (app, popover) = (app.clone(), popover.clone());
            element.cursor_pointer().on_click(move |_, window, cx| {
                popover.update(cx, |state, cx| state.dismiss(window, cx));
                app.update(cx, |state, cx| state.open_chat_with(&user_id, cx));
            })
        })
        .into_any_element()
}

fn remove_button(
    app: &Entity<AppState>,
    popover: &Entity<PopoverState>,
    chat_id: &str,
    index: usize,
    row: &MemberRow,
    group: SharedString,
) -> Div {
    let (app, popover) = (app.clone(), popover.clone());
    let chat_id = chat_id.to_owned();
    let user_id = row.user_id.clone().unwrap_or_default();
    let name = row.name.clone();
    div()
        .size(px(REMOVE_BUTTON))
        .flex_none()
        .opacity(0.)
        .group_hover(group, |slot| slot.opacity(1.))
        .child(
            Button::new(("chat-member-remove", index))
                .icon(IconName::X)
                .ghost()
                .xsmall()
                .size(px(REMOVE_BUTTON))
                .tooltip("Remove from chat")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    popover.update(cx, |state, cx| state.dismiss(window, cx));
                    confirm_remove(&app, &chat_id, &user_id, &name, window, cx);
                }),
        )
}

fn action_row(id: &'static str, icon_name: IconName, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(ACTION_HEIGHT))
        .px(px(EDGE_PADDING))
        .gap(px(8.))
        .items_center()
        .text_size(px(14.))
        .text_color(theme::text())
        .child(icon(icon_name, ACTION_ICON, theme::text_soft()))
        .child(label)
}

fn confirm_remove(
    app: &Entity<AppState>,
    chat_id: &str,
    user_id: &str,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let (app, chat_id, user_id) = (app.clone(), chat_id.to_owned(), user_id.to_owned());
    let heading = format!(
        "Remove {} from the conversation?",
        truncated(name, TITLE_LIMIT)
    );
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (app, chat_id, user_id) = (app.clone(), chat_id.clone(), user_id.clone());
        let cancel = Button::new("remove-member-cancel")
            .label("Cancel")
            .on_click(|_, window, cx| window.close_dialog(cx));
        let remove = Button::new("remove-member-confirm")
            .label("Remove")
            .danger()
            .on_click(move |_, window, cx| {
                app.update(cx, |state, cx| {
                    state.remove_chat_member(&chat_id, &user_id, cx)
                });
                window.close_dialog(cx);
            });
        alert
            .title(heading.clone())
            .description("They'll still have access to the chat history.")
            .footer(DialogFooter::new().child(cancel).child(remove))
            .on_ok(|_, _, _| false)
    });
}
