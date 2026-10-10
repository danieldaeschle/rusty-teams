use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::activity_panel::{ActivityPanel, ActivityPanelEvent};
use super::call_mini::render_call_mini;
use super::call_stage::render_stage_overlay;
use super::call_view::render_call_view;
use super::conversation::{ConversationView, ReplyToHovered};
use super::dialog_overlay::render_task_dialog;
use super::forward_dialog::{ForwardDialog, ForwardDialogEvent};
use super::profile_card::{ProfileCard, ProfileCardEvent};
use super::saved_panel::{SavedPanel, SavedPanelEvent};
use super::shortcuts::{
    AcceptCall, DeclineCall, HangUpCall, NextConversation, OpenActivity, OpenChannelsTab,
    OpenChatsTab, OpenSaved, PreviousConversation, Scope, ShowShortcuts, ToggleCallCamera,
    ToggleCallShare,
};
use super::shortcuts_dialog::{ShortcutsDialog, ShortcutsDialogEvent};
use super::sidebar::SidebarView;
use super::status_bar::render_status_bar;
use super::status_menu::own_status_button;
use super::status_message_dialog::{StatusMessageDialog, StatusMessageDialogEvent};
use super::switcher::{Switcher, SwitcherEvent, candidates_from};
use super::title_bar::render_title_bar;
use crate::activity::ActivityCenter;
use crate::app_state::{AppEvent, AppState, Selection};
use crate::embedded_web::{Overlays, RootFocus};
use crate::notice::NoticeAction;
use crate::notify::{NotificationCenter, selection_for};
use crate::sidebar_model::Step;
use crate::theme;
use crate::updater::{self, IdleInputs, UpdateStatus};

actions!(teams, [OpenSwitcher, NewChat, ToggleCallMute]);

pub fn bind_keys(cx: &mut App) {
    super::composer::bind_keys(cx);
    super::shortcuts::bind_scopes(cx, &[Scope::Global, Scope::OutsideComposer, Scope::GlobalOverTextFields]);
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
    status_dialog: Option<Entity<StatusMessageDialog>>,
    shortcuts_dialog: Option<Entity<ShortcutsDialog>>,
    profile_card: Option<Entity<ProfileCard>>,
    focus_handle: FocusHandle,
    open_target: Option<OpenTarget>,
    update: UpdateStatus,
    _subscription: Subscription,
    _activity_observation: Subscription,
    _escape_observation: Subscription,
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
                if matches!(event, AppEvent::StatusMessage) {
                    this.open_status_dialog(window, cx);
                }
                if matches!(event, AppEvent::NotificationSettings) {
                    this.notifications
                        .update(cx, |center, cx| center.open_settings(cx));
                }
                if matches!(event, AppEvent::Profile) {
                    this.open_profile_card(window, cx);
                }
                cx.notify();
            });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        cx.set_global(RootFocus(focus_handle.clone()));
        let notifications = cx.new(|cx| NotificationCenter::new(state.clone(), window, cx));
        let activity = cx.new(|cx| ActivityCenter::new(state.clone(), window, cx));
        let activity_observation = cx.observe(&activity, |_, _, cx| cx.notify());
        let escape_observation = cx.observe_keystrokes(|this, event, _, cx| {
            if event.keystroke.key == "escape" {
                this.state.update(cx, |state, cx| state.leave_stage_fullscreen(cx));
            }
        });
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
            status_dialog: None,
            shortcuts_dialog: None,
            profile_card: None,
            focus_handle,
            open_target,
            update: UpdateStatus::UpToDate,
            _subscription: subscription,
            _activity_observation: activity_observation,
            _escape_observation: escape_observation,
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

    fn open_status_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .state
            .update(cx, |state, _| state.take_status_message_request())
        {
            return;
        }
        self.switcher = None;
        self.activity_panel = None;
        self.saved_panel = None;
        let app = self.state.clone();
        let dialog = cx.new(|cx| StatusMessageDialog::new(app, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, _: &StatusMessageDialogEvent, window, cx| {
                this.status_dialog = None;
                window.focus(&this.focus_handle, cx);
                cx.notify();
            },
        )
        .detach();
        self.status_dialog = Some(dialog);
        cx.notify();
    }

    fn toggle_shortcuts_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcuts_dialog.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        self.switcher = None;
        self.activity_panel = None;
        self.saved_panel = None;
        let dialog = cx.new(|cx| ShortcutsDialog::new(window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            |this, _, _: &ShortcutsDialogEvent, window, cx| {
                this.shortcuts_dialog = None;
                window.focus(&this.focus_handle, cx);
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
                window.focus(&this.focus_handle, cx);
                cx.notify();
            },
        )
        .detach();
        self.profile_card = Some(card);
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
        let panels = self.switcher.is_some()
            || self.activity_panel.is_some()
            || self.saved_panel.is_some()
            || self.forward_dialog.is_some()
            || self.status_dialog.is_some()
            || self.shortcuts_dialog.is_some()
            || self.profile_card.is_some()
            || {
                let state = self.state.read(cx);
                state.task_dialog.is_some() || state.status_menu_open
            };
        Overlays::update(cx, |overlays| overlays.panels = panels);
        let state = self.state.read(cx);
        let call_view = render_call_view(&self.state, state).filter(|_| state.viewing_call());
        let call_mini = render_call_mini(&self.state, state);
        let stage_overlay = render_stage_overlay(&self.state, state);
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
            own_status_button(&self.state, state),
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
            .on_action(cx.listener(|this, _: &ToggleCallMute, _, cx| {
                this.state.update(cx, |state, cx| state.toggle_call_mute(cx));
            }))
            .on_action(cx.listener(|this, _: &ShowShortcuts, window, cx| {
                this.toggle_shortcuts_dialog(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenActivity, window, cx| {
                this.toggle_activity_panel(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSaved, window, cx| {
                this.toggle_saved_panel(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenChatsTab, _, cx| {
                this.sidebar.update(cx, |sidebar, cx| sidebar.show_chats(cx));
            }))
            .on_action(cx.listener(|this, _: &OpenChannelsTab, _, cx| {
                this.sidebar.update(cx, |sidebar, cx| sidebar.show_channels(cx));
            }))
            .on_action(cx.listener(|this, _: &PreviousConversation, _, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.select_adjacent(Step::Previous, cx));
            }))
            .on_action(cx.listener(|this, _: &NextConversation, _, cx| {
                this.sidebar
                    .update(cx, |sidebar, cx| sidebar.select_adjacent(Step::Next, cx));
            }))
            .on_action(cx.listener(|this, _: &AcceptCall, _, cx| {
                this.state.update(cx, |state, cx| state.accept_ringing_call(cx));
            }))
            .on_action(cx.listener(|this, _: &DeclineCall, _, cx| {
                this.state.update(cx, |state, cx| state.decline_ringing_call(cx));
            }))
            .on_action(cx.listener(|this, _: &HangUpCall, _, cx| {
                this.state.update(cx, |state, cx| state.leave_call(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleCallCamera, _, cx| {
                this.state.update(cx, |state, cx| state.toggle_call_camera(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleCallShare, _, cx| {
                this.state.update(cx, |state, cx| state.toggle_call_share(cx));
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
                            .child(match call_view {
                                Some(call_view) => call_view,
                                None => self.conversation.clone().into_any_element(),
                            }),
                    )
                    .child(status),
            )
            .children(call_mini)
            .children(stage_overlay)
            .children(self.activity_panel.clone())
            .children(self.saved_panel.clone())
            .children(self.switcher.clone())
            .children(render_task_dialog(self.state.read(cx), cx))
            .children(self.forward_dialog.clone())
            .children(self.status_dialog.clone())
            .children(self.shortcuts_dialog.clone())
            .children(self.profile_card.clone())
            .child(crate::frame_log::probe("last"))
    }
}
