mod team_keys;

use std::sync::Arc;

use gpui_kit::component::{elevation, h_flex, v_flex};
use gpui_kit::*;

use super::activity_panel::{ActivityPanel, ActivityPanelEvent};
use super::actions::{NewChat, OpenSwitcher, ReplyToHovered, ToggleCallMute};
use super::calendar_view::CalendarView;
use super::conversation::ConversationView;
use super::dialog_overlay::render_task_dialog;
use super::forward_dialog::{ForwardDialog, ForwardDialogEvent};
use super::image_viewer::{ImageViewer, ImageViewerEvent};
use super::profile_card::{ProfileCard, ProfileCardEvent};
use gpui_kit::prelude::FluentBuilder as _;
use super::rail::RailView;
use crate::chat_filter::ChatFilterPill;
use super::calls_page::CallsPage;
use super::quick_view_pane::{QuickViewEvent, QuickViewPane};
use super::shortcuts::{
    ClearChatFilters, DeclineCall, FilterMeetingChats, FilterMutedChats,
    FilterNonMeetingChats, FilterUnreadChats, FindInChat, HangUpCall, JoinMeetingFromToast, JumpToLastReadOrNewest, NextConversation, OpenActivity,
    OpenAgents, OpenCallsTab, OpenSettings, OpenChannelsTab, OpenChatFilter, OpenChatsTab, PopOutSelectedChat, OpenLobbyList, OpenSaved, OpenCalendar, PreviousConversation, PushToTalk,
    Scope, ShowShortcuts, StartAudioCall, StartVideoCall, ToggleCallCamera,
    ToggleCallHand, ToggleCallShare, ZoomIn, ZoomOut, ZoomReset, ZoomShareIn, ZoomShareOut,
    ZoomShareReset,
};
use super::window_zoom::{WindowZoom, change_zoom};
use super::call_pop_out::zoom_listener;
use super::chat_pop_out;
use super::shortcuts_dialog::{ShortcutsDialog, ShortcutsDialogEvent};
use super::sidebar::{SidebarEvent, SidebarTab, SidebarView};
use app_ui_kit::render::hooks::RenderHooks;
use app_ui_kit::layout::{ISLAND_GAP, SIDEBAR_WIDTH};
use super::connection_banner::render_connection_banner;
use super::manage_team_page::ManageTeamPage;
use super::splash::{self, Phase, Splash, StartupFacts};
use super::notice_toast::NoticeToasts;
use super::status_menu::{own_status_button, settings_more_button};
use super::status_message_dialog::{StatusMessageDialog, StatusMessageDialogEvent};
use super::switcher::{Switcher, SwitcherEvent, candidates_from};
use super::title_bar::{mode_label, render_title_bar, update_button};
use crate::activity::ActivityCenter;
use crate::agents::{AgentRunsEvent, StartDialog, StartDialogEvent, StartRequest};
use crate::app_state::{AppEvent, AppState, QuickView, Selection, selection_for};
use crate::call::{CallMedia, CallWindows, ZoomStep};
use crate::notice::NoticeAction;
use crate::notify::NotificationCenter;
use crate::settings::{SettingsDialog, SettingsDialogEvent};
use crate::settings_section::SettingsSection;
use crate::sidebar_model::Step;
use crate::theme;
use crate::updater::{self, IdleInputs, UpdateStatus};
use crate::user_activity::{self, UserInput};

pub fn bind_keys(cx: &mut App) {
    super::composer::bind_keys(cx);
    super::shortcuts::bind_scopes(
        cx,
        &[
            Scope::Global,
            Scope::OutsideComposer,
            Scope::GlobalOverTextFields,
            Scope::OutsideTextFields,
        ],
    );
}

#[derive(Debug, Clone, Copy)]
pub enum OpenTarget {
    Chat(usize),
    Channel(usize),
}

#[derive(Default)]
pub struct Startup {
    pub switcher_query: Option<String>,
    pub composer_text: Option<String>,
    pub reply_to_last: bool,
    pub launched_by_autostart: bool,
}

pub struct UserInputHandle(pub Arc<UserInput>);

impl Global for UserInputHandle {}

pub fn install_globals(state: &Entity<AppState>, user_input: Arc<UserInput>, cx: &mut App) {
    cx.set_global(crate::app_state::AppHandle(state.clone()));
    cx.set_global(UserInputHandle(user_input));
    cx.set_global(RenderHooks {
        open_profile_link: super::profile_card::open_profile_link,
        jump_to_quoted,
        inline_image,
        open_link,
    });
}

fn open_link(url: &str, cx: &mut App) {
    if let Some(handle) = cx.try_global::<crate::app_state::AppHandle>() {
        let text = opening_link_text(url);
        handle.0.clone().update(cx, |state, cx| state.raise_notice(text, None, cx));
    }
    cx.open_url(url);
}

fn opening_link_text(url: &str) -> String {
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if host.is_empty() {
        "Opening link...".to_owned()
    } else {
        format!("Opening {host}...")
    }
}

fn inline_image(url: &str, cx: &App) -> Option<std::path::PathBuf> {
    let handle = cx.try_global::<crate::app_state::AppHandle>()?;
    let entry = handle.0.read(cx).directory.image(url)?;
    Some(entry.path.clone())
}

fn jump_to_quoted(message_id: String, cx: &mut App) {
    let Some(handle) = cx.try_global::<crate::app_state::AppHandle>() else {
        return;
    };
    handle.0.clone().update(cx, |state, cx| {
        if let Some(selection) = state.selection.clone() {
            state.jump_to_message(selection, message_id, cx);
        }
    });
}

#[cfg(windows)]
fn watch_releases(running_tag: &'static str, cx: &mut App) {
    use app_base::release_feed;
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(release_feed::FIRST_CHECK_DELAY)
            .await;
        loop {
            let Some(paths) = updater::current_paths() else {
                return;
            };
            let fetch = cx.update(|cx| release_feed::fetch_update(running_tag, paths, cx));
            match fetch.await {
                Ok(Some(tag)) => log::info!("release {tag} downloaded for the next restart"),
                Ok(None) => {}
                Err(error) => log::warn!("release check failed: {error}"),
            }
            cx.background_executor()
                .timer(release_feed::CHECK_INTERVAL)
                .await;
        }
    })
    .detach();
}

const DEMO_UPDATE_VARIABLE: &str = "TEAMS_DEMO_UPDATE";

pub struct AppShell {
    state: Entity<AppState>,
    rail: Entity<RailView>,
    sidebar: Entity<SidebarView>,
    calendar: Entity<CalendarView>,
    quick_pane: Entity<QuickViewPane>,
    manage_team_page: Entity<ManageTeamPage>,
    calls_page: Entity<CallsPage>,
    shown_quick_view: Option<QuickView>,
    conversation: Entity<ConversationView>,
    switcher: Option<Entity<Switcher>>,
    notifications: Entity<NotificationCenter>,
    _call_windows: Entity<CallWindows>,
    forward_dialog: Option<Entity<ForwardDialog>>,
    image_viewer: Option<Entity<ImageViewer>>,
    status_dialog: Option<Entity<StatusMessageDialog>>,
    shortcuts_dialog: Option<Entity<ShortcutsDialog>>,
    agent_dialog: Option<Entity<StartDialog>>,
    settings_dialog: Option<Entity<SettingsDialog>>,
    profile_card: Option<Entity<ProfileCard>>,
    focus_handle: FocusHandle,
    open_target: Option<OpenTarget>,
    update: UpdateStatus,
    splash: Splash,
    toasts: NoticeToasts,
    user_input: Arc<UserInput>,
    _subscription: Subscription,
    _panel_subscriptions: [Subscription; 2],
    _sidebar_subscription: Subscription,
    _agents_subscription: Option<Subscription>,
    _agent_subscription: Option<Subscription>,
    _escape_observation: Subscription,
    _activation_observation: Subscription,
    _appearance_observation: Subscription,
}

impl AppShell {
    #[cfg(test)]
    pub(crate) fn set_update_status(&mut self, update: UpdateStatus, cx: &mut Context<Self>) {
        self.update = update;
        cx.notify();
    }

    fn focus_opened_quick_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let quick_view = self.state.read(cx).quick_view;
        if std::mem::replace(&mut self.shown_quick_view, quick_view) != quick_view
            && quick_view.is_some()
        {
            self.restore_focus(window, cx);
        }
    }

    fn leave_calendar_on_selection(&mut self, cx: &mut Context<Self>) {
        if self.sidebar.read(cx).tab() != SidebarTab::Calendar {
            return;
        }
        let selection = self.state.read(cx).selection.clone();
        let sidebar = self.sidebar.clone();
        cx.defer(move |cx| {
            sidebar.update(cx, |sidebar, cx| match selection {
                Some(Selection::Channel(_)) => sidebar.show_channels(cx),
                Some(Selection::Chat(_)) => sidebar.show_chats(cx),
                None => {}
            })
        });
    }

    pub fn new(
        state: Entity<AppState>,
        open_target: Option<OpenTarget>,
        startup: Startup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let call_media = cx.new(|_| CallMedia);
        state.update(cx, |state, _| state.call_media = Some(call_media.clone()));
        let activity = cx.new(|cx| ActivityCenter::new(state.clone(), window, cx));
        let activity_panel = cx.new(|cx| ActivityPanel::new(state.clone(), activity.clone(), cx));
        let quick_pane = cx.new(|cx| QuickViewPane::new(state.clone(), activity.clone(), cx));
        let manage_team_page = cx.new(|cx| ManageTeamPage::new(state.clone(), window, cx));
        let calls_page = cx.new(|cx| CallsPage::new(state.clone(), window, cx));
        let agents_panel = crate::agents::runs_handle(cx)
            .map(|runs| cx.new(|cx| crate::agents::AgentsPanel::new(runs, cx)));
        let agents_subscription = agents_panel.as_ref().map(|panel| {
            cx.subscribe(
                panel,
                |this, _, event: &crate::agents::AgentsPanelEvent, cx| {
                    let crate::agents::AgentsPanelEvent::Open {
                        conversation_id,
                        message_id,
                    } = event;
                    this.open_message(conversation_id, message_id, cx);
                },
            )
        });
        let panel_subscriptions = [
            cx.subscribe(
                &activity_panel,
                |this, _, event: &ActivityPanelEvent, cx| match event {
                    ActivityPanelEvent::OpenSettings => this.state.update(cx, |state, cx| {
                        state.open_settings(SettingsSection::Notifications, cx)
                    }),
                    ActivityPanelEvent::Open {
                        conversation_id,
                        message_id,
                        link,
                    } => {
                        if !this.open_message(conversation_id, message_id, cx)
                            && let Some(link) = link
                        {
                            cx.open_url(link);
                        }
                    }
                },
            ),
            cx.subscribe(&quick_pane, |this, _, event: &QuickViewEvent, cx| {
                let QuickViewEvent::Open {
                    conversation_id,
                    message_id,
                } = event;
                this.state
                    .update(cx, |state, cx| state.close_quick_view(cx));
                this.open_message(conversation_id, message_id.as_deref().unwrap_or_default(), cx);
            }),
        ];
        let agent_subscription = crate::agents::runs_handle(cx).map(|runs| {
            cx.subscribe_in(
                &runs,
                window,
                |this, _, event: &AgentRunsEvent, window, cx| match event {
                    AgentRunsEvent::StartRequested(request) => {
                        this.open_agent_dialog(request.clone(), window, cx)
                    }
                    AgentRunsEvent::OpenSettings => {
                        this.open_settings(SettingsSection::Agents, window, cx)
                    }
                    AgentRunsEvent::Finished(_) => {}
                },
            )
        });
        let sidebar = cx.new(|cx| SidebarView::new(state.clone(), activity_panel, agents_panel, cx));
        let sidebar_subscription = cx.subscribe_in(&sidebar, window, |this, _, event: &SidebarEvent, window, cx| {
            match event {
                SidebarEvent::RestoreFocus => this.restore_focus(window, cx),
                SidebarEvent::TabChanged => {
                    if this.sidebar.read(cx).tab() == SidebarTab::Calendar {
                        this.restore_focus(window, cx);
                    }
                    cx.notify();
                }
                SidebarEvent::NewChannelPost => this
                    .conversation
                    .update(cx, |conversation, cx| conversation.start_channel_post(window, cx)),
            }
        });
        let calendar = cx.new(|cx| CalendarView::new(state.clone(), cx));
        let rail = cx.new(|cx| RailView::new(state.clone(), activity.clone(), sidebar.clone(), cx));
        let conversation = cx.new(|cx| ConversationView::new(state.clone(), window, cx));
        let subscription =
            cx.subscribe_in(&state, window, |this, _, event: &AppEvent, window, cx| {
                if matches!(event, AppEvent::Sidebar | AppEvent::Status) {
                    this.apply_open_target(cx);
                }
                if matches!(event, AppEvent::Sidebar) {
                    this.focus_opened_quick_view(window, cx);
                }
                if matches!(event, AppEvent::Selection) {
                    this.leave_calendar_on_selection(cx);
                }
                if matches!(event, AppEvent::Forward) {
                    this.open_forward_dialog(window, cx);
                }
                if matches!(event, AppEvent::ImageViewer) {
                    this.open_image_viewer(window, cx);
                }
                if matches!(event, AppEvent::StatusMessage) {
                    this.open_status_dialog(window, cx);
                }
                if let AppEvent::Settings(section) = event {
                    this.open_settings(*section, window, cx);
                }
                if matches!(event, AppEvent::Profile) {
                    this.open_profile_card(window, cx);
                }
                cx.notify();
            });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let notifications =
            cx.new(|cx| NotificationCenter::new(state.clone(), activity.clone(), window, cx));
        let call_windows = cx.new(|cx| CallWindows::new(state.clone(), window, cx));
        let user_input = cx.global::<UserInputHandle>().0.clone();
        let escape_observation = cx.observe_keystrokes(|this, event, _, cx| {
            this.user_input.touch();
            if event.keystroke.key == "escape" && event.action.is_none() {
                this.state
                    .update(cx, |state, cx| state.escape_call_fullscreen(cx));
            }
        });
        let appearance_observation = cx.observe_window_appearance(window, |_, window, cx| {
            crate::active_appearance::system_appearance_changed(window.appearance(), cx);
        });
        let activation_observation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.state
                    .update(cx, |state, cx| state.refresh_calendar(cx));
            } else {
                this.state
                    .update(cx, |state, cx| state.release_push_to_talk(cx));
            }
        });
        let closing = notifications.clone();
        window.on_window_should_close(cx, move |_, cx| !closing.read(cx).intercept_close(cx));
        let mut shell = AppShell {
            state,
            rail,
            sidebar,
            calendar,
            quick_pane,
            manage_team_page,
            calls_page,
            shown_quick_view: None,
            conversation,
            switcher: None,
            notifications,
            _call_windows: call_windows,
            forward_dialog: None,
            image_viewer: None,
            status_dialog: None,
            shortcuts_dialog: None,
            agent_dialog: None,
            settings_dialog: None,
            profile_card: None,
            focus_handle,
            open_target,
            update: UpdateStatus::UpToDate,
            splash: Splash::default(),
            toasts: NoticeToasts::default(),
            user_input,
            _subscription: subscription,
            _panel_subscriptions: panel_subscriptions,
            _sidebar_subscription: sidebar_subscription,
            _agents_subscription: agents_subscription,
            _agent_subscription: agent_subscription,
            _escape_observation: escape_observation,
            _activation_observation: activation_observation,
            _appearance_observation: appearance_observation,
        };
        shell.apply_open_target(cx);
        shell
            .state
            .update(cx, |state, cx| state.refresh_connection_banner(cx));
        if shell.state.read(cx).mode.demo && std::env::var_os(DEMO_UPDATE_VARIABLE).is_some() {
            shell.update = UpdateStatus::Ready;
        }
        let Startup {
            switcher_query,
            composer_text,
            reply_to_last,
            launched_by_autostart,
        } = startup;
        if shell
            .notifications
            .read(cx)
            .startup()
            .starts_hidden(launched_by_autostart)
        {
            cx.defer_in(window, |this, window, cx| {
                this.notifications.read(cx).start_in_background(window)
            });
        }
        if let Some(query) = switcher_query {
            shell.toggle_switcher(&query, window, cx);
        }
        if composer_text.is_some() || reply_to_last {
            cx.defer_in(window, move |this, window, cx| {
                this.conversation.update(cx, |conversation, cx| {
                    if reply_to_last {
                        conversation.reply_to_latest_from_others(window, cx);
                    }
                    if let Some(text) = composer_text {
                        conversation.set_composer_text(&text, window, cx);
                    }
                });
            });
        }
        #[cfg(windows)]
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(updater::POLL_INTERVAL).await;
                if this
                    .update_in(cx, |this, window, cx| this.poll_update(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        #[cfg(windows)]
        if let Some(running_tag) = app_base::version::release_tag() {
            watch_releases(running_tag, cx);
        }
        shell
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn poll_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.update,
            UpdateStatus::Failed(_) | UpdateStatus::Installing
        ) {
            return;
        }
        let Some(paths) = updater::current_paths() else {
            return;
        };
        if self.update == UpdateStatus::UpToDate {
            if !updater::check(&paths) {
                return;
            }
            self.update = UpdateStatus::Ready;
            let update_exe = paths.update_exe.clone();
            cx.background_spawn(async move { updater::prescan(&update_exe) })
                .detach();
        }
        let conversation = self.conversation.read(cx);
        let idle = updater::is_idle(IdleInputs {
            window_active: window.is_window_active(),
            composer_empty: conversation.composer_is_empty(cx),
            send_in_flight: conversation.send_in_flight(),
        });
        if idle {
            self.restart_into_update(cx);
        }
        cx.notify();
    }

    fn restart_into_update(&mut self, cx: &mut Context<Self>) {
        if self.update == UpdateStatus::Installing {
            return;
        }
        let Some(paths) = updater::current_paths() else {
            return;
        };
        self.update = UpdateStatus::Installing;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { updater::install_and_relaunch(&paths) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => cx.quit(),
                    Err(message) => this.update = UpdateStatus::Failed(message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_open_target(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.open_target else {
            return;
        };
        let sidebar = &self.state.read(cx).sidebar;
        let selection = match target {
            OpenTarget::Chat(index) => sidebar
                .chats
                .get(index)
                .map(|chat| Selection::Chat(chat.id.clone())),
            OpenTarget::Channel(index) => sidebar
                .teams
                .iter()
                .flat_map(|team| team.channels.iter())
                .nth(index)
                .map(|channel| Selection::Channel(channel.id.clone())),
        };
        if let Some(selection) = selection {
            self.open_target = None;
            self.state
                .update(cx, |state, cx| state.select(selection, cx));
        }
    }

    fn toggle_switcher(
        &mut self,
        initial_query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.switcher.take().is_some() {
            self.restore_focus(window, cx);
            cx.notify();
            return;
        }
        self.close_image_viewer(window, cx);
        let candidates = candidates_from(self.state.read(cx));
        let app = self.state.clone();
        let switcher = cx.new(|cx| Switcher::new(app, candidates, initial_query, window, cx));
        cx.subscribe_in(
            &switcher,
            window,
            |this, _, event: &SwitcherEvent, window, cx| {
                match event {
                    SwitcherEvent::Pick(selection) => {
                        let selection = selection.clone();
                        this.state
                            .update(cx, |state, cx| state.select(selection, cx));
                    }
                    SwitcherEvent::PickMessage {
                        selection,
                        message_id,
                    } => {
                        let (selection, message_id) = (selection.clone(), message_id.clone());
                        this.state.update(cx, |state, cx| {
                            state.jump_to_message(selection, message_id, cx)
                        });
                    }
                    SwitcherEvent::PickPerson { user_id } => {
                        let user_id = user_id.clone();
                        this.state
                            .update(cx, |state, cx| state.open_chat_with(&user_id, cx));
                    }
                    SwitcherEvent::Close => {}
                }
                this.switcher = None;
                this.restore_focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.switcher = Some(switcher);
        cx.notify();
    }

    fn open_message(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection) = selection_for(&self.state.read(cx).sidebar, conversation_id) else {
            return false;
        };
        let message_id = message_id.to_owned();
        self.state.update(cx, |state, cx| {
            if message_id.is_empty() {
                state.select(selection, cx)
            } else {
                state.jump_to_message(selection, message_id, cx)
            }
        });
        true
    }

    fn open_status_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .state
            .update(cx, |state, _| state.take_status_message_request())
        {
            return;
        }
        self.close_image_viewer(window, cx);
        self.switcher = None;
        let app = self.state.clone();
        let dialog = cx.new(|cx| StatusMessageDialog::new(app, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, _: &StatusMessageDialogEvent, window, cx| {
                this.status_dialog = None;
                this.restore_focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.status_dialog = Some(dialog);
        cx.notify();
    }

    fn open_agent_dialog(
        &mut self,
        request: StartRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(runs) = crate::agents::runs_handle(cx) else {
            return;
        };
        let settings = runs.read(cx).settings();
        let Some(action) = settings
            .actions
            .iter()
            .find(|action| action.id == request.action_id)
            .cloned()
        else {
            return;
        };
        let agent_name = settings
            .agents
            .iter()
            .find(|agent| agent.id == action.agent_id)
            .map(|agent| agent.name.clone())
            .unwrap_or_default();
        self.switcher = None;
        let app = self.state.clone();
        let dialog = cx.new(|cx| StartDialog::new(app, request, action, &agent_name, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, _: &StartDialogEvent, window, cx| {
                this.agent_dialog = None;
                window.focus(&this.focus_handle, cx);
                cx.notify();
            },
        )
        .detach();
        self.agent_dialog = Some(dialog);
        cx.notify();
    }

    fn open_settings(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = &self.settings_dialog {
            dialog.update(cx, |dialog, cx| dialog.show(section, window, cx));
            return;
        }
        self.close_image_viewer(window, cx);
        self.switcher = None;
        let (app, center) = (self.state.clone(), self.notifications.clone());
        let dialog = cx.new(|cx| SettingsDialog::new(app, center, section, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, _: &SettingsDialogEvent, window, cx| {
                this.settings_dialog = None;
                this.restore_focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.settings_dialog = Some(dialog);
        cx.notify();
    }

    fn toggle_shortcuts_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcuts_dialog.take().is_some() {
            self.restore_focus(window, cx);
            cx.notify();
            return;
        }
        self.close_image_viewer(window, cx);
        self.switcher = None;
        let dialog = cx.new(|cx| ShortcutsDialog::new(window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, _: &ShortcutsDialogEvent, window, cx| {
                this.shortcuts_dialog = None;
                this.restore_focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.shortcuts_dialog = Some(dialog);
        cx.notify();
    }

    fn open_profile_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self
            .state
            .update(cx, |state, _| state.take_profile_request())
        else {
            return;
        };
        self.close_image_viewer(window, cx);
        if let Some(card) = &self.profile_card {
            card.update(cx, |card, cx| {
                card.move_to(request.user_id, request.anchor, cx)
            });
            return;
        }
        let app = self.state.clone();
        let card = cx.new(|cx| ProfileCard::new(app, request.user_id, request.anchor, window, cx));
        cx.subscribe_in(
            &card,
            window,
            |this, _, _: &ProfileCardEvent, window, cx| {
                this.profile_card = None;
                this.restore_focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.profile_card = Some(card);
        cx.notify();
    }

    fn open_image_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self
            .state
            .update(cx, |state, _| state.take_image_viewer_request())
        else {
            return;
        };
        self.switcher = None;
        let app = self.state.clone();
        let viewer = cx.new(|cx| ImageViewer::new(app, request, window, cx));
        cx.subscribe_in(
            &viewer,
            window,
            |this, _, _: &ImageViewerEvent, window, cx| {
                this.image_viewer = None;
                window.focus(&this.focus_handle, cx);
                cx.notify();
            },
        )
        .detach();
        self.image_viewer = Some(viewer);
        cx.notify();
    }

    fn start_call_in_open_chat(&mut self, with_video: bool, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            if let Some(Selection::Chat(chat_id)) = state.selection.clone() {
                if with_video {
                    state.start_chat_video_call(&chat_id, cx);
                } else {
                    state.start_chat_call(&chat_id, cx);
                }
            }
        });
    }

    fn close_image_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.image_viewer.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    fn restore_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.image_viewer {
            Some(viewer) => {
                let handle = viewer.read(cx).focus_handle();
                window.focus(&handle, cx)
            }
            None => window.focus(&self.focus_handle, cx),
        }
    }

    fn open_forward_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self
            .state
            .update(cx, |state, _| state.take_forward_request())
        else {
            return;
        };
        self.switcher = None;
        let candidates = candidates_from(self.state.read(cx));
        let app = self.state.clone();
        let dialog = cx.new(|cx| ForwardDialog::new(app, source, candidates, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, event: &ForwardDialogEvent, window, cx| {
                this.forward_dialog = None;
                this.restore_focus(window, cx);
                if let ForwardDialogEvent::Sent { target, title } = event {
                    let target = target.clone();
                    let open = NoticeAction::new("Open", move |state, cx| {
                        state.select(target.clone(), cx)
                    });
                    this.state.update(cx, |state, cx| {
                        state.cancel_forward_selection(cx);
                        state.raise_notice(format!("Forwarded to {title}"), Some(open), cx)
                    });
                }
                cx.notify();
            },
        )
        .detach();
        self.forward_dialog = Some(dialog);
        cx.notify();
    }
}

impl AppShell {
    fn render_splash(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let phase = if self.splash.is_up() {
            let opening = self.conversation.read(cx).opening_title();
            let state = self.state.read(cx);
            let phase = splash::phase(&StartupFacts {
                elapsed: self.splash.elapsed(),
                connection: &state.connection,
                has_chats: !state.sidebar.chats.is_empty() || !state.sidebar.teams.is_empty(),
                synced: state.last_sync.is_some(),
                demo: state.mode.demo,
                opening,
            });
            if phase != Phase::Ready && cx.reduce_motion() && !self.splash.recheck_scheduled {
                self.splash.recheck_scheduled = true;
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(splash::STILL_RECHECK).await;
                    this.update(cx, |this, cx| {
                        this.splash.recheck_scheduled = false;
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            if phase == Phase::Ready {
                self.splash.hide();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(splash::CONTENT_RISE).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
            }
            phase
        } else {
            Phase::Ready
        };
        self.splash.render(&phase, cx)
    }

    fn islands(
        &self,
        show_sidebar: bool,
        calendar_open: bool,
        quick_view_open: bool,
        calls_open: bool,
        manage_team_open: bool,
    ) -> Div {
        h_flex()
            .debug_selector(|| "shell/islands".into())
            .flex_1()
            .min_h_0()
            .w_full()
            .when(show_sidebar, |islands| islands.gap(px(ISLAND_GAP)))
            .pr(px(ISLAND_GAP))
            .pb(px(ISLAND_GAP))
            .child(
                h_flex()
                    .h_full()
                    .flex_none()
                    .child(self.rail.clone())
                    .when(show_sidebar, |group| {
                        group.child(
                            self.sidebar.clone().cached(
                                StyleRefinement::default()
                                    .w(px(SIDEBAR_WIDTH))
                                    .h_full()
                                    .flex_none(),
                            ),
                        )
                    }),
            )
            .child(match () {
                _ if calls_open => self
                    .calls_page
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().h_full())
                    .into_any_element(),
                _ if manage_team_open => self
                    .manage_team_page
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().h_full())
                    .into_any_element(),
                _ if quick_view_open => self
                    .quick_pane
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().h_full())
                    .into_any_element(),
                _ if calendar_open => self
                    .calendar
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().h_full())
                    .into_any_element(),
                _ => self
                    .conversation
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().h_full())
                    .into_any_element(),
            })
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open_dialogs = [
            self.state.read(cx).task_dialog.is_some(),
            self.forward_dialog.is_some(),
            self.status_dialog.is_some(),
            self.shortcuts_dialog.is_some(),
            self.agent_dialog.is_some(),
            self.settings_dialog.is_some(),
        ]
        .into_iter()
        .filter(|open| *open)
        .count();
        elevation::set_app_modal_layers(window, open_dialogs, cx);
        if open_dialogs > 0 {
            crate::inline_video::stop(cx);
        }
        if let Some(dialog) = self
            .state
            .read(cx)
            .task_dialog
            .as_ref()
            .map(|dialog| (dialog.scope.clone(), dialog.card.clone()))
        {
            self.state.update(cx, |state, cx| {
                state.ensure_card_inputs(&dialog.0, &dialog.1, window, cx)
            });
        }
        if let Some(me) = self.state.read(cx).directory.me.clone() {
            let app = self.state.clone();
            cx.defer(move |cx| {
                app.update(cx, |state, cx| {
                    state.request_avatars(vec![me.user_id.clone()], cx);
                    state.request_presence(vec![me.user_id], cx);
                });
            });
        }
        let notice = self.state.read(cx).notice.clone();
        self.toasts.sync(&self.state, notice.as_ref(), window, cx);
        let update_failure = match &self.update {
            UpdateStatus::Failed(message) => Some(message.clone()),
            _ => None,
        };
        self.toasts.update_failed(update_failure.as_deref(), window, cx);
        let calendar_tab = self.sidebar.read(cx).tab() == SidebarTab::Calendar;
        let chats_tab = self.sidebar.read(cx).tab() == SidebarTab::Chats;
        let calls_tab = self.sidebar.read(cx).tab() == SidebarTab::Calls;
        let splash = self.render_splash(cx);
        let state = self.state.read(cx);
        let calendar_open = calendar_tab;
        let quick_view_open = chats_tab && state.quick_view.is_some();
        let manage_team_open = !calendar_tab && !calls_tab && state.manage_team.is_some();
        let calls_open = calls_tab;
        let show_sidebar = !calendar_open && !calls_open;
        let banner = state.connection_banner_text().filter(|_| !self.splash.is_up()).map(|banner| {
            render_connection_banner(
                banner,
                cx.listener(|this, _, _, cx| {
                    this.state.update(cx, |state, _| state.retry_live())
                }),
            )
        });
        let title_bar = render_title_bar(
            crate::active_appearance::current(cx).app_icon,
            mode_label(state.mode),
            [
                update_button(
                    &self.update,
                    cx.listener(|this, _, _, cx| this.restart_into_update(cx)),
                ),
                Some(settings_more_button(&self.state)),
                own_status_button(&self.state, state),
            ]
            .into_iter()
            .flatten(),
            cx.listener(|this, _, window, cx| this.toggle_switcher("", window, cx)),
        );
        let shell = div()
            .id("app-shell")
            .key_context("AppShell")
            .track_focus(&self.focus_handle);
        self.team_keys(shell, cx)
            .on_action(cx.listener(|this, _: &OpenSwitcher, window, cx| {
                this.toggle_switcher("", window, cx);
            }))
            .on_action(cx.listener(|this, _: &NewChat, window, cx| {
                this.close_image_viewer(window, cx);
                this.state.update(cx, |state, cx| state.start_new_chat(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleCallMute, _, cx| {
                this.state
                    .update(cx, |state, cx| state.toggle_call_mute(cx));
            }))
            .on_action(cx.listener(|this, _: &PushToTalk, _, cx| {
                this.state
                    .update(cx, |state, cx| state.press_push_to_talk(cx));
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" {
                    this.state
                        .update(cx, |state, cx| state.release_push_to_talk(cx));
                }
            }))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                if !event.modifiers.control {
                    this.state
                        .update(cx, |state, cx| state.release_push_to_talk(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_settings(SettingsSection::General, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowShortcuts, window, cx| {
                this.toggle_shortcuts_dialog(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenActivity, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.show_activity(cx));
            }))
            .on_action(cx.listener(|this, _: &OpenSaved, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.open_quick_view(QuickView::Saved, cx));
            }))
            .on_action(cx.listener(|this, _: &OpenCalendar, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.show_calendar(cx));
            }))
            .on_action(cx.listener(|this, _: &OpenAgents, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.show_agents(cx));
            }))
            .on_action(cx.listener(|this, _: &OpenChatsTab, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.show_chats(cx));
            }))
            .on_action(cx.listener(|this, _: &FindInChat, window, cx| {
                this.close_image_viewer(window, cx);
                this.state
                    .update(cx, |state, cx| state.close_quick_view(cx));
                this.conversation
                    .update(cx, |conversation, cx| conversation.open_find(window, cx));
            }))
            .on_action(cx.listener(|this, _: &PopOutSelectedChat, _, cx| {
                let selected = this.state.read(cx).selection.clone();
                if let Some(Selection::Chat(chat_id)) = selected {
                    chat_pop_out::pop_out_chat(&this.state, &chat_id, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenChatFilter, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.toggle_filter_box(window, cx));
            }))
            .on_action(cx.listener(|this, _: &FilterUnreadChats, _, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.toggle_pill(ChatFilterPill::Unread, cx));
            }))
            .on_action(cx.listener(|this, _: &FilterNonMeetingChats, _, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.toggle_pill(ChatFilterPill::Chats, cx));
            }))
            .on_action(cx.listener(|this, _: &FilterMeetingChats, _, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.toggle_pill(ChatFilterPill::Meetings, cx));
            }))
            .on_action(cx.listener(|this, _: &FilterMutedChats, _, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.toggle_pill(ChatFilterPill::Muted, cx));
            }))
            .on_action(cx.listener(|this, _: &ClearChatFilters, window, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.clear_filters(window, cx));
            }))
            .on_action(cx.listener(|this, _: &OpenChannelsTab, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.show_channels(cx));
            }))
            .on_action(cx.listener(|this, _: &OpenCallsTab, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.show_calls(cx));
            }))
            .on_action(cx.listener(|this, _: &PreviousConversation, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar.update(cx, |sidebar, cx| {
                    sidebar.select_adjacent(Step::Previous, cx)
                });
            }))
            .on_action(cx.listener(|this, _: &NextConversation, window, cx| {
                this.close_image_viewer(window, cx);
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.select_adjacent(Step::Next, cx));
            }))
            .on_action(cx.listener(|this, _: &DeclineCall, _, cx| {
                this.state
                    .update(cx, |state, cx| state.decline_ringing_call(cx));
            }))
            .on_action(cx.listener(|this, _: &JoinMeetingFromToast, _, cx| {
                this.state
                    .update(cx, |state, cx| state.join_latest_meeting_toast(cx));
            }))
            .on_action(cx.listener(|this, _: &HangUpCall, _, cx| {
                this.state.update(cx, |state, cx| state.leave_call(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleCallCamera, _, cx| {
                this.state
                    .update(cx, |state, cx| state.toggle_call_camera(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleCallShare, _, cx| {
                this.state
                    .update(cx, |state, cx| state.toggle_call_share(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleCallHand, _, cx| {
                this.state
                    .update(cx, |state, cx| state.toggle_call_hand(cx));
            }))
            .on_action(cx.listener(|this, _: &OpenLobbyList, _, cx| {
                this.state
                    .update(cx, |state, cx| state.open_call_lobby_list(cx));
            }))
            .on_action(cx.listener(|this, _: &StartAudioCall, _, cx| {
                this.start_call_in_open_chat(false, cx);
            }))
            .on_action(cx.listener(|this, _: &StartVideoCall, _, cx| {
                this.start_call_in_open_chat(true, cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| change_zoom(&this.state, WindowZoom::In, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| change_zoom(&this.state, WindowZoom::Out, cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| change_zoom(&this.state, WindowZoom::Reset, cx)))
            .on_action(zoom_listener::<ZoomShareIn>(&self.state, ZoomStep::In))
            .on_action(zoom_listener::<ZoomShareOut>(&self.state, ZoomStep::Out))
            .on_action(zoom_listener::<ZoomShareReset>(&self.state, ZoomStep::Reset))
            .on_action(cx.listener(|this, _: &JumpToLastReadOrNewest, _, cx| {
                if this.image_viewer.is_some() {
                    cx.propagate();
                    return;
                }
                this.conversation
                    .update(cx, |conversation, cx| conversation.jump_to_last_read_or_newest(cx));
            }))
            .on_action(cx.listener(|this, _: &ReplyToHovered, window, cx| {
                this.close_image_viewer(window, cx);
                this.conversation.update(cx, |conversation, cx| {
                    conversation.reply_to_hovered(window, cx)
                });
            }))
            .relative()
            .size_full()
            .bg(theme::background())
            .text_color(theme::text())
            .text_size(px(14.))
            .child(crate::frame_log::probe("first"))
            .child(user_activity::input_probe(self.user_input.clone()))
            .child(app_ui_kit::long_press::long_press_right_click())
            .child(
                v_flex()
                    .debug_selector(|| "shell".into())
                    .size_full()
                    .child(title_bar)
                    .children(banner)
                    .child(rising_in(self.islands(show_sidebar, calendar_open, quick_view_open, calls_open, manage_team_open), &self.splash, cx)),
            )
            .children(splash)
            .children(self.image_viewer.clone())
            .children(self.switcher.clone())
            .children(render_task_dialog(self.state.read(cx), cx))
            .children(self.forward_dialog.clone())
            .children(self.status_dialog.clone())
            .children(self.shortcuts_dialog.clone())
            .children(self.agent_dialog.clone())
            .children(self.settings_dialog.clone())
            .children(self.profile_card.clone())
            .child(crate::frame_log::probe("last"))
    }
}

fn rising_in(islands: Div, splash: &Splash, cx: &App) -> AnyElement {
    if splash.content_rising().is_none() || cx.reduce_motion() {
        return islands.into_any_element();
    }
    islands
        .relative()
        .with_animation("startup-content-rise", Animation::new(splash::CONTENT_RISE).with_easing(ease_out_quint()), |islands, delta| {
            islands.top(px(splash::CONTENT_RISE_OFFSET * (1. - delta))).opacity(delta)
        })
        .into_any_element()
}

#[cfg(test)]
mod opening_link_tests {
    use super::opening_link_text;

    #[test]
    fn the_notice_names_the_host() {
        assert_eq!(opening_link_text("https://example.com/a?b#c"), "Opening example.com...");
        assert_eq!(opening_link_text("mailto:"), "Opening mailto:...");
        assert_eq!(opening_link_text(""), "Opening link...");
    }
}

