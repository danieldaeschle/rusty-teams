use std::cell::Cell;
use std::collections::HashSet;
use std::time::Duration;

use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{WindowExt as _, h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::{
    ChannelNotificationLevel, ChannelNotifications, ChannelRecord, SidebarTeam, Store, TeamRecord,
};

use super::activity_panel::ActivityPanel;
use super::avatar::{spec_avatar, square_avatar, with_presence};
use super::calls_tab::calls_body;
use super::saved_panel::SavedPanel;
use super::section_dialog::{SectionNameTarget, open_section_name_dialog};
use super::widgets::{count_badge, icon, page_header, unread_marker};
use crate::agents::AgentsPanel;
use crate::app_state::{AppEvent, AppState, Selection};
use crate::chat_actions::TITLE_LIMIT;
use crate::data::{Directory, is_pending_folder};
use crate::format;
use crate::local_previews::LocalPreview;
use crate::notice::truncated;
use crate::sidebar_model::{
    AvatarSpec, ChannelOrderInput, ChatItem, DELETED_PREVIEW, EMPTY_FOLDER_HINT, NameStyle,
    NameWeight, Preview, Section, SectionInput, SectionKind, Step, build_sections,
    channel_name_style, older_chats_trigger_resets, step_id, team_name_style, visible_channel_ids,
    wants_older_chats,
};
use crate::theme;
use app_ui_kit::layout::SIDEBAR_WIDTH;

const COLLAPSED_TEAMS_META_KEY: &str = "sidebar_collapsed_teams";
const ROW_HEIGHT: f32 = 54.;
const PROPOSAL_DOT_SIZE: f32 = 8.;
const PINNED_CHANNEL_HEIGHT: f32 = 48.;
const SECTION_HEIGHT: f32 = 30.;
const SECTION_GAP: f32 = 4.;
const HINT_HEIGHT: f32 = 26.;
const SEE_MORE_HEIGHT: f32 = 28.;
const LOADING_OLDER_LABEL: &str = "Loading older chats";
const FALLBACK_VIEWPORT: f32 = 900.;
const CALLS_REFRESH: Duration = Duration::from_secs(30);
const VIEWPORT_MARGIN: f32 = 300.;
const AVATAR_SIZE: f32 = 36.;
const PINNED_LABEL: &str = "Pinned";
const TEAMS_LABEL: &str = "Teams";
const HIDDEN_TEAMS_LABEL: &str = "Hidden teams";
const TEAM_ROW_HEIGHT: f32 = 40.;
const REVEAL_ROW_HEIGHT: f32 = 32.;
const NEW_CHAT_BUTTON_SIZE: f32 = 32.;
const MENU_ICON_SIZE: f32 = 15.;
const NOTIFICATION_ICON_SIZE: f32 = 13.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Activity,
    Chats,
    Channels,
    Calls,
    Saved,
    Agents,
}

impl SidebarTab {
    fn title(self) -> &'static str {
        match self {
            SidebarTab::Activity => "Activity",
            SidebarTab::Chats => "Chats",
            SidebarTab::Channels => "Channels",
            SidebarTab::Calls => "Calls",
            SidebarTab::Saved => "Saved",
            SidebarTab::Agents => "Agents",
        }
    }
}

#[derive(Clone)]
struct DraggedChat {
    id: String,
    title: String,
}

struct DragGhost {
    title: String,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded(px(8.))
            .bg(theme::surface_raised())
            .border_1()
            .border_color(theme::accent())
            .text_size(px(13.))
            .text_color(theme::text())
            .child(self.title.clone())
    }
}

struct ChatMenuTarget {
    id: String,
    title: String,
    folder_id: Option<String>,
    unread: bool,
    muted: bool,
    is_group: bool,
    member_count: usize,
}

#[derive(Clone)]
struct MenuContext {
    state: Entity<AppState>,
    folders: Vec<(String, String)>,
}

fn menu_icon(name: IconName) -> gpui_kit::component::Icon {
    icon(name, MENU_ICON_SIZE, theme::text_muted())
}

fn chat_menu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    context: &MenuContext,
    target: &ChatMenuTarget,
) -> PopupMenu {
    let mut popup = popup;
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
    let read_state = context.state.clone();
    let read_id = target.id.clone();
    let unread = target.unread;
    let mute_state = context.state.clone();
    let mute_id = target.id.clone();
    let muted = target.muted;
    let hide_state = context.state.clone();
    let hide_id = target.id.clone();
    popup = popup
        .separator()
        .item(
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
        )
        .item(
            PopupMenuItem::new(if muted { "Unmute" } else { "Mute" })
                .icon(menu_icon(if muted {
                    IconName::Bell
                } else {
                    IconName::BellOff
                }))
                .on_click(move |_, _, cx| {
                    mute_state.update(cx, |state, cx| state.set_chat_muted(&mute_id, !muted, cx));
                }),
        )
        .separator()
        .item(
            PopupMenuItem::new("Hide")
                .icon(menu_icon(IconName::EyeOff))
                .on_click(move |_, _, cx| {
                    hide_state.update(cx, |state, cx| state.hide_chat(&hide_id, cx));
                }),
        );
    if target.is_group {
        let leave_state = context.state.clone();
        let leave_id = target.id.clone();
        let title = target.title.clone();
        let member_count = target.member_count;
        popup = popup.item(
            PopupMenuItem::element(|_, _| {
                div().text_color(theme::red_soft()).child("Leave chat...")
            })
            .icon(icon(IconName::LogOut, MENU_ICON_SIZE, theme::red_soft()))
            .on_click(move |_, window, cx| {
                confirm_leave(&leave_state, &leave_id, &title, member_count, window, cx);
            }),
        );
    }
    popup
}

#[derive(Clone)]
struct SectionMenuTarget {
    id: String,
    name: String,
    user_created: bool,
    previous_id: Option<String>,
    next_id: Option<String>,
    movable: bool,
    read_only: bool,
}

fn section_menu_targets(
    sections: &[Section],
    directory: &Directory,
    read_only: bool,
) -> Vec<SectionMenuTarget> {
    let movable_ids: HashSet<&str> = directory
        .folders
        .iter()
        .map(|folder| folder.id.as_str())
        .filter(|folder_id| !is_pending_folder(folder_id))
        .collect();
    let movable = |section: &Section| movable_ids.contains(section.id.as_str());
    sections
        .iter()
        .enumerate()
        .map(|(index, section)| SectionMenuTarget {
            id: section.id.clone(),
            name: section.name.clone(),
            user_created: section.kind == SectionKind::Folder && movable(section),
            previous_id: sections[..index]
                .iter()
                .rev()
                .find(|other| movable(other))
                .map(|other| other.id.clone()),
            next_id: sections[index + 1..]
                .iter()
                .find(|other| movable(other))
                .map(|other| other.id.clone()),
            movable: movable(section),
            read_only,
        })
        .collect()
}

fn drop_target(section: &Section) -> Option<Option<String>> {
    if is_pending_folder(&section.id) {
        return None;
    }
    Some(
        matches!(section.kind, SectionKind::Favorites | SectionKind::Folder)
            .then(|| section.id.clone()),
    )
}

fn new_section_item(state: &Entity<AppState>, read_only: bool) -> PopupMenuItem {
    let state = state.clone();
    PopupMenuItem::new("New section")
        .icon(menu_icon(IconName::Plus))
        .disabled(read_only)
        .on_click(move |_, window, cx| {
            open_section_name_dialog(state.clone(), SectionNameTarget::New, window, cx);
        })
}

fn move_section_item(
    state: &Entity<AppState>,
    label: &'static str,
    glyph: IconName,
    folder_id: &str,
    target_id: Option<&String>,
    read_only: bool,
) -> PopupMenuItem {
    let state = state.clone();
    let folder_id = folder_id.to_owned();
    let target_id = target_id.cloned();
    PopupMenuItem::new(label)
        .icon(menu_icon(glyph))
        .disabled(read_only || target_id.is_none())
        .on_click(move |_, _, cx| {
            if let Some(target_id) = &target_id {
                state.update(cx, |state, cx| {
                    state.move_section(&folder_id, target_id, cx)
                });
            }
        })
}

fn section_menu(
    popup: PopupMenu,
    state: &Entity<AppState>,
    target: &SectionMenuTarget,
) -> PopupMenu {
    let mut popup = popup;
    if target.user_created {
        let rename_state = state.clone();
        let rename_target = SectionNameTarget::Rename {
            folder_id: target.id.clone(),
            name: target.name.clone(),
        };
        popup = popup.item(
            PopupMenuItem::new("Rename section")
                .icon(menu_icon(IconName::Pencil))
                .disabled(target.read_only)
                .on_click(move |_, window, cx| {
                    open_section_name_dialog(
                        rename_state.clone(),
                        rename_target.clone(),
                        window,
                        cx,
                    );
                }),
        );
    }
    if target.movable {
        popup = popup
            .item(move_section_item(
                state,
                "Move up",
                IconName::ArrowUp,
                &target.id,
                target.previous_id.as_ref(),
                target.read_only,
            ))
            .item(move_section_item(
                state,
                "Move down",
                IconName::ArrowDown,
                &target.id,
                target.next_id.as_ref(),
                target.read_only,
            ))
            .separator();
    }
    popup = popup.item(new_section_item(state, target.read_only));
    if target.user_created {
        let delete_state = state.clone();
        let (folder_id, name) = (target.id.clone(), target.name.clone());
        popup = popup.separator().item(
            PopupMenuItem::element(|_, _| {
                div().text_color(theme::red_soft()).child("Delete section")
            })
            .icon(icon(IconName::Trash, MENU_ICON_SIZE, theme::red_soft()))
            .disabled(target.read_only)
            .on_click(move |_, window, cx| {
                confirm_delete_section(&delete_state, &folder_id, &name, window, cx);
            }),
        );
    }
    popup
}

fn confirm_delete_section(
    state: &Entity<AppState>,
    folder_id: &str,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let state = state.clone();
    let folder_id = folder_id.to_owned();
    let heading = format!("Delete section \"{}\"?", truncated(name, TITLE_LIMIT));
    window.open_alert_dialog(cx, move |alert, _, _| {
        let state = state.clone();
        let folder_id = folder_id.clone();
        let cancel = Button::new("delete-section-cancel")
            .label("Cancel")
            .on_click(|_, window, cx| window.close_dialog(cx));
        let delete = Button::new("delete-section-confirm")
            .label("Delete")
            .danger()
            .on_click(move |_, window, cx| {
                state.update(cx, |state, cx| state.delete_section(&folder_id, cx));
                window.close_dialog(cx);
            });
        alert
            .title(heading.clone())
            .description("Chats in it move back to Chats.")
            .footer(DialogFooter::new().child(cancel).child(delete))
            .on_ok(|_, _, _| false)
    });
}

fn leave_description(member_count: usize) -> String {
    let who = match member_count {
        0 | 1 => "The other members".to_owned(),
        2 => "The other member".to_owned(),
        count => format!("The {} other members", count - 1),
    };
    let verb = if member_count == 2 { "sees" } else { "see" };
    format!("{who} {verb} that you left. You can only come back if someone adds you.")
}

fn confirm_leave(
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

pub struct SidebarView {
    state: Entity<AppState>,
    tab: SidebarTab,
    activity_panel: Entity<ActivityPanel>,
    saved_panel: Entity<SavedPanel>,
    agents_panel: Option<Entity<AgentsPanel>>,
    collapsed_teams: HashSet<String>,
    revealed_channel_teams: HashSet<String>,
    hidden_teams_open: bool,
    chats_scroll: ScrollHandle,
    older_chats_trigger_top: Cell<Option<f32>>,
    older_chats_seen: Cell<(bool, bool)>,
    channels_scroll: ScrollHandle,
    calls_scroll: ScrollHandle,
    _subscription: Subscription,
    _state_observation: Subscription,
    _agent_observation: Option<Subscription>,
    _runs_observation: Option<Subscription>,
}

fn preview_text(preview: &Preview) -> Option<(String, bool)> {
    match preview {
        Preview::Empty => None,
        Preview::Deleted => Some((DELETED_PREVIEW.to_owned(), true)),
        Preview::Typing(text) => Some((text.clone(), true)),
        Preview::Text {
            prefix: Some(prefix),
            text,
        } => Some((format!("{prefix}: {text}"), false)),
        Preview::Text { prefix: None, text } => Some((text.clone(), false)),
    }
}

fn preview_icon(text: &str) -> Option<IconName> {
    match text.rsplit(": ").next()? {
        "Image" => Some(IconName::Image),
        "File" => Some(IconName::File),
        _ => None,
    }
}

fn local_preview_row(preview: &LocalPreview, color: Hsla) -> Div {
    let row = h_flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .gap(px(4.))
        .items_center()
        .text_size(px(12.5))
        .text_color(color);
    match preview {
        LocalPreview::Draft(text) => row
            .child(
                div()
                    .flex_none()
                    .text_color(theme::accent_text())
                    .child("Draft:"),
            )
            .child(div().truncate().child(text.clone())),
        LocalPreview::NotSent => row.child(
            div()
                .flex_none()
                .text_color(theme::red_soft())
                .child("Not sent"),
        ),
    }
}

impl SidebarView {
    pub fn new(
        state: Entity<AppState>,
        activity_panel: Entity<ActivityPanel>,
        saved_panel: Entity<SavedPanel>,
        agents_panel: Option<Entity<AgentsPanel>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&state, |this, state, event: &AppEvent, cx| {
            if matches!(event, AppEvent::Selection) {
                if state.read(cx).new_chat {
                    this.tab = SidebarTab::Chats;
                }
                this.expand_team_of_selection(cx);
            }
            if matches!(
                event,
                AppEvent::Sidebar
                    | AppEvent::CallHistory
                    | AppEvent::Selection
                    | AppEvent::Directory
                    | AppEvent::Typing
                    | AppEvent::LocalPreviews
            ) {
                cx.notify();
            }
        });
        let state_observation = cx.observe(&state, |_, _, cx| cx.notify());
        let agent_observation =
            crate::agent_api::handle(cx).map(|api| cx.observe(&api, |_, _, cx| cx.notify()));
        let runs_observation = crate::agents::runs_handle(cx).map(|runs| {
            cx.observe(&runs, |view, _, cx| {
                if view.tab == SidebarTab::Agents && !crate::agents::agents_enabled(cx) {
                    view.open_tab(SidebarTab::Chats, cx);
                }
            })
        });
        let tab = if state.read(cx).start_on_channels {
            SidebarTab::Channels
        } else {
            SidebarTab::Chats
        };
        let collapsed_teams = load_collapsed_teams(&state.read(cx).store);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CALLS_REFRESH).await;
                let alive = this.update(cx, |view, cx| {
                    if view.tab == SidebarTab::Calls {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        SidebarView {
            state,
            tab,
            activity_panel,
            saved_panel,
            agents_panel,
            collapsed_teams,
            revealed_channel_teams: HashSet::new(),
            hidden_teams_open: false,
            chats_scroll: ScrollHandle::new(),
            older_chats_trigger_top: Cell::new(None),
            older_chats_seen: Cell::new((false, false)),
            channels_scroll: ScrollHandle::new(),
            calls_scroll: ScrollHandle::new(),
            _subscription: subscription,
            _state_observation: state_observation,
            _agent_observation: agent_observation,
            _runs_observation: runs_observation,
        }
    }

    pub fn tab(&self) -> SidebarTab {
        self.tab
    }

    pub fn show_activity(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Activity, cx);
    }

    pub fn show_chats(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Chats, cx);
    }

    pub fn show_channels(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Channels, cx);
    }

    pub fn show_calls(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Calls, cx);
    }

    pub fn show_saved(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Saved, cx);
    }

    pub fn show_agents(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Agents, cx);
    }

    pub fn open_tab(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        if tab == SidebarTab::Agents && !crate::agents::agents_enabled(cx) {
            return;
        }
        self.tab = tab;
        match tab {
            SidebarTab::Calls => self.state.update(cx, |state, cx| {
                state.refresh_call_history(cx);
                state.refresh_calendar(cx);
            }),
            SidebarTab::Saved => self.state.update(cx, |state, cx| state.refresh_saved(cx)),
            _ => {}
        }
        cx.notify();
    }

    pub fn select_adjacent(&mut self, step: Step, cx: &mut Context<Self>) {
        let selection = match self.tab {
            SidebarTab::Chats => self
                .state
                .read(cx)
                .adjacent_chat_id(step)
                .map(Selection::Chat),
            SidebarTab::Channels => self.adjacent_channel_id(step, cx).map(Selection::Channel),
            SidebarTab::Activity | SidebarTab::Calls | SidebarTab::Saved | SidebarTab::Agents => {
                None
            }
        };
        if let Some(selection) = selection {
            self.state
                .update(cx, |state, cx| state.select(selection, cx));
        }
    }

    fn adjacent_channel_id(&self, step: Step, cx: &mut Context<Self>) -> Option<String> {
        let state = self.state.read(cx);
        let selected_channel = match &state.selection {
            Some(Selection::Channel(channel_id)) => Some(channel_id.as_str()),
            _ => None,
        };
        let input = ChannelOrderInput {
            teams: &state.sidebar.teams,
            pinned_ids: &state.directory.pinned_channels,
            collapsed_teams: &self.collapsed_teams,
            revealed_channel_teams: &self.revealed_channel_teams,
            hidden_teams_open: self.hidden_teams_open,
            selected_channel,
        };
        step_id(&visible_channel_ids(&input), selected_channel, step)
    }

    fn expand_team_of_selection(&mut self, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        let Some(Selection::Channel(channel_id)) = state.selection.clone() else {
            return;
        };
        let team_id = state
            .sidebar
            .teams
            .iter()
            .find(|entry| {
                entry
                    .channels
                    .iter()
                    .any(|channel| channel.id == channel_id)
            })
            .map(|entry| entry.team.id.clone());
        if let Some(team_id) = team_id
            && self.collapsed_teams.remove(&team_id)
        {
            save_collapsed_teams(&self.state.read(cx).store, &self.collapsed_teams);
        }
    }

    fn new_chat_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.state.read(cx).new_chat;
        div()
            .id("new-chat")
            .size(px(NEW_CHAT_BUTTON_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .bg(theme::surface_raised())
            .cursor_pointer()
            .when(active, |button| button.bg(theme::popup_hover()))
            .hover(|button| button.bg(theme::popup_hover()))
            .tooltip(|window, cx| Tooltip::new("New chat (Ctrl+N)").build(window, cx))
            .child(icon(IconName::Pencil, 16., theme::text_soft()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.state.update(cx, |state, cx| state.start_new_chat(cx));
            }))
    }

    fn chat_list_menu_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let read_only = self.state.read(cx).mode.read_only;
        Button::new("chat-list-more")
            .icon(IconName::Ellipsis)
            .ghost()
            .size(px(NEW_CHAT_BUTTON_SIZE))
            .rounded(px(8.))
            .tooltip("More options")
            .dropdown_menu(move |menu, _, _| menu.item(new_section_item(&state, read_only)))
    }

    fn draft_row(&self) -> impl IntoElement {
        h_flex()
            .id("new-chat-draft")
            .mx(px(6.))
            .h(px(ROW_HEIGHT))
            .px(px(10.))
            .gap(px(12.))
            .items_center()
            .rounded(px(8.))
            .bg(theme::surface_raised())
            .child(
                div()
                    .size(px(AVATAR_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme::border_strong())
                    .child(icon(IconName::Pencil, 16., theme::text())),
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
                            .child("New chat"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(theme::text_muted())
                            .child("Draft"),
                    ),
            )
    }

    fn section_header(
        &self,
        section: &Section,
        menu: SectionMenuTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let drop_target = drop_target(section);
        let toggle_id = section.id.clone();
        let menu_state = self.state.clone();
        h_flex()
            .id(SharedString::from(format!("section-{}", section.id)))
            .h(px(SECTION_HEIGHT))
            .mt(px(SECTION_GAP))
            .mx(px(6.))
            .px(px(10.))
            .gap(px(6.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|header| header.bg(theme::row_hover()))
            .when_some(drop_target, |header, drop_target| {
                header
                    .drag_over::<DraggedChat>(|style, _, _, _| {
                        style
                            .bg(theme::drop_background())
                            .border_1()
                            .border_color(theme::accent())
                    })
                    .on_drop(cx.listener(move |this, dragged: &DraggedChat, _, cx| {
                        let target = drop_target.clone();
                        this.state.update(cx, |state, cx| {
                            state.move_chat(&dragged.id, target.as_deref(), cx)
                        });
                    }))
            })
            .child(icon(
                if section.collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                },
                12.,
                theme::text_muted(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_soft())
                    .child(section.name.clone()),
            )
            .when(section.collapsed && section.unread_chats > 0, |header| {
                header.child(count_badge(section.unread_chats, false))
            })
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_faint())
                    .child(section.count.to_string()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = toggle_id.clone();
                this.state
                    .update(cx, |state, cx| state.toggle_collapsed(&id, cx));
            }))
            .context_menu(move |popup, _, _| section_menu(popup, &menu_state, &menu))
    }

    fn see_more_row(&self, section: &Section, cx: &mut Context<Self>) -> impl IntoElement {
        let toggle_id = section.id.clone();
        let label = if section.hidden_count > 0 {
            format!("See more ({})", section.hidden_count)
        } else {
            "See less".to_owned()
        };
        div()
            .id(SharedString::from(format!("see-more-{}", section.id)))
            .mx(px(6.))
            .pl(px(10. + AVATAR_SIZE + 12.))
            .h(px(SEE_MORE_HEIGHT))
            .flex()
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .text_size(px(12.5))
            .text_color(theme::accent_text())
            .hover(|row| row.bg(theme::row_hover()))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = toggle_id.clone();
                this.state
                    .update(cx, |state, cx| state.toggle_list_expanded(&id, cx));
            }))
    }

    fn chat_row(
        &self,
        item: &ChatItem,
        selected: bool,
        local_preview: Option<&LocalPreview>,
        proposal_agent: Option<&'static str>,
        directory: &Directory,
        menu: &MenuContext,
    ) -> AnyElement {
        let ring = if selected {
            theme::surface_raised()
        } else {
            theme::surface()
        };
        let unread = item.unread.is_unread();
        let presence = item
            .presence_user
            .as_deref()
            .map(|user_id| directory.presence_of(user_id));
        let avatar = spec_avatar(directory, &item.avatar, AVATAR_SIZE, ring);
        let avatar = match presence {
            Some(kind) => with_presence(avatar, kind, AVATAR_SIZE, ring).into_any_element(),
            None => avatar,
        };
        let name = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(14.))
            .font_weight(if unread {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(theme::text())
            .child(item.title.clone());
        let time = h_flex()
            .flex_none()
            .gap(px(4.))
            .items_center()
            .text_size(px(11.))
            .text_color(if unread {
                theme::accent_text()
            } else {
                theme::text_muted()
            })
            .child(item.time_label.clone());
        let preview_color = if matches!(item.preview, Preview::Typing(_)) {
            theme::accent_text()
        } else if unread {
            theme::text_strong()
        } else {
            theme::text_muted()
        };
        let preview = match (proposal_agent, local_preview) {
            (Some(agent), _) => Some(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(12.5))
                    .text_color(theme::accent_text())
                    .child(div().truncate().child(format!("Proposal from {agent}"))),
            ),
            (None, Some(local)) => Some(local_preview_row(local, preview_color)),
            (None, None) => preview_text(&item.preview).map(|(text, italic)| {
                let glyph = preview_icon(&text);
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap(px(4.))
                    .items_center()
                    .text_size(px(12.5))
                    .when(italic, |preview| preview.italic())
                    .text_color(preview_color)
                    .children(glyph.map(|name| icon(name, 12., preview_color)))
                    .child(div().truncate().child(text))
            }),
        };
        let lines = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_baseline()
                    .child(name)
                    .child(time),
            )
            .child(
                h_flex()
                    .h(px(18.))
                    .gap(px(8.))
                    .items_center()
                    .child(preview.unwrap_or_else(|| div().flex_1()))
                    .children(match proposal_agent {
                        Some(_) => Some(
                            div()
                                .size(px(PROPOSAL_DOT_SIZE))
                                .flex_none()
                                .rounded_full()
                                .bg(theme::accent())
                                .into_any_element(),
                        ),
                        None => unread_marker(item.unread, item.muted)
                            .map(IntoElement::into_any_element),
                    }),
            );
        let state = self.state.clone();
        let chat_id = item.id.clone();
        let dragged = DraggedChat {
            id: item.id.clone(),
            title: item.title.clone(),
        };
        let menu_context = menu.clone();
        let target = ChatMenuTarget {
            id: item.id.clone(),
            title: item.title.clone(),
            folder_id: item.folder_id.clone(),
            unread,
            muted: item.muted,
            is_group: item.is_group,
            member_count: item.member_count,
        };
        h_flex()
            .id(SharedString::from(format!("chat-{}", item.id)))
            .mx(px(6.))
            .h(px(ROW_HEIGHT))
            .px(px(10.))
            .gap(px(12.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme::surface_raised()))
            .when(!selected, |row| row.hover(|row| row.bg(theme::row_hover())))
            .on_drag(dragged, |dragged, _, _, cx| {
                let title = dragged.title.clone();
                cx.new(|_| DragGhost { title })
            })
            .child(avatar)
            .child(lines)
            .on_click(move |_, _, cx| {
                let selection = Selection::Chat(chat_id.clone());
                state.update(cx, |state, cx| state.select(selection, cx));
            })
            .context_menu(move |popup, window, cx| {
                chat_menu(popup, window, cx, &menu_context, &target)
            })
            .into_any_element()
    }

    fn chats_body(&self, window_height: f32, cx: &mut Context<Self>) -> AnyElement {
        let proposal_agents = crate::agent_api::pending_agents(cx);
        let (sections, section_menus, menu, local_previews) = {
            let state = self.state.read(cx);
            let now = Utc::now();
            let offset = Local::now().offset().fix();
            let input = SectionInput {
                chats: &state.sidebar.chats,
                directory: &state.directory,
                collapsed: &state.collapsed,
                expanded_lists: &state.expanded_lists,
                typing: &state.typing,
                now,
                offset,
            };
            let sections = build_sections(&input);
            let menu = MenuContext {
                state: self.state.clone(),
                folders: state.directory.move_targets(),
            };
            let section_menus =
                section_menu_targets(&sections, &state.directory, state.mode.read_only);
            (sections, section_menus, menu, state.local_previews.clone())
        };

        let top = -f32::from(self.chats_scroll.offset().y);
        let measured: f32 = f32::from(self.chats_scroll.bounds().size.height);
        let height = if measured > 1. {
            measured
        } else {
            window_height.max(FALLBACK_VIEWPORT)
        };
        let (visible_from, visible_to) = (top - VIEWPORT_MARGIN, top + height + VIEWPORT_MARGIN);

        let (selected, drafting) = {
            let state = self.state.read(cx);
            (
                state.selection.clone().filter(|_| !state.new_chat),
                state.new_chat,
            )
        };
        let mut faces: Vec<String> = Vec::new();
        let mut presence_users: Vec<String> = Vec::new();
        let mut y = 0.;
        let mut list = v_flex().w_full().pb(px(12.));
        let mut skipped_height = 0.;
        let mut others_end = None;
        let directory_state = self.state.clone();
        if drafting {
            list = list.child(self.draft_row());
            y += ROW_HEIGHT;
        }
        for (section, section_menu) in sections.iter().zip(section_menus) {
            list = with_spacer(list, &mut skipped_height);
            list = list.child(self.section_header(section, section_menu, cx));
            y += SECTION_GAP + SECTION_HEIGHT;
            if section.collapsed {
                continue;
            }
            for item in &section.items {
                let on_screen = y + ROW_HEIGHT >= visible_from && y <= visible_to;
                if on_screen {
                    collect_faces(&item.avatar, &mut faces);
                    presence_users.extend(item.presence_user.clone());
                }
                if on_screen {
                    list = with_spacer(list, &mut skipped_height);
                    let is_selected = selected == Some(Selection::Chat(item.id.clone()));
                    let directory = &directory_state.read(cx).directory;
                    let local_preview = local_previews.get(&item.id).filter(|_| !is_selected);
                    list = list.child(self.chat_row(
                        item,
                        is_selected,
                        local_preview,
                        proposal_agents.get(&item.id).copied(),
                        directory,
                        &menu,
                    ));
                } else {
                    skipped_height += ROW_HEIGHT;
                }
                y += ROW_HEIGHT;
            }
            if section.kind == SectionKind::Others {
                others_end = Some(y);
            }
            if section.hidden_count > 0 || section.list_expanded {
                list = with_spacer(list, &mut skipped_height);
                list = list.child(self.see_more_row(section, cx));
                y += SEE_MORE_HEIGHT;
            }
            if section.show_empty_hint {
                list = with_spacer(list, &mut skipped_height);
                list = list.child(
                    div()
                        .ml(px(16.))
                        .mr(px(16.))
                        .h(px(HINT_HEIGHT))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .italic()
                        .text_color(theme::text_faint())
                        .child(EMPTY_FOLDER_HINT),
                );
                y += HINT_HEIGHT;
            }
        }
        let (has_older, loading_older) = self.state.read(cx).older_chats();
        let previous = self.older_chats_seen.replace((has_older, loading_older));
        if older_chats_trigger_resets(previous, (has_older, loading_older)) {
            self.older_chats_trigger_top.set(None);
        }
        if has_older
            && wants_older_chats(
                others_end,
                top + height,
                top,
                self.older_chats_trigger_top.get(),
                loading_older,
            )
        {
            self.older_chats_trigger_top.set(Some(top));
            let state = self.state.clone();
            cx.defer(move |cx| state.read(cx).request_older_chats());
        }
        if loading_older {
            list = with_spacer(list, &mut skipped_height);
            list = list.child(
                div()
                    .pl(px(16. + AVATAR_SIZE + 12.))
                    .h(px(SEE_MORE_HEIGHT))
                    .flex()
                    .items_center()
                    .text_size(px(12.5))
                    .italic()
                    .text_color(theme::text_faint())
                    .child(LOADING_OLDER_LABEL),
            );
        }
        list = with_spacer(list, &mut skipped_height);
        self.request_lazy_data(faces, presence_users, cx);
        div()
            .id("chats-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.chats_scroll)
            .child(list)
            .into_any_element()
    }

    fn request_lazy_data(
        &self,
        faces: Vec<String>,
        presence_users: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if faces.is_empty() && presence_users.is_empty() {
            return;
        }
        let state = self.state.clone();
        cx.defer(move |cx| {
            state.update(cx, |state, cx| {
                state.request_avatars(faces, cx);
                state.request_presence(presence_users, cx);
            });
        });
    }

    fn channel_row(
        &self,
        channel_id: &str,
        team: &TeamRecord,
        channel: &ChannelRecord,
        selected: bool,
        notifications: ChannelNotifications,
    ) -> AnyElement {
        let unread = channel.unread;
        let name_style = channel_name_style(unread, true, selected);
        let state = self.state.clone();
        let menu_state = self.state.clone();
        let menu_channel_id = channel_id.to_owned();
        let selection = Selection::Channel(channel_id.to_owned());
        let now = Utc::now();
        let offset = Local::now().offset().fix();
        let time = channel
            .last_message_at
            .map(|time| {
                format::list_time_label(time, now.with_timezone(&offset).date_naive(), offset)
            })
            .unwrap_or_default();
        h_flex()
            .id(SharedString::from(format!("pinned-{channel_id}")))
            .mx(px(6.))
            .h(px(PINNED_CHANNEL_HEIGHT))
            .px(px(10.))
            .gap(px(12.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme::surface_raised()))
            .when(!selected, |row| row.hover(|row| row.bg(theme::row_hover())))
            .child(square_avatar(&team.name, &team.id, 28., 7.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_baseline()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .font_weight(name_weight(name_style))
                                    .text_color(name_color(name_style))
                                    .child(format!("{} / {}", team.name, channel.name)),
                            )
                            .children(notification_badge(notifications.level))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(if unread {
                                        theme::accent_text()
                                    } else {
                                        theme::text_muted()
                                    })
                                    .child(time),
                            ),
                    )
                    .child(
                        h_flex().gap(px(8.)).items_center().child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(team.name.clone()),
                        ),
                    ),
            )
            .on_click(move |_, _, cx| {
                let selection = selection.clone();
                state.update(cx, |state, cx| state.select(selection, cx));
            })
            .context_menu(move |popup, window, cx| {
                channel_menu(
                    popup,
                    window,
                    cx,
                    menu_state.clone(),
                    &menu_channel_id,
                    notifications,
                )
            })
            .into_any_element()
    }

    fn plain_header(&self, label: &str) -> impl IntoElement {
        div()
            .h(px(SECTION_HEIGHT))
            .mt(px(SECTION_GAP))
            .px(px(16.))
            .flex()
            .items_center()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_soft())
            .child(label.to_owned())
    }

    fn team_block(
        &self,
        mut list: Div,
        entry: &SidebarTeam,
        selected: Option<&Selection>,
        cx: &mut Context<Self>,
    ) -> Div {
        let team_id = entry.team.id.clone();
        let expanded = team_expanded(&team_id, &self.collapsed_teams);
        let name_style = team_name_style(entry, expanded);
        let toggle_id = team_id.clone();
        list = list.child(
            h_flex()
                .id(SharedString::from(format!("team-{team_id}")))
                .mx(px(6.))
                .h(px(TEAM_ROW_HEIGHT))
                .px(px(10.))
                .gap(px(8.))
                .items_center()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|row| row.bg(theme::row_hover()))
                .child(icon(
                    if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                    14.,
                    theme::text_muted(),
                ))
                .child(square_avatar(&entry.team.name, &entry.team.id, 28., 7.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.5))
                        .font_weight(name_weight(name_style))
                        .text_color(name_color(name_style))
                        .child(entry.team.name.clone()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed_teams.remove(&toggle_id) {
                        this.collapsed_teams.insert(toggle_id.clone());
                    }
                    save_collapsed_teams(&this.state.read(cx).store, &this.collapsed_teams);
                    cx.notify();
                })),
        );
        if !expanded {
            return list;
        }
        let is_hidden = |channel: &ChannelRecord| entry.hidden_channel_ids.contains(&channel.id);
        let is_selected =
            |channel: &ChannelRecord| selected == Some(&Selection::Channel(channel.id.clone()));
        for channel in entry
            .channels
            .iter()
            .filter(|channel| !is_hidden(channel) || is_selected(channel))
        {
            list = list.child(self.team_channel_row(
                channel,
                is_selected(channel),
                channel_name_style(channel.unread, !is_hidden(channel), is_selected(channel)),
                entry.channel_notifications(&channel.id),
            ));
        }
        let hidden: Vec<&ChannelRecord> = entry
            .channels
            .iter()
            .filter(|channel| is_hidden(channel) && !is_selected(channel))
            .collect();
        if hidden.is_empty() {
            return list;
        }
        let revealed = self.revealed_channel_teams.contains(&team_id);
        let toggle_id = team_id.clone();
        list = list.child(self.reveal_row(
            SharedString::from(format!("hidden-channels-{team_id}")),
            hidden_channels_label(hidden.len()),
            revealed,
            40.,
            cx.listener(move |this, _, _, cx| {
                if !this.revealed_channel_teams.remove(&toggle_id) {
                    this.revealed_channel_teams.insert(toggle_id.clone());
                }
                cx.notify();
            }),
        ));
        if revealed {
            for channel in hidden {
                list = list.child(self.team_channel_row(
                    channel,
                    false,
                    channel_name_style(channel.unread, false, false),
                    entry.channel_notifications(&channel.id),
                ));
            }
        }
        list
    }

    fn reveal_row(
        &self,
        id: SharedString,
        label: String,
        open: bool,
        indent: f32,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(id)
            .mx(px(6.))
            .h(px(REVEAL_ROW_HEIGHT))
            .pl(px(indent))
            .pr(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                12.,
                theme::text_faint(),
            ))
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .child(label),
            )
            .on_click(on_click)
    }

    fn team_channel_row(
        &self,
        channel: &ChannelRecord,
        is_selected: bool,
        name_style: NameStyle,
        notifications: ChannelNotifications,
    ) -> impl IntoElement {
        let state = self.state.clone();
        let menu_state = self.state.clone();
        let menu_channel_id = channel.id.clone();
        let selection = Selection::Channel(channel.id.clone());
        h_flex()
            .id(SharedString::from(format!("channel-{}", channel.id)))
            .mx(px(6.))
            .h(px(TEAM_ROW_HEIGHT))
            .pl(px(40.))
            .pr(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(is_selected, |row| row.bg(theme::surface_raised()))
            .when(!is_selected, |row| {
                row.hover(|row| row.bg(theme::row_hover()))
            })
            .child(
                div()
                    .w(px(14.))
                    .flex()
                    .justify_center()
                    .text_size(px(15.))
                    .text_color(theme::text_faint())
                    .child("#"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.5))
                    .font_weight(name_weight(name_style))
                    .text_color(name_color(name_style))
                    .child(channel.name.clone()),
            )
            .children(notification_badge(notifications.level))
            .on_click(move |_, _, cx| {
                let selection = selection.clone();
                state.update(cx, |state, cx| state.select(selection, cx));
            })
            .context_menu(move |popup, window, cx| {
                channel_menu(
                    popup,
                    window,
                    cx,
                    menu_state.clone(),
                    &menu_channel_id,
                    notifications,
                )
            })
    }

    fn channels_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let (sidebar, pinned_ids, selected) = {
            let state = self.state.read(cx);
            (
                state.sidebar.clone(),
                state.directory.pinned_channels.clone(),
                state.selection.clone().filter(|_| !state.new_chat),
            )
        };
        let mut list = v_flex().w_full().pb(px(12.));
        let pinned: Vec<(&SidebarTeam, &ChannelRecord)> = pinned_ids
            .iter()
            .filter_map(|id| {
                sidebar.teams.iter().find_map(|entry| {
                    entry
                        .channels
                        .iter()
                        .find(|channel| &channel.id == id)
                        .map(|channel| (entry, channel))
                })
            })
            .collect();
        if !pinned.is_empty() {
            list = list.child(self.plain_header(PINNED_LABEL));
            for (entry, channel) in pinned {
                let is_selected = selected == Some(Selection::Channel(channel.id.clone()));
                list = list.child(self.channel_row(
                    &channel.id,
                    &entry.team,
                    channel,
                    is_selected,
                    entry.channel_notifications(&channel.id),
                ));
            }
        }
        list = list.child(self.plain_header(TEAMS_LABEL));
        let (shown_teams, hidden_teams): (Vec<&SidebarTeam>, Vec<&SidebarTeam>) =
            sidebar.teams.iter().partition(|entry| !entry.hidden);
        for entry in shown_teams {
            list = self.team_block(list, entry, selected.as_ref(), cx);
        }
        if !hidden_teams.is_empty() {
            let holds_selection = hidden_teams
                .iter()
                .any(|entry| team_holds(entry, selected.as_ref()));
            let open = self.hidden_teams_open || holds_selection;
            list = list.child(self.reveal_row(
                "hidden-teams".into(),
                format!("{HIDDEN_TEAMS_LABEL} ({})", hidden_teams.len()),
                open,
                10.,
                cx.listener(|this, _, _, cx| {
                    this.hidden_teams_open = !this.hidden_teams_open;
                    cx.notify();
                }),
            ));
            if open {
                for entry in hidden_teams {
                    list = self.team_block(list, entry, selected.as_ref(), cx);
                }
            }
        }
        div()
            .id("channels-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.channels_scroll)
            .child(list)
            .into_any_element()
    }
}

fn name_weight(style: NameStyle) -> FontWeight {
    match style.weight {
        NameWeight::Regular => FontWeight::NORMAL,
        NameWeight::Semibold => FontWeight::SEMIBOLD,
        NameWeight::Bold => FontWeight::BOLD,
    }
}

fn name_color(style: NameStyle) -> Hsla {
    if style.highlighted {
        theme::text()
    } else {
        theme::text_muted()
    }
}

fn notification_badge(level: ChannelNotificationLevel) -> Option<impl IntoElement> {
    let name = match level {
        ChannelNotificationLevel::BannerAndFeed => IconName::Bell,
        ChannelNotificationLevel::Off => IconName::BellOff,
        ChannelNotificationLevel::Feed => return None,
    };
    Some(icon(name, NOTIFICATION_ICON_SIZE, theme::text_muted()))
}

fn channel_menu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    state: Entity<AppState>,
    channel_id: &str,
    notifications: ChannelNotifications,
) -> PopupMenu {
    let channel_id = channel_id.to_owned();
    popup.submenu_with_icon(
        Some(menu_icon(IconName::Bell)),
        "Notifications",
        window,
        cx,
        move |submenu, _, _| {
            let level_item = |label: &'static str, level: ChannelNotificationLevel| {
                let (state, channel_id) = (state.clone(), channel_id.clone());
                PopupMenuItem::new(label)
                    .checked(notifications.level == level)
                    .on_click(move |_, _, cx| {
                        let changed = ChannelNotifications {
                            level,
                            include_replies: notifications.include_replies
                                && level != ChannelNotificationLevel::Off,
                        };
                        state.update(cx, |state, cx| {
                            state.set_channel_notifications(&channel_id, changed, cx)
                        });
                    })
            };
            let (replies_state, replies_id) = (state.clone(), channel_id.clone());
            submenu
                .item(level_item(
                    "Banner and activity",
                    ChannelNotificationLevel::BannerAndFeed,
                ))
                .item(level_item("Activity only", ChannelNotificationLevel::Feed))
                .item(level_item("Off", ChannelNotificationLevel::Off))
                .separator()
                .item(
                    PopupMenuItem::new("Include thread replies")
                        .checked(notifications.include_replies)
                        .disabled(notifications.level == ChannelNotificationLevel::Off)
                        .on_click(move |_, _, cx| {
                            let changed = ChannelNotifications {
                                level: notifications.level,
                                include_replies: !notifications.include_replies,
                            };
                            replies_state.update(cx, |state, cx| {
                                state.set_channel_notifications(&replies_id, changed, cx)
                            });
                        }),
                )
        },
    )
}

fn team_expanded(team_id: &str, collapsed_teams: &HashSet<String>) -> bool {
    !collapsed_teams.contains(team_id)
}

fn load_collapsed_teams(store: &Store) -> HashSet<String> {
    store
        .meta(COLLAPSED_TEAMS_META_KEY)
        .ok()
        .flatten()
        .map(|value| parse_collapsed_teams(&value))
        .unwrap_or_default()
}

fn save_collapsed_teams(store: &Store, collapsed_teams: &HashSet<String>) {
    let _ = store.set_meta(
        COLLAPSED_TEAMS_META_KEY,
        &serialize_collapsed_teams(collapsed_teams),
    );
}

fn serialize_collapsed_teams(collapsed_teams: &HashSet<String>) -> String {
    let mut team_ids: Vec<&String> = collapsed_teams.iter().collect();
    team_ids.sort_unstable();
    serde_json::to_string(&team_ids).unwrap_or_default()
}

fn parse_collapsed_teams(value: &str) -> HashSet<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn team_holds(entry: &SidebarTeam, selected: Option<&Selection>) -> bool {
    entry
        .channels
        .iter()
        .any(|channel| selected == Some(&Selection::Channel(channel.id.clone())))
}

fn hidden_channels_label(count: usize) -> String {
    if count == 1 {
        "1 hidden channel".to_owned()
    } else {
        format!("{count} hidden channels")
    }
}

fn collect_faces(spec: &AvatarSpec, faces: &mut Vec<String>) {
    match spec {
        AvatarSpec::Single(face) => faces.extend(face.user_id.clone()),
        AvatarSpec::Pair(first, second) => {
            faces.extend(first.user_id.clone());
            faces.extend(second.user_id.clone());
        }
        AvatarSpec::Meeting | AvatarSpec::Muted => {}
    }
}

fn with_spacer(list: Div, skipped_height: &mut f32) -> Div {
    let height = std::mem::take(skipped_height);
    if height > 0. {
        list.child(div().h(px(height)).flex_none())
    } else {
        list
    }
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_height = f32::from(window.viewport_size().height);
        let body = match self.tab {
            SidebarTab::Activity => self.activity_panel.clone().into_any_element(),
            SidebarTab::Chats => self.chats_body(window_height, cx),
            SidebarTab::Channels => self.channels_body(cx),
            SidebarTab::Calls => calls_body(&self.state, &self.calls_scroll, window_height, cx),
            SidebarTab::Saved => self.saved_panel.clone().into_any_element(),
            SidebarTab::Agents => match &self.agents_panel {
                Some(panel) => panel.clone().into_any_element(),
                None => div().into_any_element(),
            },
        };
        let header = match self.tab {
            SidebarTab::Chats => Some(
                page_header(self.tab.title())
                    .child(self.chat_list_menu_button(cx))
                    .child(self.new_chat_button(cx)),
            ),
            SidebarTab::Channels | SidebarTab::Calls | SidebarTab::Agents => {
                Some(page_header(self.tab.title()))
            }
            SidebarTab::Activity | SidebarTab::Saved => None,
        };
        v_flex()
            .debug_selector(|| "shell/islands/sidebar".into())
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .bg(theme::surface())
            .rounded(px(10.))
            .overflow_hidden()
            .children(header)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        drop_target, leave_description, parse_collapsed_teams, serialize_collapsed_teams,
        team_expanded,
    };
    use crate::data::PENDING_FOLDER_PREFIX;
    use crate::sidebar_model::{Section, SectionKind};

    fn section(id: &str, kind: SectionKind) -> Section {
        Section {
            id: id.to_owned(),
            name: id.to_owned(),
            kind,
            collapsed: false,
            count: 0,
            unread_chats: 0,
            items: Vec::new(),
            hidden_count: 0,
            list_expanded: false,
            show_empty_hint: false,
        }
    }

    #[test]
    fn pending_sections_take_no_dropped_chats() {
        let pending = format!("{PENDING_FOLDER_PREFIX}1");
        assert_eq!(drop_target(&section(&pending, SectionKind::Folder)), None);
        assert_eq!(
            drop_target(&section("work", SectionKind::Folder)),
            Some(Some("work".to_owned()))
        );
        assert_eq!(
            drop_target(&section("recent", SectionKind::Others)),
            Some(None)
        );
    }

    #[test]
    fn teams_start_expanded() {
        assert!(team_expanded("t1", &HashSet::new()));
    }

    #[test]
    fn collapsed_team_stays_closed() {
        let collapsed = HashSet::from(["t1".to_owned()]);
        assert!(!team_expanded("t1", &collapsed));
        assert!(team_expanded("t2", &collapsed));
    }

    #[test]
    fn collapsed_teams_round_trip_as_json() {
        let collapsed = HashSet::from(["b".to_owned(), "a".to_owned()]);
        let json = serialize_collapsed_teams(&collapsed);
        assert_eq!(json, r#"["a","b"]"#);
        assert_eq!(parse_collapsed_teams(&json), collapsed);
        assert!(parse_collapsed_teams("garbage").is_empty());
    }

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
