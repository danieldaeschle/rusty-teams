use calling::{CallCommand, CallHandle, CallSpec, CallState, Callee, EndKind, EndReason, HoldState};
use gpui_kit::*;

use super::demo::{DemoCall, start_demo_call};
use super::model::{ActiveCall, BreakoutState, CallModel, ConsultCall, PendingMove};
use super::target::plan_for_chat;
use crate::app_state::AppState;
use crate::data::is_one_on_one;

const DEMO_ROOM_GUESTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferCandidate {
    pub mri: String,
    pub name: String,
    pub user_id: Option<String>,
    pub chat_id: String,
}

pub fn room_closed_move(call: &ActiveCall, reason: &EndReason) -> Option<PendingMove> {
    let room = call.model.breakout.as_ref()?;
    if *reason != EndReason::Remote(EndKind::RoomClosed) {
        return None;
    }
    Some(return_move(room))
}

fn return_move(room: &BreakoutState) -> PendingMove {
    PendingMove { target: room.main.clone().expect("a room with a way back"), title: room.main_title.clone(), breakout: None }
}

impl AppState {
    pub fn set_call_recording(&mut self, on: bool, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| call.model.can_record()) else {
            return;
        };
        let title = call.model.title.clone();
        self.send_call_command(CallCommand::SetRecording { on, title });
    }

    pub fn accept_recording_notice(&mut self, _cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.model.consent_required) {
            self.send_call_command(CallCommand::ConsentToRecording);
        }
    }

    pub fn toggle_call_hold(&mut self, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        let hold = match call.model.hold {
            HoldState::Local => false,
            HoldState::Active if call.model.can_hold() => true,
            _ => return,
        };
        self.send_call_command(CallCommand::Hold(hold));
    }

    pub fn open_call_whiteboard(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| call.model.can_open_whiteboard()) else {
            return;
        };
        if let Some(url) = call.model.whiteboard_url() {
            cx.open_url(url);
            return;
        }
        let title = call.model.title.clone();
        self.send_call_command(CallCommand::OpenWhiteboard { title });
    }

    pub fn transfer_candidates(&self) -> Vec<TransferCandidate> {
        let peer = self.call.as_ref().and_then(|call| call.model.tiles.first().map(|tile| tile.mri.clone()));
        self.sidebar
            .chats
            .iter()
            .filter(|chat| is_one_on_one(chat))
            .filter_map(|chat| {
                let plan = plan_for_chat(chat, self.directory.me.as_ref())?;
                let (mri, name) = plan.callees.into_iter().next()?;
                Some(TransferCandidate { user_id: mri.strip_prefix("8:orgid:").map(str::to_owned), mri, name, chat_id: chat.id.clone() })
            })
            .filter(|candidate| Some(&candidate.mri) != peer.as_ref())
            .collect()
    }

    pub fn can_transfer_call(&self) -> bool {
        self.call.as_ref().is_some_and(|call| call.model.can_transfer() && call.consult.is_none())
    }

    pub fn open_transfer_picker(&mut self, picker: Entity<crate::views::transfer_picker::TransferPicker>, cx: &mut Context<Self>) {
        if self.can_transfer_call() {
            self.transfer_picker = Some(picker);
            cx.notify();
        }
    }

    pub fn close_transfer_picker(&mut self, cx: &mut Context<Self>) {
        if self.transfer_picker.take().is_some() {
            cx.notify();
        }
    }

    pub fn transfer_call_blind(&mut self, target: TransferCandidate, cx: &mut Context<Self>) {
        self.close_transfer_picker(cx);
        if !self.can_transfer_call() {
            return;
        }
        let callee = Callee { mri: target.mri, display_name: target.name };
        self.send_call_command(CallCommand::Transfer { target: callee, replaces: None });
    }

    pub fn start_consult(&mut self, target: TransferCandidate, cx: &mut Context<Self>) {
        self.close_transfer_picker(cx);
        if !self.can_transfer_call() {
            return;
        }
        let spec = CallSpec::People {
            callees: vec![Callee { mri: target.mri.clone(), display_name: target.name.clone() }],
            thread_id: target.chat_id.clone(),
        };
        let handle = if self.mode.demo {
            Some(start_demo_call(DemoCall::People(vec![(target.mri.clone(), target.name.clone())])))
        } else {
            self.call_launcher.clone().map(|launcher| launcher.start_consult(spec))
        };
        let Some(CallHandle { commands, updates, .. }) = handle else {
            self.raise_notice("Not connected to Teams yet".to_owned(), None, cx);
            return;
        };
        self.send_call_command(CallCommand::Hold(true));
        self.call_count += 1;
        let consult_id = self.call_count;
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let call_id = call.id;
        call.consult = Some(ConsultCall {
            id: consult_id,
            target: (target.mri, target.name),
            commands,
            state: CallState::Connecting,
            replacement: None,
            transferring: false,
        });
        self.follow_consult(call_id, consult_id, updates, cx);
        cx.notify();
    }

    fn follow_consult(&mut self, call_id: u64, consult_id: u64, mut updates: tokio::sync::mpsc::UnboundedReceiver<calling::CallUpdate>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.recv().await {
                if this.update(cx, |state, cx| state.apply_consult_update(call_id, consult_id, update, cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    pub(super) fn apply_consult_update(&mut self, call_id: u64, consult_id: u64, update: calling::CallUpdate, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.id == call_id) else {
            return;
        };
        let Some(consult) = call.consult.as_mut().filter(|consult| consult.id == consult_id) else {
            return;
        };
        match update {
            calling::CallUpdate::ReplacementLink(link) => consult.replacement = Some(link),
            calling::CallUpdate::Notice(text) => self.raise_notice(text, None, cx),
            calling::CallUpdate::State(state) => {
                let ended = matches!(state, CallState::Ended { .. });
                consult.state = state;
                if ended {
                    let transferring = consult.transferring;
                    call.consult = None;
                    if !transferring {
                        self.raise_notice("The consultation ended".to_owned(), None, cx);
                        self.send_call_command(CallCommand::Hold(false));
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub fn transfer_consulted(&mut self, _cx: &mut Context<Self>) {
        let Some(consult) = self.call.as_mut().and_then(|call| call.consult.as_mut()).filter(|consult| consult.ready_to_transfer()) else {
            return;
        };
        consult.transferring = true;
        let (mri, name) = consult.target.clone();
        let replaces = consult.replacement.clone();
        self.send_call_command(CallCommand::Transfer { target: Callee { mri, display_name: name }, replaces });
    }

    pub fn cancel_consult(&mut self, _cx: &mut Context<Self>) {
        if let Some(consult) = self.call.as_ref().and_then(|call| call.consult.as_ref()) {
            let _ = consult.commands.send(CallCommand::Hangup);
        }
    }

    pub(super) fn begin_breakout_move(&mut self, moved: calling::BreakoutMove, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.pending_move.is_none()) else {
            return;
        };
        let (main, main_title) = match &call.model.breakout {
            Some(room) => (room.main.clone().or_else(|| call.model.main_meeting.clone()), room.main_title.clone()),
            None => (call.meeting_target.clone(), call.model.title.clone()),
        };
        let pending = if moved.returning {
            PendingMove { target: moved.target, title: main_title, breakout: None }
        } else {
            let breakout = BreakoutState { room_name: moved.room_name.clone(), main, main_title };
            PendingMove { target: moved.target, title: moved.room_name.clone(), breakout: Some(breakout) }
        };
        let notice = if moved.returning { "Moving you back to the main meeting".to_owned() } else { format!("Moving you to {}", moved.room_name) };
        call.pending_move = Some(pending);
        self.raise_notice(notice, None, cx);
        self.send_call_command(CallCommand::Hangup);
    }

    pub fn return_to_main_meeting(&mut self, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.pending_move.is_none()) else {
            return;
        };
        let Some(room) = call.model.breakout.as_ref().filter(|room| room.main.is_some()) else {
            return;
        };
        call.pending_move = Some(return_move(room));
        self.send_call_command(CallCommand::Hangup);
    }

    pub(super) fn join_moved_call(&mut self, moved: PendingMove, cx: &mut Context<Self>) {
        let demo = DemoCall::Meeting { guests: super::demo::demo_guests(DEMO_ROOM_GUESTS), lobby: false, organizer: false };
        let Some(handle) = self.place_call(CallSpec::Meeting(moved.target.clone()), demo, cx) else {
            return;
        };
        let mut model = CallModel::meeting(&moved.title, false);
        model.breakout = moved.breakout;
        self.begin_call(handle, model, Some(moved.target.thread_id.clone()), cx);
        if let Some(call) = self.call.as_mut() {
            call.meeting_target = Some(moved.target);
        }
    }
}
