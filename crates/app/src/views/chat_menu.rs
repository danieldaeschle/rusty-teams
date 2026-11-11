use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use teams_core::{ChatNotificationMode, DesktopNotifications, chat_link, is_notes_chat};

use super::chat_pop_out;
use super::widgets::menu_icon;
use crate::app_state::{AppState, chat_title};
use crate::chat_actions::TITLE_LIMIT;
use crate::notice::truncated;
use crate::presence_follow::{NOTIFY_WHEN_AVAILABLE, TURN_OFF_NOTIFICATIONS};
use crate::theme;

const GROUP_KIND: &str = "group";

#[derive(Clone)]
pub(crate) struct ChatMenuTarget {
    pub id: String,
    pub title: String,
    pub folder_id: Option<String>,
    pub unread: bool,
    pub muted: bool,
    pub is_group: bool,
    pub member_count: usize,
}

impl ChatMenuTarget {
    pub(crate) fn for_chat(state: &AppState, chat_id: &str) -> Option<Self> {
        let chat = state.sidebar.chats.iter().find(|chat| chat.id == chat_id)?;
        Some(ChatMenuTarget {
            id: chat.id.clone(),
            title: chat_title(chat),
            folder_id: state
                .directory
                .folder_of(chat_id)
                .map(|folder| folder.id.clone()),
            unread: chat.unread,
            muted: chat.muted,
            is_group: chat.kind == GROUP_KIND,
            member_count: chat.members.len(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct MenuContext {
    pub state: Entity<AppState>,
    pub folders: Vec<(String, String)>,
}

impl MenuContext {
    pub(crate) fn new(state: &Entity<AppState>, cx: &App) -> Self {
        MenuContext {
            state: state.clone(),
            folders: state.read(cx).directory.move_targets(),
        }
    }
}

pub(crate) fn chat_menu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    context: &MenuContext,
    target: &ChatMenuTarget,
) -> PopupMenu {
    let mut popup = popup;
    let read_state = context.state.clone();
    let read_id = target.id.clone();
    let unread = target.unread;
    let mute_state = context.state.clone();
    let mute_id = target.id.clone();
    let muted = target.muted;
    let hide_state = context.state.clone();
    let hide_id = target.id.clone();
    let notes = is_notes_chat(&target.id);
    popup = popup.item(
        PopupMenuItem::new(if unread {
            "Mark as read"
        } else {
            "Mark as unread"
        })
        .icon(menu_icon(if unread {
            IconName::MailOpen
        } else {
            IconName::Mail
        }))
        .on_click(move |_, _, cx| {
            read_state.update(cx, |state, cx| {
                if unread {
                    state.mark_chat_read(&read_id, cx);
                } else {
                    state.mark_chat_unread(&read_id, cx);
                }
            });
        }),
    );
    let current = target.folder_id.clone();
    let folders: Vec<(String, String)> = context
        .folders
        .iter()
        .filter(|(folder_id, _)| current.as_ref() != Some(folder_id))
        .cloned()
        .collect();
    if !folders.is_empty() {
        let move_state = context.state.clone();
        let move_id = target.id.clone();
        popup = popup.submenu_with_icon(
            Some(menu_icon(IconName::Folder)),
            "Move to section",
            window,
            cx,
            move |submenu, _, _| {
                folders.iter().fold(submenu, |submenu, (folder_id, name)| {
                    let (state, chat_id, folder_id) =
                        (move_state.clone(), move_id.clone(), folder_id.clone());
                    submenu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        state.update(cx, |state, cx| {
                            state.move_chat(&chat_id, Some(&folder_id), cx)
                        });
                    }))
                })
            },
        );
    }
    if current.is_some() {
        let remove_state = context.state.clone();
        let remove_id = target.id.clone();
        popup = popup.item(
            PopupMenuItem::new("Remove from section")
                .icon(menu_icon(IconName::FolderMinus))
                .on_click(move |_, _, cx| {
                    remove_state.update(cx, |state, cx| state.move_chat(&remove_id, None, cx));
                }),
        );
    }
    let hide_item = PopupMenuItem::new("Hide")
        .icon(menu_icon(IconName::EyeOff))
        .on_click(move |_, _, cx| {
            hide_state.update(cx, |state, cx| state.hide_chat(&hide_id, cx));
        });
    if notes {
        return popup
            .item(pop_out_item(context, target))
            .separator()
            .item(hide_item);
    }
    popup = popup.item(
        PopupMenuItem::new(if muted { "Unmute" } else { "Mute" })
            .icon(menu_icon(if muted {
                IconName::Bell
            } else {
                IconName::BellOff
            }))
            .on_click(move |_, _, cx| {
                mute_state.update(cx, |state, cx| state.set_chat_muted(&mute_id, !muted, cx));
            }),
    );
    popup = chat_notifications_submenu(popup, window, cx, context, target);
    let partner = (!target.is_group)
        .then(|| context.state.read(cx).one_on_one_partner(&target.id))
        .flatten();
    if let Some(partner) = partner {
        let follow_state = context.state.clone();
        let follow = !context
            .state
            .read(cx)
            .is_presence_followed(&partner.user_id);
        popup = popup.item(
            PopupMenuItem::new(if follow {
                NOTIFY_WHEN_AVAILABLE
            } else {
                TURN_OFF_NOTIFICATIONS
            })
            .icon(menu_icon(if follow {
                IconName::BellRing
            } else {
                IconName::BellOff
            }))
            .on_click(move |_, _, cx| {
                follow_state.update(cx, |state, cx| {
                    state.set_presence_followed(&partner.user_id, &partner.name, follow, cx)
                });
            }),
        );
    }
    let (link_state, link_id) = (context.state.clone(), target.id.clone());
    popup = popup
        .item(
            PopupMenuItem::new("Copy link")
                .icon(menu_icon(IconName::Link))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(chat_link(&link_id)));
                    link_state.update(cx, |state, cx| {
                        state.raise_notice("Link copied".to_owned(), None, cx)
                    });
                }),
        )
        .item(pop_out_item(context, target))
        .separator()
        .item(hide_item);
    if !target.is_group {
        let remove_state = context.state.clone();
        let remove_id = target.id.clone();
        popup = popup.item(
            PopupMenuItem::new(REMOVE_HISTORY_LABEL)
                .icon(menu_icon(IconName::Trash))
                .on_click(move |_, window, cx| {
                    confirm_remove_history(&remove_state, &remove_id, window, cx);
                }),
        );
    }
    if target.is_group {
        let leave_state = context.state.clone();
        let leave_id = target.id.clone();
        let title = target.title.clone();
        let member_count = target.member_count;
        popup = popup.item(
            PopupMenuItem::element(|_, _| {
                div().text_color(theme::red_soft()).child("Leave chat...")
            })
            .icon(menu_icon(IconName::LogOut).text_color(theme::red_soft()))
            .on_click(move |_, window, cx| {
                confirm_leave(&leave_state, &leave_id, &title, member_count, window, cx);
            }),
        );
    }
    popup
}

fn pop_out_item(context: &MenuContext, target: &ChatMenuTarget) -> PopupMenuItem {
    let (state, chat_id) = (context.state.clone(), target.id.clone());
    PopupMenuItem::new(chat_pop_out::POP_OUT_CHAT_LABEL)
        .icon(menu_icon(IconName::ExternalLink))
        .on_click(move |_, _, cx| chat_pop_out::pop_out_chat(&state, &chat_id, cx))
}

const CHAT_NOTIFICATION_MODES: [(&str, ChatNotificationMode); 2] = [
    ("All new messages", ChatNotificationMode::AllNewMessages),
    ("Mute", ChatNotificationMode::Mute),
];

const DESKTOP_NOTIFICATIONS: [(&str, DesktopNotifications); 2] = [
    (
        "Match settings used for all chats",
        DesktopNotifications::MatchAllChats,
    ),
    ("Don't get desktop notifications", DesktopNotifications::Off),
];

fn chat_notifications_submenu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    context: &MenuContext,
    target: &ChatMenuTarget,
) -> PopupMenu {
    let state = context.state.clone();
    let chat_id = target.id.clone();
    let mode = if target.muted {
        ChatNotificationMode::Mute
    } else {
        ChatNotificationMode::AllNewMessages
    };
    let desktop = state.read(cx).chat_desktop_notifications(&chat_id);
    popup.submenu_with_icon(
        Some(menu_icon(IconName::Bell)),
        "Manage notifications",
        window,
        cx,
        move |submenu, _, _| {
            let choose = |chosen_mode: ChatNotificationMode,
                          chosen_desktop: DesktopNotifications| {
                let (state, chat_id) = (state.clone(), chat_id.clone());
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    state.update(cx, |state, cx| {
                        state.set_chat_notifications(&chat_id, chosen_mode, chosen_desktop, cx)
                    });
                }
            };
            let submenu = submenu.label("Chat notification mode");
            let submenu =
                CHAT_NOTIFICATION_MODES
                    .iter()
                    .fold(submenu, |submenu, (label, value)| {
                        submenu.item(
                            PopupMenuItem::new(*label)
                                .checked(*value == mode)
                                .on_click(choose(*value, desktop)),
                        )
                    });
            let submenu = submenu
                .separator()
                .label("Set a desktop notification preference");
            DESKTOP_NOTIFICATIONS
                .iter()
                .fold(submenu, |submenu, (label, value)| {
                    submenu.item(
                        PopupMenuItem::new(*label)
                            .checked(*value == desktop)
                            .on_click(choose(mode, *value)),
                    )
                })
        },
    )
}

pub(crate) fn leave_description(member_count: usize) -> String {
    let who = match member_count {
        0 | 1 => "The other members".to_owned(),
        2 => "The other member".to_owned(),
        count => format!("The {} other members", count - 1),
    };
    let verb = if member_count == 2 { "sees" } else { "see" };
    format!("{who} {verb} that you left. You can only come back if someone adds you.")
}

const REMOVE_HISTORY_LABEL: &str = "Remove chat history";
const REMOVE_HISTORY_TITLE: &str = "Remove chat history?";
const REMOVE_HISTORY_TEXT: &str =
    "This will remove the chat from your list and search. It affects you only.";

fn confirm_remove_history(
    state: &Entity<AppState>,
    chat_id: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let state = state.clone();
    let chat_id = chat_id.to_owned();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let state = state.clone();
        let chat_id = chat_id.clone();
        let cancel = Button::new("remove-history-cancel")
            .label("Cancel")
            .on_click(|_, window, cx| window.close_dialog(cx));
        let remove = Button::new("remove-history-confirm")
            .label("Remove")
            .primary()
            .on_click(move |_, window, cx| {
                state.update(cx, |state, cx| state.remove_chat_history(&chat_id, cx));
                window.close_dialog(cx);
            });
        alert
            .title(REMOVE_HISTORY_TITLE)
            .description(REMOVE_HISTORY_TEXT)
            .footer(DialogFooter::new().child(cancel).child(remove))
            .on_ok(|_, _, _| false)
    });
}

pub(crate) fn confirm_leave(
    state: &Entity<AppState>,
    chat_id: &str,
    title: &str,
    member_count: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let state = state.clone();
    let chat_id = chat_id.to_owned();
    let heading = format!("Leave \"{}\"?", truncated(title, TITLE_LIMIT));
    let description = leave_description(member_count);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let state = state.clone();
        let chat_id = chat_id.clone();
        let cancel = Button::new("leave-cancel")
            .label("Cancel")
            .on_click(|_, window, cx| window.close_dialog(cx));
        let leave = Button::new("leave-confirm")
            .label("Leave")
            .danger()
            .on_click(move |_, window, cx| {
                state.update(cx, |state, cx| state.leave_chat(&chat_id, cx));
                window.close_dialog(cx);
            });
        alert
            .title(heading.clone())
            .description(description.clone())
            .footer(DialogFooter::new().child(cancel).child(leave))
            .on_ok(|_, _, _| false)
    });
}

#[cfg(test)]
mod tests {
    use super::leave_description;

    #[test]
    fn leave_text_counts_the_other_members() {
        assert_eq!(
            leave_description(4),
            "The 3 other members see that you left. You can only come back if someone adds you."
        );
        assert!(leave_description(2).starts_with("The other member sees"));
        assert!(leave_description(0).starts_with("The other members see"));
    }
}
