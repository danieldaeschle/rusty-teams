use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, WindowExt as _, h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::{
    ChannelNotificationLevel, ChannelNotifications, ChannelRecord, MessageImportance, SidebarTeam,
    Store,
};
use teams_core::team_avatar_key;

use super::activity_panel::ActivityPanel;
use super::avatar::{spec_avatar, team_avatar, with_presence};
use super::channel_admin_dialogs::{confirm_delete_channel, open_edit_channel_dialog};
use super::chat_menu::{ChatMenuTarget, MenuContext, chat_menu};
use super::create_channel_dialog::open_create_channel_dialog;
use super::last_views::LastViews;
use super::scroll_edge::{scroll_offset, with_top_edge};
use super::section_dialog::{SectionNameTarget, open_section_name_dialog};
use super::widgets::{count_badge, icon, menu_icon, page_header, symbol, unread_marker};
use crate::i18n::{tr, tr_args};
use crate::agents::AgentsPanel;
use crate::app_state::{AppEvent, AppState, QuickView, Selection};
use crate::channel_menu::{
    ChannelMenuItem, MenuEntry, TeamMenuItem, TeamMenuState, channel_menu_entries,
    team_menu_entries,
};
use crate::chat_actions::TITLE_LIMIT;
use crate::chat_filter::{ChatFilter, filtered_sections, has_no_matches};
use crate::data::{AvatarState, Directory, is_pending_folder};
use crate::format;
use crate::local_previews::LocalPreview;
use crate::notice::truncated;
use crate::sidebar_model::{
    AvatarSpec, ChannelOrderInput, ChatItem, DELETED_CHAT_PREVIEW, EMPTY_FOLDER_HINT, MentionBadge,
    NameStyle, NameWeight, Preview, Section, SectionInput, SectionKind, Step, build_sections,
    channel_badge, channel_name_style, channel_shown, channel_shows_unread,
    older_chats_trigger_resets, step_id, team_badge, team_channel_rows, team_hoisted,
    team_name_style, visible_channel_ids, wants_older_chats,
};
use crate::theme;
use app_ui_kit::layout::SIDEBAR_WIDTH;
use gpui_kit::component::input::InputState;

pub(crate) mod channel_filter_bar;
pub(crate) mod chat_filter_bar;

pub const SIDEBAR_SCROLL_DIVIDER: &str = "shell/islands/sidebar/divider-scroll";
const COLLAPSED_TEAMS_META_KEY: &str = "sidebar_collapsed_teams";
const ROW_HEIGHT: f32 = 54.;
const ROW_MORE_TOP: f32 = 6.;
const ROW_MORE_RIGHT: f32 = 6.;
const PROPOSAL_DOT_SIZE: f32 = 8.;
const PINNED_CHANNEL_HEIGHT: f32 = 48.;
const SECTION_HEIGHT: f32 = 30.;
const SECTION_GAP: f32 = 4.;
const QUICK_VIEWS_SECTION_ID: &str = "quick-views";
const QUICK_ROW_HEIGHT: f32 = 32.;
const QUICK_ICON_SIZE: f32 = 18.;
const QUICK_VIEWS: [(QuickView, &str); 3] = [
    (QuickView::Mentions, "alternate_email"),
    (QuickView::Drafts, "edit_note"),
    (QuickView::Saved, "bookmark"),
];
const HINT_HEIGHT: f32 = 26.;
const SEE_MORE_HEIGHT: f32 = 28.;
const FALLBACK_VIEWPORT: f32 = 900.;
const VIEWPORT_MARGIN: f32 = 300.;
const AVATAR_SIZE: f32 = 36.;
const TEAM_ROW_HEIGHT: f32 = 36.;
const CHANNEL_ROW_HEIGHT: f32 = 32.;
const TEAM_ICON_SIZE: f32 = 20.;
const TEAM_ICON_RADIUS: f32 = 4.;
const CHANNEL_INDENT: f32 = 60.;
pub const CHANNEL_ROW_TAG: &str = "sidebar/channel-row";
pub const SEE_ALL_CHANNELS_TAG: &str = "sidebar/see-all-channels";
const PRIVATE_MEMBERSHIP: &str = "private";
const REVEAL_ROW_HEIGHT: f32 = 32.;
const NEW_CHAT_BUTTON_SIZE: f32 = 32.;
const NOTIFICATION_ICON_SIZE: f32 = 13.;
const MENTION_ICON_SIZE: f32 = 15.;
const IMPORTANCE_MARKER_SIZE: f32 = 16.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Activity,
    Chats,
    Channels,
    Calendar,
    Calls,
    Agents,
}

impl SidebarTab {
    fn title(self) -> String {
        match self {
            SidebarTab::Activity => tr("rail.activity"),
            SidebarTab::Chats => tr("rail.chat"),
            SidebarTab::Channels => tr("rail.teams"),
            SidebarTab::Calendar => tr("rail.calendar"),
            SidebarTab::Calls => tr("rail.calls"),
            SidebarTab::Agents => tr("rail.agents"),
        }
    }
}

pub fn chat_row_selector(chat_id: &str) -> String {
    format!("chat-row/{chat_id}")
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
    PopupMenuItem::new(tr("sidebar.new_section"))
        .icon(menu_icon(IconName::Plus))
        .disabled(read_only)
        .on_click(move |_, window, cx| {
            open_section_name_dialog(state.clone(), SectionNameTarget::New, window, cx);
        })
}

fn move_section_item(
    state: &Entity<AppState>,
    label: String,
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
            PopupMenuItem::new(tr("sidebar.rename_section"))
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
                tr("sidebar.move_up"),
                IconName::ArrowUp,
                &target.id,
                target.previous_id.as_ref(),
                target.read_only,
            ))
            .item(move_section_item(
                state,
                tr("sidebar.move_down"),
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
                div()
                    .text_color(theme::red_soft())
                    .child(tr("sidebar.delete_section"))
            })
            .icon(menu_icon(IconName::Trash).text_color(theme::red_soft()))
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
    let mut args = HashMap::new();
    args.insert("name".to_owned(), truncated(name, TITLE_LIMIT));
    let heading = tr_args("sidebar.delete_section_title", &args);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let state = state.clone();
        let folder_id = folder_id.clone();
        let cancel = Button::new("delete-section-cancel")
            .label(tr("common.cancel"))
            .on_click(|_, window, cx| window.close_dialog(cx));
        let delete = Button::new("delete-section-confirm")
            .label(tr("common.delete"))
            .danger()
            .on_click(move |_, window, cx| {
                state.update(cx, |state, cx| state.delete_section(&folder_id, cx));
                window.close_dialog(cx);
            });
        alert
            .title(heading.clone())
            .description(tr("sidebar.delete_section_detail"))
            .footer(DialogFooter::new().child(cancel).child(delete))
            .on_ok(|_, _, _| false)
    });
}

pub enum SidebarEvent {
    RestoreFocus,
    TabChanged,
    NewChannelPost,
}

impl EventEmitter<SidebarEvent> for SidebarView {}

pub struct SidebarView {
    state: Entity<AppState>,
    tab: SidebarTab,
    activity_panel: Entity<ActivityPanel>,
    agents_panel: Option<Entity<AgentsPanel>>,
    collapsed_teams: HashSet<String>,
    revealed_channel_teams: HashSet<String>,
    hidden_teams_open: bool,
    chats_scroll: ScrollHandle,
    older_chats_trigger_top: Cell<Option<f32>>,
    older_chats_seen: Cell<(bool, bool)>,
    section_ids: RefCell<Vec<String>>,
    channels_scroll: ScrollHandle,
    chat_filter: ChatFilter,
    filter_box_open: bool,
    channel_filter: crate::channel_filter::ChannelFilter,
    channel_filter_open: bool,
    channel_filter_input: Option<Entity<InputState>>,
    channel_filter_subscription: Option<Subscription>,
    filter_input: Option<Entity<InputState>>,
    filter_subscription: Option<Subscription>,
    last_views: LastViews,
    _activity_observation: Subscription,
    _subscription: Subscription,
    _state_observation: Subscription,
    _agent_observation: Option<Subscription>,
    _runs_observation: Option<Subscription>,
}

fn preview_text(preview: &Preview) -> Option<(String, bool)> {
    match preview {
        Preview::Empty => None,
        Preview::Deleted => Some((DELETED_CHAT_PREVIEW.to_owned(), true)),
        Preview::Event(text) => Some((text.clone(), true)),
        Preview::Typing(text) => Some((text.clone(), true)),
        Preview::Text {
            prefix: Some(prefix),
            text,
        } => Some((format!("{prefix}: {text}"), false)),
        Preview::Text { prefix: None, text } => Some((text.clone(), false)),
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
                    .child(tr("sidebar.draft_prefix")),
            )
            .child(div().truncate().child(text.clone())),
        LocalPreview::NotSent => row.child(
            div()
                .flex_none()
                .text_color(theme::red_soft())
                .child(tr("sidebar.not_sent")),
        ),
    }
}

impl SidebarView {
    pub fn new(
        state: Entity<AppState>,
        activity_panel: Entity<ActivityPanel>,
        agents_panel: Option<Entity<AgentsPanel>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&state, |this, state, event: &AppEvent, cx| {
            if matches!(event, AppEvent::Selection) {
                if let Some(selection) = &state.read(cx).selection {
                    this.last_views.remember(selection);
                }
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
        let activity = activity_panel.read(cx).activity();
        let activity_observation = cx.observe(&activity, |view, _, cx| {
            if view
                .chat_filter
                .has(crate::chat_filter::ChatFilterPill::Mentions)
            {
                cx.notify();
            }
        });
        SidebarView {
            state,
            tab,
            activity_panel,
            agents_panel,
            collapsed_teams,
            revealed_channel_teams: HashSet::new(),
            hidden_teams_open: false,
            chats_scroll: ScrollHandle::new(),
            older_chats_trigger_top: Cell::new(None),
            older_chats_seen: Cell::new((false, false)),
            section_ids: RefCell::new(Vec::new()),
            channels_scroll: ScrollHandle::new(),
            chat_filter: ChatFilter::default(),
            filter_box_open: false,
            channel_filter: Default::default(),
            channel_filter_open: false,
            channel_filter_input: None,
            channel_filter_subscription: None,
            filter_input: None,
            filter_subscription: None,
            last_views: LastViews::default(),
            _activity_observation: activity_observation,
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

    pub fn show_calendar(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Calendar, cx);
    }

    pub fn open_quick_view(&mut self, view: QuickView, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Chats, cx);
        self.state
            .update(cx, |state, cx| state.open_quick_view(view, cx));
    }

    pub fn show_agents(&mut self, cx: &mut Context<Self>) {
        self.open_tab(SidebarTab::Agents, cx);
    }

    pub fn open_tab(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        if tab == SidebarTab::Agents && !crate::agents::agents_enabled(cx) {
            return;
        }
        let changed = self.tab != tab;
        self.tab = tab;
        if changed {
            self.restore_last_view(tab, cx);
            cx.emit(SidebarEvent::TabChanged);
        }
        match tab {
            SidebarTab::Calls => self.state.update(cx, |state, cx| {
                state.refresh_call_history(cx);
                state.refresh_calendar(cx);
            }),
            SidebarTab::Calendar => self.state.update(cx, |state, cx| state.open_calendar(cx)),
            SidebarTab::Activity => {
                let activity = self.activity_panel.read(cx).activity();
                activity.update(cx, |activity, cx| activity.mark_seen(cx));
            }
            _ => {}
        }
        cx.notify();
    }

    fn restore_last_view(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        let first_channel_id = self.first_channel_id(cx);
        let state = self.state.read(cx);
        if let Some(current) = &state.selection {
            self.last_views.remember(current);
        }
        let first_chat_id = state.sidebar.chats.first().map(|chat| chat.id.as_str());
        if let Some(selection) = self.last_views.restore(
            tab,
            state.selection.as_ref(),
            first_chat_id,
            first_channel_id.as_deref(),
        ) {
            self.state
                .update(cx, |state, cx| state.select(selection, cx));
        }
    }

    pub fn select_adjacent(&mut self, step: Step, cx: &mut Context<Self>) {
        let selection = match self.tab {
            SidebarTab::Chats => self
                .state
                .read(cx)
                .adjacent_chat_id(step)
                .map(Selection::Chat),
            SidebarTab::Channels => self.adjacent_channel_id(step, cx).map(Selection::Channel),
            SidebarTab::Activity
            | SidebarTab::Calendar
            | SidebarTab::Calls
            | SidebarTab::Agents => None,
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
        let input = self.channel_order_input(state, selected_channel);
        step_id(&visible_channel_ids(&input), selected_channel, step)
    }

    fn first_channel_id(&self, cx: &App) -> Option<String> {
        let input = self.channel_order_input(self.state.read(cx), None);
        visible_channel_ids(&input)
            .first()
            .map(|id| (*id).to_owned())
    }

    fn channel_order_input<'a>(
        &'a self,
        state: &'a AppState,
        selected_channel: Option<&'a str>,
    ) -> ChannelOrderInput<'a> {
        ChannelOrderInput {
            teams: &state.sidebar.teams,
            pinned_ids: &state.directory.pinned_channels,
            collapsed_teams: &self.collapsed_teams,
            revealed_channel_teams: &self.revealed_channel_teams,
            hidden_teams_open: self.hidden_teams_open,
            selected_channel,
        }
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
            .tooltip(|window, cx| {
                Tooltip::new(tr("sidebar.new_chat_shortcut")).build(window, cx)
            })
            .child(icon(IconName::Pencil, 16., theme::text_soft()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.state.update(cx, |state, cx| state.start_new_chat(cx));
            }))
    }

    fn chat_list_menu_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let section_ids = self.section_ids.borrow().clone();
        let read_only = self.state.read(cx).mode.read_only;
        Button::new("chat-list-more")
            .icon(IconName::Ellipsis)
            .ghost()
            .size(px(NEW_CHAT_BUTTON_SIZE))
            .rounded(px(8.))
            .tooltip(tr("sidebar.more_options"))
            .dropdown_menu(move |menu, _, _| {
                let (read_state, collapse_state) = (state.clone(), state.clone());
                let section_ids = section_ids.clone();
                menu.item(
                    PopupMenuItem::new(tr("sidebar.mark_all_read"))
                        .icon(menu_icon(IconName::CheckCheck))
                        .disabled(read_only)
                        .on_click(move |_, _, cx| {
                            read_state.update(cx, |state, cx| state.mark_all_chats_read(cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(tr("sidebar.collapse_all"))
                        .icon(menu_icon(IconName::ChevronsDownUp))
                        .on_click(move |_, _, cx| {
                            let ids = section_ids.clone();
                            collapse_state
                                .update(cx, |state, cx| state.collapse_sections(&ids, cx));
                        }),
                )
                .separator()
                .item(new_section_item(&state, read_only))
            })
    }

    fn channel_list_menu_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let read_only = self.state.read(cx).mode.read_only;
        let view = cx.entity().downgrade();
        let team_ids: Vec<String> = self
            .state
            .read(cx)
            .sidebar
            .teams
            .iter()
            .map(|entry| entry.team.id.clone())
            .collect();
        Button::new("channel-list-more")
            .icon(IconName::Ellipsis)
            .ghost()
            .size(px(NEW_CHAT_BUTTON_SIZE))
            .rounded(px(8.))
            .tooltip(tr("sidebar.more_options"))
            .dropdown_menu(move |menu, _, _| {
                let (state, view, team_ids) = (state.clone(), view.clone(), team_ids.clone());
                menu.item(
                    PopupMenuItem::new(tr("sidebar.mark_all_read"))
                        .icon(menu_icon(IconName::CheckCheck))
                        .disabled(read_only)
                        .on_click(move |_, _, cx| {
                            state.update(cx, |state, cx| state.mark_all_channels_read(cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(tr("sidebar.collapse_all"))
                        .icon(menu_icon(IconName::ChevronsDownUp))
                        .on_click(move |_, _, cx| {
                            let team_ids = team_ids.clone();
                            view.update(cx, |this, cx| this.collapse_all_teams(team_ids, cx))
                                .ok();
                        }),
                )
            })
    }

    fn draft_row(&self) -> impl IntoElement {
        h_flex()
            .id("new-chat-draft")
            .mx(px(6.))
            .h(px(ROW_HEIGHT))
            .flex_none()
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
                            .child(tr("sidebar.new_chat")),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(theme::text_muted())
                            .child(tr("sidebar.draft")),
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
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = toggle_id.clone();
                this.state
                    .update(cx, |state, cx| state.toggle_collapsed(&id, cx));
            }))
            .context_menu(move |popup, _, _| section_menu(popup, &menu_state, &menu))
    }

    fn quick_views_section(&self, cx: &mut Context<Self>) -> (Div, f32) {
        let (collapsed, open_view) = {
            let state = self.state.read(cx);
            (
                state.collapsed.contains(QUICK_VIEWS_SECTION_ID),
                state.quick_view,
            )
        };
        let header = h_flex()
            .id(QUICK_VIEWS_SECTION_ID)
            .h(px(SECTION_HEIGHT))
            .flex_none()
            .mt(px(SECTION_GAP))
            .mx(px(6.))
            .px(px(10.))
            .gap(px(6.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|header| header.bg(theme::row_hover()))
            .child(icon(
                if collapsed {
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
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_soft())
                    .child(tr("sidebar.quick_views")),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.state.update(cx, |state, cx| {
                    state.toggle_collapsed(QUICK_VIEWS_SECTION_ID, cx)
                });
            }));
        let mut section = v_flex().w_full().child(header);
        let mut height = SECTION_GAP + SECTION_HEIGHT;
        if !collapsed {
            for (view, glyph) in QUICK_VIEWS {
                section =
                    section.child(self.quick_view_row(view, glyph, open_view == Some(view), cx));
                height += QUICK_ROW_HEIGHT;
            }
        }
        (section, height)
    }

    fn quick_view_row(
        &self,
        view: QuickView,
        glyph: &'static str,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        h_flex()
            .id(SharedString::from(format!("quick-view-{}", view.title())))
            .debug_selector(move || format!("quick-view/{}", view.title()))
            .h(px(QUICK_ROW_HEIGHT))
            .flex_none()
            .mx(px(6.))
            .px(px(10.))
            .gap(px(10.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme::surface_raised()))
            .when(!selected, |row| row.hover(|row| row.bg(theme::row_hover())))
            .child(symbol(glyph, QUICK_ICON_SIZE, theme::text_soft()))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .child(view.title()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.state
                    .update(cx, |state, cx| state.open_quick_view(view, cx));
            }))
    }

    fn see_more_row(&self, section: &Section, cx: &mut Context<Self>) -> impl IntoElement {
        let toggle_id = section.id.clone();
        let label = if section.hidden_count > 0 {
            let mut args = HashMap::new();
            args.insert("count".to_owned(), section.hidden_count.to_string());
            tr_args("sidebar.see_more", &args)
        } else {
            tr("sidebar.see_less")
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
        let row_group = SharedString::from(format!("chat-row-{}", item.id));
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
            .group_hover(row_group.clone(), |time| time.invisible())
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
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap(px(4.))
                    .items_center()
                    .text_size(px(12.5))
                    .when(italic, |preview| preview.italic())
                    .text_color(preview_color)
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
                    .children(importance_marker(item.importance))
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
        let reorder_folder = item
            .folder_id
            .clone()
            .filter(|folder_id| !is_pending_folder(folder_id));
        let target = ChatMenuTarget {
            id: item.id.clone(),
            title: item.title.clone(),
            folder_id: item.folder_id.clone(),
            unread,
            muted: item.muted,
            is_group: item.is_group,
            member_count: item.member_count,
        };
        let more_context = menu.clone();
        let more_target = target.clone();
        let more_button = div()
            .absolute()
            .top(px(ROW_MORE_TOP))
            .right(px(ROW_MORE_RIGHT))
            .invisible()
            .group_hover(row_group.clone(), |button| button.visible())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new(SharedString::from(format!("chat-more-{}", item.id)))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Ellipsis)
                    .tooltip(tr("sidebar.more_options"))
                    .dropdown_menu(move |popup, window, cx| {
                        chat_menu(popup, window, cx, &more_context, &more_target)
                    }),
            );
        h_flex()
            .id(SharedString::from(format!("chat-{}", item.id)))
            .debug_selector(|| chat_row_selector(&item.id))
            .group(row_group)
            .relative()
            .mx(px(6.))
            .h(px(ROW_HEIGHT))
            .flex_none()
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
            .when_some(reorder_folder, |row, folder_id| {
                let (drop_state, target_id) = (self.state.clone(), item.id.clone());
                row.drag_over::<DraggedChat>(|style, _, _, _| style.bg(theme::drop_background()))
                    .on_drop(move |dragged: &DraggedChat, _, cx| {
                        drop_state.update(cx, |state, cx| {
                            state.drop_chat_on(&dragged.id, &target_id, &folder_id, cx)
                        });
                    })
            })
            .child(avatar)
            .child(lines)
            .child(more_button)
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
            let sections = if self.chat_filter.is_active() {
                let mentioned = self.mentioned_chats(cx);
                let selected_id = self.selected_chat_id(cx);
                filtered_sections(
                    &input,
                    &self.chat_filter,
                    &mentioned,
                    selected_id.as_deref(),
                )
            } else {
                build_sections(&input)
            };
            let menu = MenuContext::new(&self.state, cx);
            let section_menus =
                section_menu_targets(&sections, &state.directory, state.mode.read_only);
            *self.section_ids.borrow_mut() =
                sections.iter().map(|section| section.id.clone()).collect();
            (sections, section_menus, menu, state.local_previews.clone())
        };

        let top = -f32::from(scroll_offset(&self.chats_scroll).y);
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
                state
                    .selection
                    .clone()
                    .filter(|_| !state.new_chat && state.quick_view.is_none()),
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
        if !self.chat_filter.is_active() {
            let (quick_views, quick_height) = self.quick_views_section(cx);
            list = list.child(quick_views);
            y += quick_height;
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
                    .child(tr("sidebar.loading_older")),
            );
        }
        let selected_chat_id = self.selected_chat_id(cx);
        if self.chat_filter.is_active() && has_no_matches(&sections, selected_chat_id.as_deref()) {
            list = list.child(self.filter_empty_state(cx));
        }
        list = with_spacer(list, &mut skipped_height);
        self.request_lazy_data(faces, presence_users, cx);
        let scroll_area = div()
            .id("chats-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .track_scroll(&self.chats_scroll)
            .child(list);
        with_top_edge(scroll_area, &self.chats_scroll, SIDEBAR_SCROLL_DIVIDER).into_any_element()
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
        entry: &SidebarTeam,
        channel: &ChannelRecord,
        selected: bool,
        team_photo: Option<&AvatarState>,
    ) -> AnyElement {
        let team = &entry.team;
        let unread = channel_shows_unread(entry, channel);
        let notifications = entry.channel_notifications(channel_id);
        let badge = channel_badge(entry, channel_id);
        let name_style = channel_name_style(unread, true, selected);
        let state = self.state.clone();
        let menu_state = self.state.clone();
        let menu_channel_id = channel_id.to_owned();
        let selection = Selection::Channel(channel_id.to_owned());
        let time = channel_activity(channel);
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
            .child(team_avatar(team_photo, &team.name, &team.id, 28., 7.))
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
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(team.name.clone()),
                            )
                            .children(badge.map(mention_badge)),
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

    fn plain_header(&self, label: impl Into<SharedString>) -> impl IntoElement {
        div()
            .h(px(SECTION_HEIGHT))
            .mt(px(SECTION_GAP))
            .px(px(16.))
            .flex()
            .items_center()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_soft())
            .child(label.into())
    }

    fn team_block(
        &self,
        mut list: Div,
        entry: &SidebarTeam,
        hoisted_only: bool,
        selected: Option<&Selection>,
        team_photo: Option<&AvatarState>,
        cx: &mut Context<Self>,
    ) -> Div {
        let team_id = entry.team.id.clone();
        let expanded =
            self.channel_filter.is_active() || team_expanded(&team_id, &self.collapsed_teams);
        let name_style = team_name_style(entry, expanded);
        let badge = team_badge(entry, expanded);
        let toggle_id = team_id.clone();
        let (team_menu_state, team_menu_id) = (self.state.clone(), team_id.clone());
        let team_menu_flags = TeamMenuState {
            hidden: entry.hidden,
            can_add_channel: entry.rights.add_channel,
        };
        list = list.child(
            h_flex()
                .id(SharedString::from(format!("team-{team_id}")))
                .mx(px(6.))
                .h(px(TEAM_ROW_HEIGHT))
                .flex_none()
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
                .child(team_avatar(
                    team_photo,
                    &entry.team.name,
                    &entry.team.id,
                    TEAM_ICON_SIZE,
                    TEAM_ICON_RADIUS,
                ))
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
                .children(badge.map(mention_badge))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed_teams.remove(&toggle_id) {
                        this.collapsed_teams.insert(toggle_id.clone());
                    }
                    save_collapsed_teams(&this.state.read(cx).store, &this.collapsed_teams);
                    cx.notify();
                }))
                .context_menu(move |popup, _, _| {
                    team_menu(
                        popup,
                        team_menu_state.clone(),
                        &team_menu_id,
                        team_menu_flags,
                    )
                }),
        );
        if !expanded {
            return list;
        }
        let is_selected =
            |channel: &ChannelRecord| selected == Some(&Selection::Channel(channel.id.clone()));
        let revealed = self.revealed_channel_teams.contains(&team_id);
        let rows = team_channel_rows(entry, hoisted_only, revealed, is_selected);
        for channel in &rows.shown {
            list = list.child(self.team_channel_row(
                channel,
                is_selected(channel),
                channel_name_style(
                    channel_shows_unread(entry, channel),
                    channel_shown(entry, &channel.id),
                    is_selected(channel),
                ),
                entry.channel_notifications(&channel.id),
                channel_badge(entry, &channel.id),
            ));
        }
        if rows.hidden_count == 0 {
            return list;
        }
        let toggle_id = team_id.clone();
        list = list.child(self.see_all_channels_row(
            SharedString::from(format!("hidden-channels-{team_id}")),
            revealed,
            cx.listener(move |this, _, _, cx| {
                if !this.revealed_channel_teams.remove(&toggle_id) {
                    this.revealed_channel_teams.insert(toggle_id.clone());
                }
                cx.notify();
            }),
        ));
        for channel in rows.revealed {
            list = list.child(self.team_channel_row(
                channel,
                false,
                channel_name_style(channel_shows_unread(entry, channel), false, false),
                entry.channel_notifications(&channel.id),
                channel_badge(entry, &channel.id),
            ));
        }
        list
    }

    fn see_all_channels_row(
        &self,
        id: SharedString,
        open: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(id)
            .debug_selector(|| SEE_ALL_CHANNELS_TAG.into())
            .mx(px(6.))
            .h(px(CHANNEL_ROW_HEIGHT))
            .flex_none()
            .pl(px(CHANNEL_INDENT))
            .pr(px(10.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .text_size(px(13.5))
            .text_color(theme::accent_text())
            .child(if open {
                tr("sidebar.see_fewer_channels")
            } else {
                tr("sidebar.see_all_channels")
            })
            .on_click(on_click)
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
            .flex_none()
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
        badge: Option<MentionBadge>,
    ) -> impl IntoElement {
        let state = self.state.clone();
        let menu_state = self.state.clone();
        let menu_channel_id = channel.id.clone();
        let selection = Selection::Channel(channel.id.clone());
        h_flex()
            .id(SharedString::from(format!("channel-{}", channel.id)))
            .debug_selector(|| CHANNEL_ROW_TAG.into())
            .mx(px(6.))
            .h(px(CHANNEL_ROW_HEIGHT))
            .flex_none()
            .pl(px(CHANNEL_INDENT))
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
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(name_weight(name_style))
                            .text_color(name_color(name_style))
                            .child(channel.name.clone()),
                    )
                    .when(is_private_channel(channel), |name| {
                        name.child(div().flex_none().child(symbol(
                            "lock",
                            14.,
                            theme::text_muted(),
                        )))
                    }),
            )
            .children(notification_badge(notifications.level))
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(channel_activity(channel)),
            )
            .children(badge.map(mention_badge))
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

    fn collapse_all_teams(&mut self, team_ids: Vec<String>, cx: &mut Context<Self>) {
        self.collapsed_teams.extend(team_ids);
        save_collapsed_teams(&self.state.read(cx).store, &self.collapsed_teams);
        cx.notify();
    }

    fn channels_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let (sidebar, pinned_ids, selected, team_photos) = {
            let state = self.state.read(cx);
            let team_photos: HashMap<String, AvatarState> = state
                .sidebar
                .teams
                .iter()
                .filter_map(|entry| {
                    let photo = state.directory.avatar(&team_avatar_key(&entry.team.id))?;
                    Some((entry.team.id.clone(), photo.clone()))
                })
                .collect();
            (
                state.sidebar.clone(),
                state.directory.pinned_channels.clone(),
                state.selection.clone().filter(|_| !state.new_chat),
                team_photos,
            )
        };
        let filtering = self.channel_filter.is_active();
        let mut sidebar = sidebar;
        let pinned_ids = if filtering {
            sidebar.teams = self.channel_filter.apply(&sidebar.teams);
            Vec::new()
        } else {
            pinned_ids
        };
        let team_keys = sidebar
            .teams
            .iter()
            .map(|entry| team_avatar_key(&entry.team.id))
            .collect();
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
            list = list.child(self.plain_header(tr("sidebar.pinned")));
            for (entry, channel) in pinned {
                let is_selected = selected == Some(Selection::Channel(channel.id.clone()));
                list = list.child(self.channel_row(
                    &channel.id,
                    entry,
                    channel,
                    is_selected,
                    team_photos.get(&entry.team.id),
                ));
            }
        }
        list = list.child(self.plain_header(tr("sidebar.teams")));
        let hidden_teams: Vec<&SidebarTeam> =
            sidebar.teams.iter().filter(|entry| entry.hidden).collect();
        for entry in sidebar
            .teams
            .iter()
            .filter(|entry| !entry.hidden || team_hoisted(entry))
        {
            let team_photo = team_photos.get(&entry.team.id);
            list = self.team_block(list, entry, entry.hidden, selected.as_ref(), team_photo, cx);
        }
        if !hidden_teams.is_empty() {
            let holds_selection = hidden_teams
                .iter()
                .any(|entry| team_holds(entry, selected.as_ref()));
            let open = self.hidden_teams_open || holds_selection;
            list = list.child(self.reveal_row(
                "hidden-teams".into(),
                format!("{} ({})", tr("sidebar.hidden_teams"), hidden_teams.len()),
                open,
                10.,
                cx.listener(|this, _, _, cx| {
                    this.hidden_teams_open = !this.hidden_teams_open;
                    cx.notify();
                }),
            ));
            if open {
                for entry in hidden_teams {
                    let team_photo = team_photos.get(&entry.team.id);
                    list = self.team_block(list, entry, false, selected.as_ref(), team_photo, cx);
                }
            }
        }
        self.request_lazy_data(team_keys, Vec::new(), cx);
        let scroll_area = div()
            .id("channels-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .track_scroll(&self.channels_scroll)
            .child(list);
        with_top_edge(scroll_area, &self.channels_scroll, SIDEBAR_SCROLL_DIVIDER).into_any_element()
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

/// Teams chat list: red Important / AlertUrgent glyph while the last unread message is marked.
fn importance_marker(importance: MessageImportance) -> Option<Icon> {
    let name = match importance {
        MessageImportance::Normal => return None,
        MessageImportance::High => "priority_high",
        MessageImportance::Urgent => "notification_important_fill",
    };
    Some(symbol(name, IMPORTANCE_MARKER_SIZE, theme::red()))
}

fn mention_badge(badge: MentionBadge) -> AnyElement {
    match badge {
        MentionBadge::Count(count) => count_badge(count, false).into_any_element(),
        MentionBadge::Channel => {
            icon(IconName::Hash, MENTION_ICON_SIZE, theme::accent_text()).into_any_element()
        }
        MentionBadge::Team => {
            icon(IconName::Users, MENTION_ICON_SIZE, theme::accent_text()).into_any_element()
        }
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

pub(crate) fn channel_menu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    state: Entity<AppState>,
    channel_id: &str,
    notifications: ChannelNotifications,
) -> PopupMenu {
    let menu_state = state.read(cx).channel_menu_state(channel_id);
    let mut popup = popup;
    for entry in channel_menu_entries(menu_state) {
        let item = match entry {
            MenuEntry::Separator => {
                popup = popup.separator();
                continue;
            }
            MenuEntry::Item(ChannelMenuItem::Notifications) => {
                popup = notifications_submenu(popup, window, cx, &state, channel_id, notifications);
                continue;
            }
            MenuEntry::Item(item) => item,
        };
        let (action_state, action_id) = (state.clone(), channel_id.to_owned());
        popup = popup.item(
            PopupMenuItem::new(item.label())
                .icon(menu_icon(channel_menu_icon(item)))
                .disabled(menu_state.disables(item))
                .on_click(move |_, window, cx| {
                    match item {
                        ChannelMenuItem::EditChannel => {
                            open_edit_channel_dialog(action_state.clone(), &action_id, window, cx);
                            return;
                        }
                        ChannelMenuItem::DeleteChannel => {
                            confirm_delete_channel(action_state.clone(), &action_id, window, cx);
                            return;
                        }
                        _ => {}
                    }
                    action_state.update(cx, |state, cx| match item {
                        ChannelMenuItem::Pin => state.set_channel_pinned(&action_id, true, cx),
                        ChannelMenuItem::Unpin => state.set_channel_pinned(&action_id, false, cx),
                        ChannelMenuItem::Show => state.set_channel_shown(&action_id, true, cx),
                        ChannelMenuItem::Hide => state.set_channel_shown(&action_id, false, cx),
                        ChannelMenuItem::ManageChannel => {
                            state.manage_channel_in_browser(&action_id, cx)
                        }
                        ChannelMenuItem::CopyLink => state.copy_channel_link(&action_id, cx),
                        ChannelMenuItem::Notifications
                        | ChannelMenuItem::EditChannel
                        | ChannelMenuItem::DeleteChannel => {}
                    });
                }),
        );
    }
    popup
}

fn channel_menu_icon(item: ChannelMenuItem) -> IconName {
    match item {
        ChannelMenuItem::Notifications => IconName::Bell,
        ChannelMenuItem::Pin => IconName::Pin,
        ChannelMenuItem::Unpin => IconName::PinOff,
        ChannelMenuItem::Show => IconName::Eye,
        ChannelMenuItem::Hide => IconName::EyeOff,
        ChannelMenuItem::EditChannel => IconName::Pencil,
        ChannelMenuItem::ManageChannel => IconName::Settings,
        ChannelMenuItem::CopyLink => IconName::Link,
        ChannelMenuItem::DeleteChannel => IconName::Trash,
    }
}

fn team_menu(
    popup: PopupMenu,
    state: Entity<AppState>,
    team_id: &str,
    flags: TeamMenuState,
) -> PopupMenu {
    let mut popup = popup;
    for entry in team_menu_entries(flags) {
        let MenuEntry::Item(item) = entry else {
            popup = popup.separator();
            continue;
        };
        let (action_state, action_id) = (state.clone(), team_id.to_owned());
        popup = popup.item(
            PopupMenuItem::new(item.label())
                .icon(menu_icon(match item {
                    TeamMenuItem::AddChannel => IconName::Plus,
                    TeamMenuItem::Show => IconName::Eye,
                    TeamMenuItem::Hide => IconName::EyeOff,
                    TeamMenuItem::ManageTeam => IconName::Settings,
                    TeamMenuItem::CopyLink => IconName::Link,
                }))
                .on_click(move |_, window, cx| {
                    if item == TeamMenuItem::AddChannel {
                        open_create_channel_dialog(action_state.clone(), &action_id, window, cx);
                        return;
                    }
                    action_state.update(cx, |state, cx| match item {
                        TeamMenuItem::AddChannel => {}
                        TeamMenuItem::Show => state.set_team_shown(&action_id, true, cx),
                        TeamMenuItem::Hide => state.set_team_shown(&action_id, false, cx),
                        TeamMenuItem::ManageTeam => state.manage_team_in_browser(&action_id, cx),
                        TeamMenuItem::CopyLink => state.copy_team_link(&action_id, cx),
                    });
                }),
        );
    }
    popup
}

fn notifications_submenu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    state: &Entity<AppState>,
    channel_id: &str,
    notifications: ChannelNotifications,
) -> PopupMenu {
    let (state, channel_id) = (state.clone(), channel_id.to_owned());
    popup.submenu_with_icon(
        Some(menu_icon(IconName::Bell)),
        tr("sidebar.notifications"),
        window,
        cx,
        move |submenu, _, _| {
            let level_item = |label: String, level: ChannelNotificationLevel| {
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
                    tr("sidebar.notify_banner_activity"),
                    ChannelNotificationLevel::BannerAndFeed,
                ))
                .item(level_item(
                    tr("sidebar.notify_activity_only"),
                    ChannelNotificationLevel::Feed,
                ))
                .item(level_item(
                    tr("sidebar.notify_off"),
                    ChannelNotificationLevel::Off,
                ))
                .separator()
                .item(
                    PopupMenuItem::new(tr("sidebar.include_thread_replies"))
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

fn is_private_channel(channel: &ChannelRecord) -> bool {
    channel
        .membership_type
        .as_deref()
        .is_some_and(|membership| membership.eq_ignore_ascii_case(PRIVATE_MEMBERSHIP))
}

fn channel_activity(channel: &ChannelRecord) -> String {
    let offset = Local::now().offset().fix();
    channel
        .last_message_at
        .map(|time| {
            format::channel_time_label(time, Utc::now().with_timezone(&offset).date_naive(), offset)
        })
        .unwrap_or_default()
}

fn collect_faces(spec: &AvatarSpec, faces: &mut Vec<String>) {
    match spec {
        AvatarSpec::Single(face) | AvatarSpec::Bot(face) => faces.extend(face.user_id.clone()),
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
            SidebarTab::Agents => match &self.agents_panel {
                Some(panel) => panel.clone().into_any_element(),
                None => div().into_any_element(),
            },
            SidebarTab::Calls | SidebarTab::Calendar => div().into_any_element(),
        };
        let header = match self.tab {
            SidebarTab::Chats => Some(
                v_flex()
                    .flex_none()
                    .child(
                        page_header(&self.tab.title())
                            .child(self.chat_list_menu_button(cx))
                            .child(self.filter_toggle_button(cx))
                            .child(self.new_chat_button(cx)),
                    )
                    .child(self.filter_bar(cx)),
            ),
            SidebarTab::Channels => Some(
                v_flex()
                    .flex_none()
                    .child(
                        page_header(&self.tab.title())
                            .child(self.channel_list_menu_button(cx))
                            .child(self.channel_filter_button(cx))
                            .child(self.new_channel_items_button(cx)),
                    )
                    .child(self.channel_filter_bar(cx)),
            ),
            SidebarTab::Agents => Some(page_header(&self.tab.title())),
            SidebarTab::Activity | SidebarTab::Calls | SidebarTab::Calendar => None,
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

    use super::{drop_target, parse_collapsed_teams, serialize_collapsed_teams, team_expanded};
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
}
