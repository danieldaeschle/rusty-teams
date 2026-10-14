use calling::{
    CallCommand, CallSpec, Callee, EndKind, EndReason, HoldState,
};
use gpui_kit::*;

use super::background::BackgroundPick;
use super::demo::DemoCall;
use super::health::HealthBanner;
use super::model::{ActiveCall, BreakoutState, CallKind, CallModel, PendingMove};
use super::recording_menu::RecordAction;
use super::target::plan_for_chat;
use crate::app_state::{AppEvent, AppState};
use crate::data::is_one_on_one;
use crate::views::call_transcript::TranscriptPanel;

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
    PendingMove {
        target: room.main.clone().expect("a room with a way back"),
        title: room.main_title.clone(),
        breakout: None,
    }
}

impl AppState {
    pub fn run_call_record_action(&mut self, action: RecordAction, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| call.model.can_record()) else {
            return;
        };
        let command = match action {
            RecordAction::Start(kind) => CallCommand::StartRecording {
                kind,
                title: call.model.title.clone(),
            },
            RecordAction::Stop(scope) => CallCommand::StopRecording { scope },
        };
        self.send_call_command(command);
    }

    pub fn open_call_transcript(
        &mut self,
        panel: Entity<TranscriptPanel>,
        cx: &mut Context<Self>,
    ) {
        self.transcript_panel = Some(panel);
        self.show_call_transcript(cx);
    }

    pub(super) fn show_call_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.model.kind == CallKind::Meeting)
        else {
            return;
        };
        call.chat_open = false;
        call.people_open = false;
        call.transcript_open = true;
        let captions_off = !call.model.captions_on();
        if captions_off {
            self.send_call_command(CallCommand::SetCaptions(true));
        }
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn close_call_transcript(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.transcript_open = false;
        }
        self.transcript_panel = None;
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn choose_call_caption_language(&mut self, tag: &str, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.model.is_active() && call.model.kind == CallKind::Meeting)
        else {
            return;
        };
        call.model.choose_caption_language(tag);
        let captions_off = !call.model.captions_on();
        if captions_off {
            self.send_call_command(CallCommand::SetCaptions(true));
        }
        self.send_ready_caption_language();
        cx.notify();
    }

    fn send_ready_caption_language(&mut self) {
        let language = self
            .call
            .as_mut()
            .and_then(|call| call.model.take_ready_caption_language());
        if let Some(language) = language {
            self.send_call_command(CallCommand::SetCaptionLanguage(language));
        }
    }

    pub fn accept_recording_notice(&mut self, _cx: &mut Context<Self>) {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.model.consent_required)
        {
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

    pub fn set_call_incoming_video(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.send_call_command(CallCommand::SetIncomingVideo(enabled));
        cx.notify();
    }

    pub fn close_call_health_banner(&mut self, banner: HealthBanner, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.model.health.close_banner(banner);
            cx.notify();
        }
    }

    pub fn turn_off_call_effects(&mut self, cx: &mut Context<Self>) {
        self.set_call_background(BackgroundPick::None, cx);
        let effects = calling::CameraEffects {
            auto_framing: false,
            adjust_brightness: false,
            soft_focus: false,
            ..self.call_effects
        };
        self.set_call_effects(effects, cx);
    }

    pub fn open_call_whiteboard(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_ref()
            .filter(|call| call.model.can_open_whiteboard())
        else {
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
        let peer = self
            .call
            .as_ref()
            .and_then(|call| call.model.tiles.first().map(|tile| tile.mri.clone()));
        self.sidebar
            .chats
            .iter()
            .filter(|chat| is_one_on_one(chat))
            .filter_map(|chat| {
                let plan = plan_for_chat(chat, self.directory.me.as_ref())?;
                let (mri, name) = plan.callees.into_iter().next()?;
                Some(TransferCandidate {
                    user_id: mri.strip_prefix("8:orgid:").map(str::to_owned),
                    mri,
                    name,
                    chat_id: chat.id.clone(),
                })
            })
            .filter(|candidate| Some(&candidate.mri) != peer.as_ref())
            .collect()
    }

    pub fn can_transfer_call(&self) -> bool {
        self.call
            .as_ref()
            .is_some_and(|call| call.model.can_transfer() && call.consult.is_none())
            && self.held_call.is_none()
    }

    pub fn open_transfer_picker(
        &mut self,
        picker: Entity<crate::views::transfer_picker::TransferPicker>,
        cx: &mut Context<Self>,
    ) {
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
        let callee = Callee {
            mri: target.mri,
            display_name: target.name,
        };
        self.send_call_command(CallCommand::Transfer {
            target: callee,
            replaces: None,
        });
    }

    pub(super) fn begin_breakout_move(
        &mut self,
        moved: calling::BreakoutMove,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.pending_move.is_none())
        else {
            return;
        };
        let (main, main_title) = match &call.model.breakout {
            Some(room) => (
                room.main
                    .clone()
                    .or_else(|| call.model.main_meeting.clone()),
                room.main_title.clone(),
            ),
            None => (call.meeting_target.clone(), call.model.title.clone()),
        };
        let pending = if moved.returning {
            PendingMove {
                target: moved.target,
                title: main_title,
                breakout: None,
            }
        } else {
            let breakout = BreakoutState {
                room_name: moved.room_name.clone(),
                main,
                main_title,
            };
            PendingMove {
                target: moved.target,
                title: moved.room_name.clone(),
                breakout: Some(breakout),
            }
        };
        let notice = if moved.returning {
            "Moving you back to the main meeting".to_owned()
        } else {
            format!("Moving you to {}", moved.room_name)
        };
        call.pending_move = Some(pending);
        self.raise_notice(notice, None, cx);
        self.send_call_command(CallCommand::Hangup);
    }

    pub fn return_to_main_meeting(&mut self, _cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.pending_move.is_none())
        else {
            return;
        };
        let Some(room) = call
            .model
            .breakout
            .as_ref()
            .filter(|room| room.main.is_some())
        else {
            return;
        };
        call.pending_move = Some(return_move(room));
        self.send_call_command(CallCommand::Hangup);
    }

    pub(super) fn join_moved_call(&mut self, moved: PendingMove, cx: &mut Context<Self>) {
        let demo = DemoCall::Meeting {
            guests: super::demo::demo_guests(DEMO_ROOM_GUESTS),
            lobby: false,
            organizer: false,
        };
        let Some(handle) =
            self.place_call(CallSpec::Meeting(moved.target.clone()), demo, false, cx)
        else {
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
