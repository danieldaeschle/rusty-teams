use std::time::{Duration, Instant};

use calling::{CallCommand, CallHandle, CallUpdate, DeviceChoice, MuteCommand};
use gpui_kit::*;

use super::demo::start_demo_call;
use super::model::{ActiveCall, CallModel, TEST_CALL_TITLE, ended_notice};
use crate::app_state::{AppEvent, AppState};

const TIMER_REFRESH: Duration = Duration::from_millis(500);
const NOT_CONNECTED_NOTICE: &str = "Not connected to Teams yet";
const READ_ONLY_NOTICE: &str = "Calls are off in read-only mode";

impl AppState {
    pub fn start_test_call(&mut self, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        let handle = if self.mode.demo {
            start_demo_call()
        } else if self.mode.read_only {
            self.raise_notice(READ_ONLY_NOTICE.to_owned(), None, cx);
            return;
        } else if let Some(launcher) = self.call_launcher.clone() {
            launcher.start_test_call()
        } else {
            self.raise_notice(NOT_CONNECTED_NOTICE.to_owned(), None, cx);
            return;
        };
        self.call_count += 1;
        let call_id = self.call_count;
        let CallHandle { commands, updates } = handle;
        self.call = Some(ActiveCall {
            id: call_id,
            model: CallModel::new(TEST_CALL_TITLE),
            commands,
            viewing: true,
        });
        self.follow_call(call_id, updates, cx);
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    fn follow_call(
        &mut self,
        call_id: u64,
        mut updates: tokio::sync::mpsc::UnboundedReceiver<CallUpdate>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.recv().await {
                let alive = this.update(cx, |state, cx| state.apply_call_update(call_id, update, cx));
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TIMER_REFRESH).await;
                let running = this.update(cx, |state, cx| {
                    let running = state.call.as_ref().is_some_and(|call| call.id == call_id);
                    if running {
                        cx.notify();
                    }
                    running
                });
                if !running.unwrap_or(false) {
                    return;
                }
            }
        })
        .detach();
    }

    fn apply_call_update(&mut self, call_id: u64, update: CallUpdate, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.id == call_id) else {
            return;
        };
        call.model.apply(update);
        let Some(reason) = call.model.ended_reason().cloned() else {
            cx.emit(AppEvent::Call);
            cx.notify();
            return;
        };
        let elapsed = call.model.elapsed(Instant::now());
        self.call = None;
        self.raise_notice(ended_notice(&reason, elapsed), None, cx);
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    fn send_call_command(&self, command: CallCommand) {
        if let Some(call) = &self.call {
            let _ = call.commands.send(command);
        }
    }

    pub fn toggle_call_mute(&mut self, _cx: &mut Context<Self>) {
        let can_toggle = self.call.as_ref().is_some_and(|call| call.model.can_unmute());
        if can_toggle {
            self.send_call_command(CallCommand::Mute(MuteCommand::Toggle));
        }
    }

    pub fn leave_call(&mut self, _cx: &mut Context<Self>) {
        self.send_call_command(CallCommand::Hangup);
    }

    pub fn select_call_input(&mut self, choice: DeviceChoice, _cx: &mut Context<Self>) {
        self.send_call_command(CallCommand::SelectInput(choice));
    }

    pub fn select_call_output(&mut self, choice: DeviceChoice, _cx: &mut Context<Self>) {
        self.send_call_command(CallCommand::SelectOutput(choice));
    }

    pub fn show_call(&mut self, viewing: bool, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.viewing != viewing) else {
            return;
        };
        call.viewing = viewing;
        if viewing {
            self.new_chat = false;
        }
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn viewing_call(&self) -> bool {
        self.call.as_ref().is_some_and(|call| call.viewing)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use calling::{CallState, CallUpdate, EndReason, call_channel};
    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use store::Store;

    use super::{ActiveCall, CallModel, TEST_CALL_TITLE};
    use crate::app_state::{AppState, Mode, Selection};

    fn app_with_call(cx: &mut TestAppContext) -> Entity<AppState> {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let (handle, _control) = call_channel();
        cx.update(|cx| {
            app.update(cx, |state, _| {
                state.call = Some(ActiveCall {
                    id: 7,
                    model: CallModel::new(TEST_CALL_TITLE),
                    commands: handle.commands,
                    viewing: true,
                });
            })
        });
        app
    }

    #[gpui_kit::test]
    fn picking_another_chat_moves_the_call_to_the_mini_window(cx: &mut TestAppContext) {
        let app = app_with_call(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                assert!(state.viewing_call());
                state.select(Selection::Chat("c1".into()), cx);
                assert!(!state.viewing_call());
                state.show_call(true, cx);
                assert!(state.viewing_call());
            })
        });
    }

    #[gpui_kit::test]
    fn an_ended_call_closes_the_view_and_raises_a_notice(cx: &mut TestAppContext) {
        let app = app_with_call(cx);
        let since = Instant::now() - Duration::from_secs(42);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(7, CallUpdate::State(CallState::Connected { since }), cx);
                assert!(state.call.is_some());
                state.apply_call_update(
                    7,
                    CallUpdate::State(CallState::Ended {
                        reason: EndReason::LocalHangup,
                    }),
                    cx,
                );
                assert!(state.call.is_none());
                let notice = state.notice.as_ref().expect("notice");
                assert_eq!(notice.text, "Call ended 00:42");
            })
        });
    }

    #[gpui_kit::test]
    fn updates_of_an_older_call_are_ignored(cx: &mut TestAppContext) {
        let app = app_with_call(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(
                    6,
                    CallUpdate::State(CallState::Ended {
                        reason: EndReason::Dropped,
                    }),
                    cx,
                );
                assert!(state.call.is_some());
                assert!(state.notice.is_none());
            })
        });
    }
}
