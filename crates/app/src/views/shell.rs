use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::activity_panel::{ActivityPanel, ActivityPanelEvent};
use super::conversation::{ConversationView, ReplyToHovered};
use super::dialog_overlay::render_task_dialog;
use super::forward_dialog::{ForwardDialog, ForwardDialogEvent};
use super::saved_panel::{SavedPanel, SavedPanelEvent};
use super::sidebar::SidebarView;
use super::status_bar::render_status_bar;
use super::switcher::{Switcher, SwitcherEvent, candidates_from};
use super::title_bar::render_title_bar;
use crate::activity::ActivityCenter;
use crate::app_state::{AppEvent, AppState, Selection};
use crate::notice::NoticeAction;
use crate::notify::{NotificationCenter, selection_for};
use crate::theme;
use crate::updater::{self, IdleInputs, UpdateStatus};

actions!(teams, [OpenSwitcher, NewChat]);

pub fn bind_keys(cx: &mut App) {
    super::composer::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("ctrl-k", OpenSwitcher, None),
        KeyBinding::new("ctrl-n", NewChat, None),
        KeyBinding::new("alt-r", ReplyToHovered, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-k", OpenSwitcher, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-n", NewChat, None),
    ]);
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
}

pub struct AppShell {
    state: Entity<AppState>,
    sidebar: Entity<SidebarView>,
    conversation: Entity<ConversationView>,
    switcher: Option<Entity<Switcher>>,
    notifications: Entity<NotificationCenter>,
    activity: Entity<ActivityCenter>,
    activity_panel: Option<Entity<ActivityPanel>>,
    saved_panel: Option<Entity<SavedPanel>>,
    forward_dialog: Option<Entity<ForwardDialog>>,
    focus_handle: FocusHandle,
    open_target: Option<OpenTarget>,
    update: UpdateStatus,
    _subscription: Subscription,
    _activity_observation: Subscription,
}

impl AppShell {
    pub fn new(
        state: Entity<AppState>,
        open_target: Option<OpenTarget>,
        startup: Startup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sidebar = cx.new(|cx| SidebarView::new(state.clone(), cx));
        let conversation = cx.new(|cx| ConversationView::new(state.clone(), window, cx));
        let subscription =
            cx.subscribe_in(&state, window, |this, _, event: &AppEvent, window, cx| {
                if matches!(event, AppEvent::Sidebar | AppEvent::Status) {
                    this.apply_open_target(cx);
                }
                if matches!(event, AppEvent::Forward) {
                    this.open_forward_dialog(window, cx);
                }
                cx.notify();
            });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let notifications = cx.new(|cx| NotificationCenter::new(state.clone(), window, cx));
        let activity = cx.new(|cx| ActivityCenter::new(state.clone(), window, cx));
        let activity_observation = cx.observe(&activity, |_, _, cx| cx.notify());
        let closing = notifications.clone();
        window.on_window_should_close(cx, move |_, cx| !closing.read(cx).intercept_close(cx));
        let mut shell = AppShell {
            state,
            sidebar,
            conversation,
            switcher: None,
            notifications,
            activity,
            activity_panel: None,
            saved_panel: None,
            forward_dialog: None,
            focus_handle,
            open_target,
            update: UpdateStatus::UpToDate,
            _subscription: subscription,
            _activity_observation: activity_observation,
        };
        shell.apply_open_target(cx);
        let Startup {
            switcher_query,
            composer_text,
            reply_to_last,
        } = startup;
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
        shell
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn poll_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.update, UpdateStatus::Failed(_)) {
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
        let Some(paths) = updater::current_paths() else {
            return;
        };
        match updater::install_and_relaunch(&paths) {
            Ok(()) => cx.quit(),
            Err(message) => self.update = UpdateStatus::Failed(message),
        }
        cx.notify();
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
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        self.activity_panel = None;
        self.saved_panel = None;
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
                    SwitcherEvent::Close => {}
                }
                this.switcher = None;
                window.focus(&this.focus_handle, cx);
                cx.notify();
            },
        )
        .detach();
        self.switcher = Some(switcher);
        cx.notify();
    }

    fn toggle_activity_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_panel.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        self.switcher = None;
        self.saved_panel = None;
        let (app, activity) = (self.state.clone(), self.activity.clone());
        let panel = cx.new(|cx| ActivityPanel::new(app, activity, window, cx));
        cx.subscribe_in(
            &panel,
            window,
            |this, _, event: &ActivityPanelEvent, window, cx| {
                this.activity_panel = None;
                window.focus(&this.focus_handle, cx);
                match event {
                    ActivityPanelEvent::Close => {}
                    ActivityPanelEvent::OpenSettings => this
                        .notifications
                        .update(cx, |center, cx| center.open_settings(cx)),
                    ActivityPanelEvent::Open {
                        conversation_id,
                        message_id,
                    } => {
                        let selection =
                            selection_for(&this.state.read(cx).sidebar, conversation_id);
                        if let Some(selection) = selection {
                            let message_id = message_id.clone();
                            this.state.update(cx, |state, cx| {
                                if message_id.is_empty() {
                                    state.select(selection, cx)
                                } else {
                                    state.jump_to_message(selection, message_id, cx)
                                }
                            });
                        }
                    }
                }
                cx.notify();
            },
        )
        .detach();
        self.activity_panel = Some(panel);
        cx.notify();
    }
    fn toggle_saved_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saved_panel.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        self.switcher = None;
        self.activity_panel = None;
        let app = self.state.clone();
        let panel = cx.new(|cx| SavedPanel::new(app, window, cx));
        cx.subscribe_in(
            &panel,
            window,
            |this, _, event: &SavedPanelEvent, window, cx| {
                this.saved_panel = None;
                window.focus(&this.focus_handle, cx);
                if let SavedPanelEvent::Open {
                    conversation_id,
                    message_id,
                } = event
                {
                    let selection = selection_for(&this.state.read(cx).sidebar, conversation_id);
                    if let Some(selection) = selection {
                        let message_id = message_id.clone();
                        this.state.update(cx, |state, cx| {
                            state.jump_to_message(selection, message_id, cx)
                        });
                    }
                }
                cx.notify();
            },
        )
        .detach();
        self.saved_panel = Some(panel);
        cx.notify();
    }

    fn open_forward_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self
            .state
            .update(cx, |state, _| state.take_forward_request())
        else {
            return;
        };
        self.switcher = None;
        self.activity_panel = None;
        self.saved_panel = None;
        let candidates = candidates_from(self.state.read(cx));
        let app = self.state.clone();
        let dialog = cx.new(|cx| ForwardDialog::new(app, source, candidates, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, event: &ForwardDialogEvent, window, cx| {
                this.forward_dialog = None;
                window.focus(&this.focus_handle, cx);
                if let ForwardDialogEvent::Sent { target, title } = event {
                    let target = target.clone();
                    let open = NoticeAction::new("Open", move |state, cx| {
                        state.select(target.clone(), cx)
                    });
                    this.state.update(cx, |state, cx| {
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

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let state = self.state.read(cx);
        let status = render_status_bar(
            state,
            &self.update,
            cx.listener(|this, _, _, cx| this.restart_into_update(cx)),
            cx.listener(|this, _, _, cx| {
                this.state
                    .update(cx, |state, cx| state.run_notice_action(cx))
            }),
        );
        let title_bar = render_title_bar(
            &state.directory,
            self.activity.read(cx).feed().unread_count(),
            cx.listener(|this, _, window, cx| this.toggle_switcher("", window, cx)),
            cx.listener(|this, _, window, cx| this.toggle_saved_panel(window, cx)),
            cx.listener(|this, _, window, cx| this.toggle_activity_panel(window, cx)),
        );
        div()
            .id("app-shell")
            .key_context("AppShell")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &OpenSwitcher, window, cx| {
                this.toggle_switcher("", window, cx);
            }))
            .on_action(cx.listener(|this, _: &NewChat, _, cx| {
                this.state.update(cx, |state, cx| state.start_new_chat(cx));
            }))
            .on_action(cx.listener(|this, _: &ReplyToHovered, window, cx| {
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
            .child(
                v_flex()
                    .size_full()
                    .child(title_bar)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(self.sidebar.clone())
                            .child(self.conversation.clone()),
                    )
                    .child(status),
            )
            .children(self.activity_panel.clone())
            .children(self.saved_panel.clone())
            .children(self.switcher.clone())
            .children(render_task_dialog(self.state.read(cx), cx))
            .children(self.forward_dialog.clone())
            .child(crate::frame_log::probe("last"))
    }
}
