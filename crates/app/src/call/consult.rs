use calling::{CallCommand, CallHandle, CallSpec, CallUpdate, Callee};
use gpui_kit::*;

use super::demo::{DemoCall, start_demo_call};
use super::features::TransferCandidate;
use super::model::{ActiveCall, CallModel, ConsultCall, ConsultTransfer};
use crate::app_state::{AppEvent, AppState};

const CONSULT_REPORT_NOTE: &str = "consult call, the first call is on hold";

fn send_to(call: &ActiveCall, command: CallCommand) {
    if let Some(report) = &call.report {
        report.borrow_mut().command(&command);
    }
    let _ = call.commands.send(command);
}

impl AppState {
    pub fn start_consult(&mut self, target: TransferCandidate, cx: &mut Context<Self>) {
        self.close_transfer_picker(cx);
        if !self.can_transfer_call() {
            return;
        }
        let handle = if self.mode.demo {
            Some(start_demo_call(DemoCall::People(vec![(
                target.mri.clone(),
                target.name.clone(),
            )])))
        } else {
            self.call_launcher.clone().map(|launcher| {
                launcher.start_consult(CallSpec::People {
                    callees: vec![Callee {
                        mri: target.mri.clone(),
                        display_name: target.name.clone(),
                    }],
                    thread_id: target.chat_id.clone(),
                })
            })
        };
        let Some(handle) = handle else {
            self.raise_notice("Not connected to Teams yet".to_owned(), None, cx);
            return;
        };
        self.begin_consult(target, handle, cx);
    }

    fn begin_consult(&mut self, target: TransferCandidate, handle: CallHandle, cx: &mut Context<Self>) {
        let callee = (target.mri.clone(), target.name.clone());
        self.note_call_feature("consult", format!("start with {}", target.mri));
        self.send_call_command(CallCommand::Hold(true));
        let Some(held) = self.call.take() else {
            return;
        };
        let held_name = held.model.peer_name.clone();
        self.held_call = Some(held);
        if let Some(user_id) = target.user_id.clone() {
            self.request_avatars(vec![user_id], cx);
        }
        let model = CallModel::people(&target.name, std::slice::from_ref(&callee));
        self.begin_call(handle, model, Some(target.chat_id), cx);
        if let Some(call) = self.call.as_mut() {
            call.consult = Some(ConsultCall::new(callee, held_name));
        }
        self.note_call_feature("consult", CONSULT_REPORT_NOTE.to_owned());
        cx.notify();
    }

    pub fn transfer_consulted(&mut self, cx: &mut Context<Self>) {
        let (Some(call), Some(held)) = (self.call.as_mut(), self.held_call.as_ref()) else {
            return;
        };
        let model = &call.model;
        let Some(consult) = call
            .consult
            .as_mut()
            .filter(|consult| consult.ready_to_transfer(model))
        else {
            return;
        };
        consult.transfer = ConsultTransfer::Pending;
        let (mri, display_name) = consult.target.clone();
        send_to(
            held,
            CallCommand::Transfer {
                target: Callee { mri, display_name },
                replaces: consult.replacement.clone(),
            },
        );
        cx.notify();
    }

    pub fn resume_held_call(&mut self, _cx: &mut Context<Self>) {
        let (Some(call), Some(held)) = (self.call.as_ref(), self.held_call.as_ref()) else {
            return;
        };
        if call.consult.is_none() {
            return;
        }
        send_to(held, CallCommand::Hold(false));
        send_to(call, CallCommand::Hangup);
    }

    pub(super) fn is_held_call(&self, call_id: u64) -> bool {
        self.held_call.as_ref().is_some_and(|held| held.id == call_id)
    }

    pub(super) fn apply_held_call_update(&mut self, update: CallUpdate, cx: &mut Context<Self>) {
        let Some(held) = self.held_call.as_mut() else {
            return;
        };
        match update {
            CallUpdate::Notice(text) => {
                self.raise_notice(text, None, cx);
                return;
            }
            CallUpdate::TransferResult { succeeded } => {
                self.settle_consult_transfer(succeeded);
                cx.notify();
                return;
            }
            CallUpdate::VideoReady
            | CallUpdate::Levels { .. }
            | CallUpdate::Stats { .. }
            | CallUpdate::Sample(_)
            | CallUpdate::Health(_) => return,
            other => held.model.apply(other),
        }
        if held.model.ended_reason().is_none() {
            return;
        }
        let Some(mut ended) = self.held_call.take() else {
            return;
        };
        ended.pictures.clear(cx);
        let consult = self.call.as_mut().and_then(|call| {
            let transferred = call
                .consult
                .as_ref()
                .is_some_and(|consult| consult.transfer == ConsultTransfer::Done);
            if transferred { None } else { call.consult.take() }
        });
        if let Some(consult) = consult {
            self.raise_notice(format!("{} hung up", consult.held_name), None, cx);
        }
        self.refresh_call_history_after_call(cx);
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    fn settle_consult_transfer(&mut self, succeeded: bool) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let Some(consult) = call.consult.as_mut() else {
            return;
        };
        consult.transfer = if succeeded {
            ConsultTransfer::Done
        } else {
            ConsultTransfer::Idle
        };
        if succeeded {
            send_to(call, CallCommand::Hangup);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use calling::{
        CallCommand, CallControl, CallState, CallUpdate, Callee, EndReason, HoldState, Replacement,
        call_channel,
    };
    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use store::Store;

    use super::{ActiveCall, CallModel, ConsultCall, TransferCandidate};
    use crate::app_state::{AppState, Mode};

    const HELD_ID: u64 = 7;
    const CONSULT_ID: u64 = 8;

    fn connected(name: &str, mri: &str) -> CallModel {
        let mut model = CallModel::people(name, &[(mri.into(), name.into())]);
        model.apply(CallUpdate::State(CallState::Connected {
            since: Instant::now(),
        }));
        model
    }

    fn consulting(cx: &mut TestAppContext) -> (Entity<AppState>, CallControl, CallControl) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let (held_handle, held_control) = call_channel();
        let (consult_handle, consult_control) = call_channel();
        cx.update(|cx| {
            app.update(cx, |state, _| {
                let mut held_model = connected("Chris", "8:orgid:chris");
                held_model.apply(CallUpdate::Hold(HoldState::Local));
                state.held_call = Some(ActiveCall::new(
                    HELD_ID,
                    held_model,
                    held_handle.commands,
                    None,
                    held_handle.video,
                ));
                let mut consult = ActiveCall::new(
                    CONSULT_ID,
                    connected("Ana", "8:orgid:ana"),
                    consult_handle.commands,
                    None,
                    consult_handle.video,
                );
                consult.consult = Some(ConsultCall::new(
                    ("8:orgid:ana".into(), "Ana".into()),
                    "Chris".into(),
                ));
                state.call = Some(consult);
            })
        });
        (app, held_control, consult_control)
    }

    fn ended() -> CallUpdate {
        CallUpdate::State(CallState::Ended {
            reason: EndReason::LocalHangup,
        })
    }

    fn leg() -> Replacement {
        Replacement {
            link: "https://cc/ana/replacement".into(),
            participant_id: Some("leg-ana".into()),
        }
    }

    #[gpui_kit::test]
    fn consulting_holds_the_first_call_and_shows_the_consult_call(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let (held_handle, mut held_control) = call_channel();
        let (consult_handle, _consult_control) = call_channel();
        let target = TransferCandidate {
            mri: "8:orgid:ana".into(),
            name: "Ana".into(),
            user_id: Some("ana".into()),
            chat_id: "19:ana".into(),
        };
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.call = Some(ActiveCall::new(
                    HELD_ID,
                    connected("Chris", "8:orgid:chris"),
                    held_handle.commands,
                    None,
                    held_handle.video,
                ));
                state.begin_consult(target, consult_handle, cx);
                assert_eq!(state.held_call.as_ref().unwrap().id, HELD_ID);
                let call = state.call.as_ref().unwrap();
                assert_ne!(call.id, HELD_ID);
                assert_eq!(call.model.peer_name, "Ana");
                assert_eq!(call.consult.as_ref().unwrap().banner_text(), "Chris is on hold");
            })
        });
        assert_eq!(held_control.try_recv_command(), Some(CallCommand::Hold(true)));
    }

    #[gpui_kit::test]
    fn the_transfer_goes_to_the_held_call_with_the_consulted_persons_leg(cx: &mut TestAppContext) {
        let (app, mut held_control, mut consult_control) = consulting(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.transfer_consulted(cx);
                state.apply_call_update(CONSULT_ID, CallUpdate::ReplacementLink(leg()), cx);
                state.transfer_consulted(cx);
                state.transfer_consulted(cx);
            })
        });
        assert_eq!(
            held_control.try_recv_command(),
            Some(CallCommand::Transfer {
                target: Callee {
                    mri: "8:orgid:ana".into(),
                    display_name: "Ana".into()
                },
                replaces: Some(leg()),
            })
        );
        assert_eq!(held_control.try_recv_command(), None);
        assert_eq!(consult_control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn a_failed_transfer_can_be_tried_again_and_a_completed_one_ends_both_calls(
        cx: &mut TestAppContext,
    ) {
        let (app, mut held_control, mut consult_control) = consulting(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(CONSULT_ID, CallUpdate::ReplacementLink(leg()), cx);
                state.transfer_consulted(cx);
                state.apply_call_update(HELD_ID, CallUpdate::TransferResult { succeeded: false }, cx);
                state.transfer_consulted(cx);
                state.apply_call_update(HELD_ID, CallUpdate::TransferResult { succeeded: true }, cx);
                state.apply_call_update(HELD_ID, ended(), cx);
                assert!(state.held_call.is_none());
                assert!(state.call.as_ref().unwrap().consult.is_some());
                state.apply_call_update(CONSULT_ID, ended(), cx);
                assert!(state.call.is_none());
                assert_eq!(state.notice.as_ref().unwrap().text, "Call transferred to Ana");
            })
        });
        assert!(matches!(held_control.try_recv_command(), Some(CallCommand::Transfer { .. })));
        assert!(matches!(held_control.try_recv_command(), Some(CallCommand::Transfer { .. })));
        assert_eq!(consult_control.try_recv_command(), Some(CallCommand::Hangup));
    }

    #[gpui_kit::test]
    fn the_consult_call_keeps_going_when_the_held_caller_hangs_up(cx: &mut TestAppContext) {
        let (app, _held_control, mut consult_control) = consulting(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(HELD_ID, ended(), cx);
                assert!(state.held_call.is_none());
                let call = state.call.as_ref().expect("consult call stays");
                assert_eq!(call.id, CONSULT_ID);
                assert!(call.consult.is_none());
                assert!(call.model.is_active());
                assert_eq!(state.notice.as_ref().unwrap().text, "Chris hung up");
            })
        });
        assert_eq!(consult_control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn the_held_call_comes_back_when_the_consult_ends_and_resume_ends_the_consult(
        cx: &mut TestAppContext,
    ) {
        let (app, mut held_control, mut consult_control) = consulting(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.resume_held_call(cx);
                state.apply_call_update(CONSULT_ID, ended(), cx);
                let call = state.call.as_ref().expect("held call is back");
                assert_eq!(call.id, HELD_ID);
                assert!(state.held_call.is_none());
                assert_eq!(state.notice.as_ref().unwrap().text, "The consultation ended");
            })
        });
        assert_eq!(held_control.try_recv_command(), Some(CallCommand::Hold(false)));
        assert_eq!(consult_control.try_recv_command(), Some(CallCommand::Hangup));
    }

    #[gpui_kit::test]
    fn held_call_updates_never_reach_the_visible_consult_call(cx: &mut TestAppContext) {
        let (app, _held_control, _consult_control) = consulting(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(HELD_ID, CallUpdate::Hold(HoldState::Active), cx);
                assert_eq!(state.held_call.as_ref().unwrap().model.hold, HoldState::Active);
                assert_eq!(state.call.as_ref().unwrap().model.hold, HoldState::Active);
                assert_eq!(state.call.as_ref().unwrap().model.peer_name, "Ana");
                assert!(!state.can_transfer_call());
            })
        });
    }
}
