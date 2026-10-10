use std::time::{Duration, Instant};

use calling::meeting::LiveMeeting;
use calling::{
    AcceptMode, CallCommand, CallHandle, CallSpec, CallUpdate, Callee, DeviceChoice, EngineEvent, MuteCommand, Reaction, RingSignal, ShareSource, VideoKey,
};
use gpui_kit::*;

use super::background::{BACKGROUND_META_KEY, BackgroundPick, SHOWN_IMAGES, is_image_file, save_custom_backgrounds};
use super::demo::{DemoCall, DemoScene, demo_caller, demo_guests, demo_ring, demo_video_ring, start_demo_call, start_demo_scene_call};
use super::features::room_closed_move;
use super::model::{ActiveCall, CallKind, CallModel, ended_notice};
use super::ring::{MissedCall, RingOutcome};
use super::target::{is_organizer, plan_for_chat};
use crate::app_state::{AppEvent, AppState, Selection, chat_title};
use crate::notify::selection_for;

const TIMER_REFRESH: Duration = Duration::from_millis(500);
const SHARE_SOURCES_EVERY_TICKS: u32 = 10;
pub(super) const NOT_CONNECTED_NOTICE: &str = "Not connected to Teams yet";
const READ_ONLY_NOTICE: &str = "Calls are off in read-only mode";
const NOT_CALLABLE_NOTICE: &str = "This chat cannot be called";
pub(super) const MEETING_UNAVAILABLE_NOTICE: &str = "This meeting cannot be joined";
const ANSWER_FAILED_NOTICE: &str = "Could not answer the call";
const MEETING_KIND: &str = "meeting";
const DEMO_LOBBY_MEETING: &str = "demo-chat-meeting-retro";
const DEMO_CROWD: usize = 11;
const DEMO_SMALL_MEETING: usize = 4;
const DEMO_LIVE_MINUTES: i64 = 60;
const ORGID_PREFIX: &str = "8:orgid:";
pub const SHARE_SOUND_META_KEY: &str = "call_share_sound";

pub fn load_share_sound(store: &store::Store) -> bool {
    store.meta(SHARE_SOUND_META_KEY).ok().flatten().as_deref() == Some("1")
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

impl AppState {
    pub fn start_test_call(&mut self, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        let Some(handle) = self.place_call(CallSpec::Echo, DemoCall::Test, cx) else {
            return;
        };
        self.begin_call(handle, CallModel::test(), None, cx);
    }

    pub fn start_chat_call(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        let plan = self
            .sidebar
            .chats
            .iter()
            .find(|chat| chat.id == conversation_id)
            .and_then(|chat| plan_for_chat(chat, self.directory.me.as_ref()));
        let Some(plan) = plan else {
            self.raise_notice(NOT_CALLABLE_NOTICE.to_owned(), None, cx);
            return;
        };
        let spec = CallSpec::People {
            callees: plan
                .callees
                .iter()
                .map(|(mri, name)| Callee {
                    mri: mri.clone(),
                    display_name: name.clone(),
                })
                .collect(),
            thread_id: conversation_id.to_owned(),
        };
        let Some(handle) = self.place_call(spec, DemoCall::People(plan.callees.clone()), cx) else {
            return;
        };
        let user_ids = plan
            .callees
            .iter()
            .filter_map(|(mri, _)| mri.strip_prefix(ORGID_PREFIX).map(str::to_owned))
            .collect();
        self.request_avatars(user_ids, cx);
        let model = CallModel::people(&plan.title, &plan.callees);
        self.begin_call(handle, model, Some(conversation_id.to_owned()), cx);
    }

    pub fn join_meeting_call(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        let Some(meeting) = self.running_meeting(conversation_id).cloned() else {
            self.raise_notice(MEETING_UNAVAILABLE_NOTICE.to_owned(), None, cx);
            return;
        };
        let Some(target) = meeting.target() else {
            self.raise_notice(MEETING_UNAVAILABLE_NOTICE.to_owned(), None, cx);
            return;
        };
        let guests = if conversation_id == DEMO_LOBBY_MEETING { DEMO_SMALL_MEETING } else { DEMO_CROWD };
        let demo = DemoCall::Meeting {
            guests: demo_guests(guests),
            lobby: conversation_id == DEMO_LOBBY_MEETING,
            organizer: conversation_id != DEMO_LOBBY_MEETING,
        };
        let Some(handle) = self.place_call(CallSpec::Meeting(target.clone()), demo, cx) else {
            return;
        };
        let title = selection_for(&self.sidebar, conversation_id)
            .map(|selection| crate::app_state::selection_title(&self.sidebar, &selection))
            .unwrap_or_else(|| "Meeting".to_owned());
        let model = CallModel::meeting(&title, is_organizer(&meeting, self.directory.me.as_ref()));
        self.begin_call(handle, model, Some(conversation_id.to_owned()), cx);
        if let Some(call) = self.call.as_mut() {
            call.meeting_target = Some(target);
        }
    }

    pub(super) fn place_call(&mut self, spec: CallSpec, demo: DemoCall, cx: &mut Context<Self>) -> Option<CallHandle> {
        if self.mode.demo {
            return Some(start_demo_call(demo));
        }
        if self.mode.read_only {
            self.raise_notice(READ_ONLY_NOTICE.to_owned(), None, cx);
            return None;
        }
        match self.call_launcher.clone() {
            Some(launcher) => Some(launcher.start_call(spec)),
            None => {
                self.raise_notice(NOT_CONNECTED_NOTICE.to_owned(), None, cx);
                None
            }
        }
    }

    pub(super) fn begin_call(
        &mut self,
        handle: CallHandle,
        model: CallModel,
        conversation_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.call_count += 1;
        let call_id = self.call_count;
        let CallHandle { commands, updates, video } = handle;
        self.call = Some(ActiveCall::new(call_id, model, commands, conversation_id, video));
        if self.call_share_sound {
            self.send_call_command(CallCommand::SetShareSound(true));
        }
        if let Some(call) = self.call.as_mut() {
            call.model.background = self.call_background.clone();
        }
        if self.call_background != BackgroundPick::None {
            self.send_background_command(cx);
        }
        self.refresh_call_backgrounds(cx);
        self.new_chat = false;
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
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(TIMER_REFRESH).await;
                ticks += 1;
                let running = this.update(cx, |state, cx| {
                    let running = state.call.as_ref().is_some_and(|call| call.id == call_id);
                    if running {
                        if ticks.is_multiple_of(SHARE_SOURCES_EVERY_TICKS) {
                            state.refresh_share_sources();
                        }
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

    pub(super) fn apply_call_update(&mut self, call_id: u64, update: CallUpdate, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.id == call_id) else {
            return;
        };
        match update {
            CallUpdate::VideoReady => {
                call.pictures.apply(&call.video, cx);
                cx.notify();
                return;
            }
            CallUpdate::Notice(text) => {
                self.raise_notice(text, None, cx);
                return;
            }
            CallUpdate::WhiteboardUrl(url) => {
                cx.open_url(&url);
                return;
            }
            CallUpdate::BreakoutMove(moved) => {
                self.begin_breakout_move(moved, cx);
                return;
            }
            _ => {}
        }
        call.model.apply(update);
        if call.model.screen_sharer.is_none() {
            call.stage_fullscreen = false;
            call.pictures.forget(&VideoKey::Screen, cx);
        }
        if !call.model.camera_on {
            call.pictures.forget(&VideoKey::LocalCamera, cx);
        }
        let Some(reason) = call.model.ended_reason().cloned() else {
            cx.emit(AppEvent::Call);
            cx.notify();
            return;
        };
        let elapsed = call.model.elapsed(Instant::now());
        let notice = ended_notice(&reason, elapsed, &call.model.peer_name);
        let next_move = call.pending_move.take().or_else(|| room_closed_move(call, &reason));
        let consult = call.consult.take();
        call.pictures.clear(cx);
        self.call = None;
        if let Some(consult) = consult {
            let _ = consult.commands.send(CallCommand::Hangup);
        }
        match next_move {
            Some(next_move) => self.join_moved_call(next_move, cx),
            None => self.raise_notice(notice, None, cx),
        }
        self.refresh_call_history_after_call(cx);
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub(super) fn send_call_command(&self, command: CallCommand) {
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

    pub fn toggle_call_camera(&mut self, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| call.model.can_use_camera()) else {
            return;
        };
        let on = !call.model.camera_on;
        self.send_call_command(CallCommand::SetCamera(on));
    }

    pub fn select_call_camera(&mut self, choice: DeviceChoice, _cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.model.camera = choice.clone();
        }
        self.send_call_command(CallCommand::SelectCamera(choice));
    }

    pub fn start_call_share(&mut self, source: ShareSource, _cx: &mut Context<Self>) {
        self.send_call_command(CallCommand::StartShare(source));
    }

    pub fn toggle_call_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        if call.model.local_share.is_some() {
            self.stop_call_share(cx);
            return;
        }
        if !call.model.can_share() {
            return;
        }
        if let Some(source) = call.model.screens().first().map(|source| (*source).clone()) {
            self.start_call_share(source, cx);
        }
    }

    pub fn stop_call_share(&mut self, _cx: &mut Context<Self>) {
        self.send_call_command(CallCommand::StopShare);
    }

    pub fn refresh_share_sources(&self) {
        let idle = self.call.as_ref().is_some_and(|call| call.model.local_share.is_none());
        if idle {
            self.send_call_command(CallCommand::RefreshShareSources);
        }
    }

    pub fn leave_call(&mut self, _cx: &mut Context<Self>) {
        self.send_call_command(CallCommand::Hangup);
    }

    pub fn end_meeting_for_all(&mut self, _cx: &mut Context<Self>) {
        let allowed = self.call.as_ref().is_some_and(|call| call.model.can_end_meeting);
        if allowed {
            self.send_call_command(CallCommand::EndMeeting);
        }
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

    pub fn toggle_call_chat(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.chat_thread().is_some()) else {
            return;
        };
        call.chat_open = !call.chat_open;
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn call_chat_thread(&self) -> Option<String> {
        let call = self.call.as_ref().filter(|call| call.chat_open)?;
        call.chat_thread().map(str::to_owned)
    }

    pub fn call_chat_unread(&self) -> u32 {
        let Some(thread) = self.call.as_ref().filter(|call| !call.chat_open).and_then(|call| call.chat_thread()) else {
            return 0;
        };
        let counted = self.directory.unread_counts.get(thread).copied();
        counted.unwrap_or_else(|| u32::from(self.sidebar.chats.iter().any(|chat| chat.id == thread && chat.unread)))
    }

    pub fn toggle_call_hand(&mut self, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| call.model.is_active()) else {
            return;
        };
        let raised = call.model.own_hand.is_none();
        self.send_call_command(CallCommand::SetHand(raised));
    }

    pub fn lower_call_hand(&mut self, mri: String, _cx: &mut Context<Self>) {
        let allowed = self.call.as_ref().is_some_and(|call| call.model.can_lower_hands());
        if allowed {
            self.send_call_command(CallCommand::LowerHand { mri });
        }
    }

    pub fn lower_all_call_hands(&mut self, _cx: &mut Context<Self>) {
        let allowed = self.call.as_ref().is_some_and(|call| call.model.can_lower_hands() && call.model.any_hand_raised());
        if allowed {
            self.send_call_command(CallCommand::LowerAllHands);
        }
    }

    pub fn send_call_reaction(&mut self, reaction: Reaction, _cx: &mut Context<Self>) {
        let live = self.call.as_ref().is_some_and(|call| call.model.is_active());
        if live {
            self.send_call_command(CallCommand::SendReaction(reaction));
        }
    }

    pub fn set_call_share_sound(&mut self, on: bool, cx: &mut Context<Self>) {
        self.call_share_sound = on;
        let _ = self.store.set_meta(SHARE_SOUND_META_KEY, if on { "1" } else { "0" });
        self.send_call_command(CallCommand::SetShareSound(on));
        cx.notify();
    }

    pub fn admit_call_guest(&mut self, mri: String, _cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.model.can_admit()) {
            self.send_call_command(CallCommand::Admit { mri });
        }
    }

    pub fn admit_all_call_guests(&mut self, _cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.model.can_admit() && !call.model.lobby_guests().is_empty()) {
            self.send_call_command(CallCommand::AdmitAll);
        }
    }

    pub fn deny_call_guest(&mut self, mri: String, _cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.model.can_admit()) {
            self.send_call_command(CallCommand::Deny { mri });
        }
    }

    pub fn mute_call_participant(&mut self, mri: String, _cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.model.can_manage()) {
            self.send_call_command(CallCommand::MuteParticipant { mri });
        }
    }

    pub fn mute_all_call(&mut self, _cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.model.can_manage()) {
            self.send_call_command(CallCommand::MuteAll);
        }
    }

    pub fn remove_call_participant(&mut self, mri: String, _cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.model.can_manage()) {
            self.send_call_command(CallCommand::RemoveParticipant { mri });
        }
    }

    pub fn toggle_call_spotlight(&mut self, mri: String, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| call.model.can_manage() && call.model.is_active()) else {
            return;
        };
        let command = if call.model.is_spotlighted(&mri) { CallCommand::StopSpotlight { mri } } else { CallCommand::Spotlight { mri } };
        self.send_call_command(command);
    }

    pub fn toggle_call_pin(&mut self, mri: &str, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.model.toggle_pin(mri);
            cx.notify();
        }
    }

    pub fn toggle_call_captions(&mut self, _cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| call.model.is_active() && call.model.kind == CallKind::Meeting) else {
            return;
        };
        let on = !call.model.captions_on();
        self.send_call_command(CallCommand::SetCaptions(on));
    }

    pub fn set_call_background(&mut self, pick: BackgroundPick, cx: &mut Context<Self>) {
        let _ = self.store.set_meta(BACKGROUND_META_KEY, &pick.to_meta());
        self.call_background = pick.clone();
        if let Some(call) = self.call.as_mut() {
            call.model.background = pick;
        }
        self.send_background_command(cx);
        cx.notify();
    }

    pub(super) fn send_background_command(&mut self, cx: &mut Context<Self>) {
        let pick = self.call_background.clone();
        if let Some(choice) = self.call_backgrounds.choice_for(&pick) {
            self.send_call_command(CallCommand::SetBackground(choice));
            return;
        }
        let BackgroundPick::Default(id) = pick else { return };
        let Some(image) = self.call_backgrounds.find(&id).cloned() else { return };
        let Some(launcher) = self.call_launcher.clone() else { return };
        let cache = self.call_backgrounds.cache.clone();
        let receiver = crate::runtime::spawn(async move { launcher.download_background(&cache, &image).await });
        cx.spawn(async move |this, cx| {
            let path = receiver.await.ok().flatten();
            this.update(cx, |state, cx| match path {
                Some(path) if state.call_background == BackgroundPick::Default(id) => {
                    state.send_call_command(CallCommand::SetBackground(calling::BackgroundChoice::Image(path)));
                }
                Some(_) => {}
                None => state.raise_notice("That background could not be downloaded".to_owned(), None, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn refresh_call_backgrounds(&mut self, cx: &mut Context<Self>) {
        if self.mode.demo || !self.call_backgrounds.needs_refresh() {
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else { return };
        self.call_backgrounds.refreshing = true;
        let cache = self.call_backgrounds.cache.clone();
        let wanted = SHOWN_IMAGES;
        let receiver = crate::runtime::spawn(async move { launcher.refresh_backgrounds(&cache, wanted).await });
        cx.spawn(async move |this, cx| {
            let catalog = receiver.await.ok().flatten();
            this.update(cx, |state, cx| {
                state.call_backgrounds.refreshing = false;
                if let Some(catalog) = catalog {
                    state.call_backgrounds.images = catalog;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn add_call_background(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Some(path) = receiver.await.ok().and_then(Result::ok).flatten().and_then(|paths| paths.into_iter().next()) else {
                return;
            };
            this.update(cx, |state, cx| {
                if !is_image_file(&path) {
                    state.raise_notice("Pick a JPG or PNG picture".to_owned(), None, cx);
                    return;
                }
                state.call_backgrounds.add_custom(path.clone());
                save_custom_backgrounds(&state.store, &state.call_backgrounds.customs);
                state.set_call_background(BackgroundPick::Custom(path), cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn toggle_stage_fullscreen(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.model.screen_sharer.is_some()) else {
            return;
        };
        call.stage_fullscreen = !call.stage_fullscreen;
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn leave_stage_fullscreen(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.stage_fullscreen) else {
            return;
        };
        call.stage_fullscreen = false;
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn stage_fullscreen(&self) -> bool {
        self.call.as_ref().is_some_and(|call| call.stage_fullscreen && call.model.screen_sharer.is_some())
    }

    pub fn viewing_call(&self) -> bool {
        self.call.as_ref().is_some_and(|call| call.viewing)
    }
}

impl AppState {
    pub fn on_engine_event(&mut self, event: EngineEvent, cx: &mut Context<Self>) {
        match event {
            EngineEvent::Incoming(ring) => self.push_ring(ring, false, cx),
            EngineEvent::RingEnded { ring_id, kind, .. } => {
                self.signal_ring(ring_id, RingSignal::Remote(kind), cx);
            }
        }
    }

    fn push_ring(&mut self, ring: calling::IncomingRing, demo: bool, cx: &mut Context<Self>) {
        let caller_user_id = ring.caller.mri.strip_prefix(ORGID_PREFIX).map(str::to_owned);
        self.rings.push(ring, demo, Instant::now());
        if let Some(user_id) = caller_user_id {
            self.request_avatars(vec![user_id], cx);
        }
        cx.emit(AppEvent::Ring);
        cx.notify();
    }

    pub fn demo_incoming_ring(&mut self, cx: &mut Context<Self>) {
        self.push_ring(demo_ring(), true, cx);
    }

    pub fn demo_incoming_video_ring(&mut self, cx: &mut Context<Self>) {
        self.push_ring(demo_video_ring(), true, cx);
    }

    pub fn start_demo_scene(&mut self, scene: DemoScene, cx: &mut Context<Self>) {
        if self.call.is_some() {
            return;
        }
        let (script, model, conversation_id) = if scene.is_one_to_one() {
            (DemoCall::People(vec![demo_caller()]), CallModel::people("Mara Lindqvist", &[demo_caller()]), "demo-chat-mara")
        } else {
            let script = DemoCall::Meeting { guests: demo_guests(DEMO_SMALL_MEETING), lobby: false, organizer: true };
            (script, CallModel::meeting("Standup", true), "demo-chat-meeting-standup")
        };
        let handle = start_demo_scene_call(script, scene);
        self.begin_call(handle, model, Some(conversation_id.to_owned()), cx);
        if let Some(call) = self.call.as_mut().filter(|_| !scene.is_one_to_one()) {
            call.meeting_target = Some(calling::MeetingTarget {
                thread_id: conversation_id.to_owned(),
                tenant_id: "demo-tenant".to_owned(),
                organizer_id: "demo-me".to_owned(),
                meeting_data: None,
            });
        }
    }

    fn signal_ring(&mut self, ring_id: u64, signal: RingSignal, cx: &mut Context<Self>) {
        if let Some(outcome) = self.rings.signal(ring_id, signal, Instant::now()) {
            self.finish_ring(ring_id, outcome, cx);
        }
    }

    fn finish_ring(&mut self, ring_id: u64, outcome: RingOutcome, cx: &mut Context<Self>) {
        if let RingOutcome::Missed(missed) = outcome {
            if let Some(launcher) = &self.call_launcher {
                launcher.drop_ring(ring_id);
            }
            cx.emit(AppEvent::MissedCall(missed));
        }
        cx.emit(AppEvent::Ring);
        cx.notify();
    }

    pub fn tick_rings(&mut self, cx: &mut Context<Self>) {
        if self.rings.is_empty() {
            return;
        }
        for (ring_id, outcome) in self.rings.tick(Instant::now()) {
            self.finish_ring(ring_id, outcome, cx);
        }
    }

    pub fn accept_ringing_call(&mut self, cx: &mut Context<Self>) {
        if let Some(ring_id) = self.rings.first_ringing_id() {
            self.accept_ring(ring_id, AcceptMode::Audio, cx);
        }
    }

    pub fn decline_ringing_call(&mut self, cx: &mut Context<Self>) {
        if let Some(ring_id) = self.rings.first_ringing_id() {
            self.decline_ring(ring_id, cx);
        }
    }

    pub fn accept_ring(&mut self, ring_id: u64, mode: AcceptMode, cx: &mut Context<Self>) {
        let Some(entry) = self.rings.get(ring_id).cloned() else {
            return;
        };
        if self.call.is_some() {
            self.leave_call(cx);
        }
        self.signal_ring(ring_id, RingSignal::Accept, cx);
        let conversation_id = entry.ring.thread_id.clone();
        let title = conversation_id
            .as_deref()
            .and_then(|id| self.sidebar.chats.iter().find(|chat| chat.id == id))
            .filter(|_| entry.ring.is_group)
            .map(chat_title)
            .unwrap_or_else(|| entry.caller_name().to_owned());
        let model = CallModel::incoming(&title, &entry.ring.caller.mri, entry.caller_name());
        if entry.demo {
            let handle = start_demo_call(DemoCall::Incoming(demo_caller()));
            self.begin_call(handle, model, conversation_id, cx);
            if mode == AcceptMode::Video {
                self.send_call_command(CallCommand::SetCamera(true));
            }
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            self.raise_notice(ANSWER_FAILED_NOTICE.to_owned(), None, cx);
            return;
        };
        let receiver = crate::runtime::spawn(async move { launcher.accept_ring(ring_id, mode).await });
        cx.spawn(async move |this, cx| {
            let handle = receiver.await.ok().flatten();
            this.update(cx, |state, cx| match handle {
                Some(handle) => state.begin_call(handle, model, conversation_id, cx),
                None => state.raise_notice(ANSWER_FAILED_NOTICE.to_owned(), None, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn decline_ring(&mut self, ring_id: u64, cx: &mut Context<Self>) {
        let Some(entry) = self.rings.get(ring_id) else {
            return;
        };
        let demo = entry.demo;
        if !demo && let Some(launcher) = &self.call_launcher {
            launcher.decline_ring(ring_id);
        }
        self.signal_ring(ring_id, RingSignal::Decline, cx);
    }

    pub fn conversation_of_missed_call(&self, missed: &MissedCall) -> Option<String> {
        if let Some(thread_id) = missed.thread_id.as_ref().filter(|id| self.sidebar.chats.iter().any(|chat| &chat.id == *id)) {
            return Some(thread_id.clone());
        }
        let user_id = missed.caller_mri.strip_prefix(ORGID_PREFIX)?;
        self.sidebar
            .chats
            .iter()
            .find(|chat| {
                crate::data::is_one_on_one(chat)
                    && chat.members.iter().any(|member| member.user_id.as_deref() == Some(user_id))
            })
            .map(|chat| chat.id.clone())
    }
}

impl AppState {
    pub fn running_meeting(&self, conversation_id: &str) -> Option<&LiveMeeting> {
        self.live_meetings
            .get(conversation_id)
            .filter(|meeting| meeting.is_running(now_unix()) && meeting.target().is_some())
    }

    pub fn demo_live_meetings(&mut self) {
        for conversation_id in ["demo-chat-meeting-standup", DEMO_LOBBY_MEETING] {
            self.live_meetings.insert(
                conversation_id.to_owned(),
                LiveMeeting {
                    thread_id: conversation_id.to_owned(),
                    conversation_url: None,
                    expiration: Some(now_unix() + DEMO_LIVE_MINUTES * 60),
                    organizer_id: Some("demo-jonas".to_owned()),
                    tenant_id: Some("demo-tenant".to_owned()),
                    meeting_code: None,
                    passcode: None,
                },
            );
        }
    }

    pub fn watches_live_meeting(&self, conversation_id: &str) -> bool {
        let meeting_chat = self
            .sidebar
            .chats
            .iter()
            .any(|chat| chat.id == conversation_id && chat.kind.eq_ignore_ascii_case(MEETING_KIND));
        meeting_chat || matches!(selection_for(&self.sidebar, conversation_id), Some(Selection::Channel(_)))
    }

    pub fn refresh_live_meeting(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.mode.demo || !self.watches_live_meeting(conversation_id) {
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            return;
        };
        if !self.live_refreshing.insert(conversation_id.to_owned()) {
            return;
        }
        let thread_id = conversation_id.to_owned();
        let receiver = crate::runtime::spawn(async move { launcher.live_meeting(&thread_id).await });
        let conversation_id = conversation_id.to_owned();
        cx.spawn(async move |this, cx| {
            let meeting = receiver.await.ok().flatten();
            this.update(cx, |state, cx| state.apply_live_meeting(conversation_id, meeting, cx)).ok();
        })
        .detach();
    }

    pub fn apply_live_meeting(&mut self, conversation_id: String, meeting: Option<LiveMeeting>, cx: &mut Context<Self>) {
        self.live_refreshing.remove(&conversation_id);
        let changed = match meeting {
            Some(meeting) => self.live_meetings.insert(conversation_id.clone(), meeting.clone()) != Some(meeting),
            None => self.live_meetings.remove(&conversation_id).is_some(),
        };
        if changed {
            cx.emit(AppEvent::LiveMeeting);
            cx.notify();
        }
    }

    pub fn on_thread_changed(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        self.refresh_live_meeting(conversation_id, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use std::path::PathBuf;

    use calling::{BackgroundChoice, CallCommand, CallControl, CallState, CallUpdate, EndKind, EndReason, EngineEvent, HoldState, Reaction, call_channel};
    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use store::{ChatRecord, MemberRecord, Store};

    use super::{ActiveCall, CallModel};
    use crate::app_state::{AppState, Mode, Selection};
    use crate::call::TransferCandidate;
    use crate::call::background::BackgroundPick;
    use crate::call::demo::demo_ring;
    use crate::call::features::room_closed_move;
    use crate::call::model::{BreakoutState, ConsultCall};
    use crate::data::Person;

    fn app_with_call(cx: &mut TestAppContext) -> Entity<AppState> {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let (handle, _control) = call_channel();
        cx.update(|cx| {
            app.update(cx, |state, _| {
                state.call = Some(ActiveCall::new(7, CallModel::test(), handle.commands, None, handle.video));
            })
        });
        app
    }

    fn app_with_commands(cx: &mut TestAppContext, model: CallModel, conversation_id: Option<&str>) -> (Entity<AppState>, CallControl) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let (handle, control) = call_channel();
        let conversation_id = conversation_id.map(str::to_owned);
        cx.update(|cx| {
            app.update(cx, |state, _| {
                state.call = Some(ActiveCall::new(7, model, handle.commands, conversation_id, handle.video));
            })
        });
        (app, control)
    }

    fn live_meeting(can_end: bool) -> CallModel {
        let mut model = CallModel::meeting("Standup", can_end);
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model.apply(CallUpdate::OwnIdentity { mri: "8:orgid:me".into() });
        model
    }

    #[gpui_kit::test]
    fn the_computer_sound_switch_is_remembered_and_sent_to_the_call(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(false), None);
        let store = cx.update(|cx| app.read(cx).store.clone());
        assert!(!cx.update(|cx| app.read(cx).call_share_sound));
        cx.update(|cx| app.update(cx, |state, cx| state.set_call_share_sound(true, cx)));
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetShareSound(true)));
        let reopened = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        assert!(cx.update(|cx| reopened.read(cx).call_share_sound));
        cx.update(|cx| app.update(cx, |state, cx| state.set_call_share_sound(false, cx)));
        let reopened = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        assert!(!cx.update(|cx| reopened.read(cx).call_share_sound));
    }

    #[gpui_kit::test]
    fn hands_and_reactions_become_call_commands_and_lowering_needs_the_organizer(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.toggle_call_hand(cx);
                state.send_call_reaction(Reaction::Heart, cx);
                state.lower_call_hand("8:orgid:a".into(), cx);
                state.lower_all_call_hands(cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetHand(true)));
        assert_eq!(control.try_recv_command(), Some(CallCommand::SendReaction(Reaction::Heart)));
        assert_eq!(control.try_recv_command(), None);
        let (organizer, mut organizer_control) = app_with_commands(cx, live_meeting(true), None);
        cx.update(|cx| {
            organizer.update(cx, |state, cx| {
                state.apply_call_update(7, CallUpdate::Roster(vec![calling::RosterEntry {
                    mri: "8:orgid:a".into(),
                    display_name: "Ana".into(),
                    muted: false,
                    in_lobby: false,
                    has_video: false,
                    sharing: false,
                    hand: Some(calling::RaisedHand { state_id: "s".into(), rank: 1 }),
                    spotlight: None,
                    organizer: false,
                }]), cx);
                state.lower_call_hand("8:orgid:a".into(), cx);
                state.lower_all_call_hands(cx);
            })
        });
        assert_eq!(organizer_control.try_recv_command(), Some(CallCommand::LowerHand { mri: "8:orgid:a".into() }));
        assert_eq!(organizer_control.try_recv_command(), Some(CallCommand::LowerAllHands));
    }

    fn guest(mri: &str, name: &str, in_lobby: bool, spotlit: bool) -> calling::RosterEntry {
        calling::RosterEntry {
            mri: mri.into(),
            display_name: name.into(),
            muted: false,
            in_lobby,
            has_video: false,
            sharing: false,
            hand: None,
            spotlight: spotlit.then(|| calling::Spotlight { state_id: "sp".into(), rank: 1 }),
            organizer: false,
        }
    }

    #[gpui_kit::test]
    fn organizer_controls_become_commands_and_attendees_send_nothing(cx: &mut TestAppContext) {
        let (attendee, mut attendee_control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| {
            attendee.update(cx, |state, cx| {
                state.admit_call_guest("8:orgid:g".into(), cx);
                state.admit_all_call_guests(cx);
                state.deny_call_guest("8:orgid:g".into(), cx);
                state.mute_call_participant("8:orgid:a".into(), cx);
                state.mute_all_call(cx);
                state.remove_call_participant("8:orgid:a".into(), cx);
                state.toggle_call_spotlight("8:orgid:a".into(), cx);
            })
        });
        assert_eq!(attendee_control.try_recv_command(), None);
        let (organizer, mut control) = app_with_commands(cx, live_meeting(true), None);
        cx.update(|cx| {
            organizer.update(cx, |state, cx| {
                state.apply_call_update(7, CallUpdate::Roster(vec![guest("8:orgid:a", "Ana", false, false), guest("8:orgid:g", "Gast", true, false)]), cx);
                state.admit_call_guest("8:orgid:g".into(), cx);
                state.admit_all_call_guests(cx);
                state.deny_call_guest("8:orgid:g".into(), cx);
                state.mute_call_participant("8:orgid:a".into(), cx);
                state.mute_all_call(cx);
                state.remove_call_participant("8:orgid:a".into(), cx);
                state.toggle_call_spotlight("8:orgid:a".into(), cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::Admit { mri: "8:orgid:g".into() }));
        assert_eq!(control.try_recv_command(), Some(CallCommand::AdmitAll));
        assert_eq!(control.try_recv_command(), Some(CallCommand::Deny { mri: "8:orgid:g".into() }));
        assert_eq!(control.try_recv_command(), Some(CallCommand::MuteParticipant { mri: "8:orgid:a".into() }));
        assert_eq!(control.try_recv_command(), Some(CallCommand::MuteAll));
        assert_eq!(control.try_recv_command(), Some(CallCommand::RemoveParticipant { mri: "8:orgid:a".into() }));
        assert_eq!(control.try_recv_command(), Some(CallCommand::Spotlight { mri: "8:orgid:a".into() }));
    }

    #[gpui_kit::test]
    fn the_spotlight_button_stops_an_existing_spotlight_and_pinning_stays_local(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(true), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(7, CallUpdate::Roster(vec![guest("8:orgid:a", "Ana", false, true)]), cx);
                state.toggle_call_spotlight("8:orgid:a".into(), cx);
                state.toggle_call_pin("8:orgid:a", cx);
                assert_eq!(state.call.as_ref().unwrap().model.pinned.as_deref(), Some("8:orgid:a"));
                state.toggle_call_pin("8:orgid:a", cx);
                assert_eq!(state.call.as_ref().unwrap().model.pinned, None);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::StopSpotlight { mri: "8:orgid:a".into() }));
        assert_eq!(control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn captions_toggle_between_on_and_off_and_only_in_meetings(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.toggle_call_captions(cx);
                state.apply_call_update(7, CallUpdate::Captions(calling::CaptionState::On), cx);
                state.toggle_call_captions(cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetCaptions(true)));
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetCaptions(false)));
        let app = app_with_call(cx);
        cx.update(|cx| app.update(cx, |state, cx| state.toggle_call_captions(cx)));
    }

    #[gpui_kit::test]
    fn the_background_choice_is_remembered_and_sent_to_the_call(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(false), None);
        let store = cx.update(|cx| app.read(cx).store.clone());
        assert_eq!(cx.update(|cx| app.read(cx).call_background.clone()), BackgroundPick::None);
        cx.update(|cx| app.update(cx, |state, cx| state.set_call_background(BackgroundPick::Blur, cx)));
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetBackground(BackgroundChoice::Blur)));
        assert_eq!(cx.update(|cx| app.read(cx).call.as_ref().unwrap().model.background.clone()), BackgroundPick::Blur);
        let reopened = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        assert_eq!(cx.update(|cx| reopened.read(cx).call_background.clone()), BackgroundPick::Blur);
        let picture = PathBuf::from("/pictures/wall.jpg");
        cx.update(|cx| app.update(cx, |state, cx| state.set_call_background(BackgroundPick::Custom(picture.clone()), cx)));
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetBackground(BackgroundChoice::Image(picture.clone()))));
        let reopened = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        assert_eq!(cx.update(|cx| reopened.read(cx).call_background.clone()), BackgroundPick::Custom(picture));
        cx.update(|cx| app.update(cx, |state, cx| state.set_call_background(BackgroundPick::None, cx)));
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetBackground(BackgroundChoice::None)));
    }

    #[gpui_kit::test]
    fn a_default_image_that_is_not_downloaded_yet_sends_nothing(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| app.update(cx, |state, cx| state.set_call_background(BackgroundPick::Default("office_01".into()), cx)));
        assert_eq!(control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn only_the_organizer_starts_a_recording_and_the_notice_needs_a_consent_request(cx: &mut TestAppContext) {
        let (attendee, mut attendee_control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| {
            attendee.update(cx, |state, cx| {
                state.set_call_recording(true, cx);
                state.accept_recording_notice(cx);
            })
        });
        assert_eq!(attendee_control.try_recv_command(), None);
        let (organizer, mut control) = app_with_commands(cx, live_meeting(true), None);
        cx.update(|cx| {
            organizer.update(cx, |state, cx| {
                state.set_call_recording(true, cx);
                state.apply_call_update(7, CallUpdate::Recording(true), cx);
                state.set_call_recording(false, cx);
                state.accept_recording_notice(cx);
                state.apply_call_update(7, CallUpdate::ConsentRequired(true), cx);
                state.accept_recording_notice(cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetRecording { on: true, title: "Standup".into() }));
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetRecording { on: false, title: "Standup".into() }));
        assert_eq!(control.try_recv_command(), Some(CallCommand::ConsentToRecording));
        assert_eq!(control.try_recv_command(), None);
    }

    fn live_one_to_one() -> CallModel {
        let mut model = CallModel::people("Bea", &[("8:orgid:bea".into(), "Bea".into())]);
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model
    }

    #[gpui_kit::test]
    fn hold_and_resume_alternate_and_stop_while_the_other_side_holds(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_one_to_one(), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.toggle_call_hold(cx);
                state.apply_call_update(7, CallUpdate::Hold(HoldState::Local), cx);
                state.toggle_call_hold(cx);
                state.apply_call_update(7, CallUpdate::Hold(HoldState::Remote), cx);
                state.toggle_call_hold(cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::Hold(true)));
        assert_eq!(control.try_recv_command(), Some(CallCommand::Hold(false)));
        assert_eq!(control.try_recv_command(), None);
        let (meeting, mut meeting_control) = app_with_commands(cx, live_meeting(true), None);
        cx.update(|cx| meeting.update(cx, |state, cx| state.toggle_call_hold(cx)));
        assert_eq!(meeting_control.try_recv_command(), None);
    }

    fn candidate(name: &str) -> TransferCandidate {
        TransferCandidate { mri: format!("8:orgid:{name}"), name: name.into(), user_id: Some(name.into()), chat_id: format!("19:{name}") }
    }

    #[gpui_kit::test]
    fn a_blind_transfer_names_the_target_and_a_consulted_one_carries_the_replacement_link(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_one_to_one(), None);
        cx.update(|cx| app.update(cx, |state, cx| state.transfer_call_blind(candidate("ana"), cx)));
        assert_eq!(
            control.try_recv_command(),
            Some(CallCommand::Transfer { target: calling::Callee { mri: "8:orgid:ana".into(), display_name: "ana".into() }, replaces: None })
        );
        let (consulting, mut consult_control) = app_with_commands(cx, live_one_to_one(), None);
        let (consult_handle, _consult_updates) = call_channel();
        cx.update(|cx| {
            consulting.update(cx, |state, cx| {
                state.call.as_mut().unwrap().consult = Some(ConsultCall {
                    id: 8,
                    target: ("8:orgid:ana".into(), "Ana".into()),
                    commands: consult_handle.commands,
                    state: CallState::Connecting,
                    replacement: Some("https://cc.skype.com/replacement".into()),
                    transferring: false,
                });
                state.transfer_consulted(cx);
                state.call.as_mut().unwrap().consult.as_mut().unwrap().state = CallState::Connected { since: Instant::now() };
                state.transfer_consulted(cx);
            })
        });
        assert_eq!(
            consult_control.try_recv_command(),
            Some(CallCommand::Transfer {
                target: calling::Callee { mri: "8:orgid:ana".into(), display_name: "Ana".into() },
                replaces: Some("https://cc.skype.com/replacement".into()),
            })
        );
        assert_eq!(consult_control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn the_transfer_list_holds_one_to_one_chats_without_the_person_on_the_call(cx: &mut TestAppContext) {
        let app = app_with_chats(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                let names: Vec<String> = state.transfer_candidates().into_iter().map(|candidate| candidate.name).collect();
                assert_eq!(names, vec!["Bea".to_owned()]);
                let (handle, _control) = call_channel();
                let mut on_the_call = CallModel::people("Bea", &[("8:orgid:bea-id".into(), "Bea".into())]);
                on_the_call.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
                state.call = Some(ActiveCall::new(9, on_the_call, handle.commands, None, handle.video));
                assert!(state.transfer_candidates().is_empty());
                cx.notify();
            })
        });
    }

    fn room_move(returning: bool) -> calling::BreakoutMove {
        calling::BreakoutMove {
            room_name: "Room 1".into(),
            target: calling::MeetingTarget { thread_id: "19:meeting_room@thread.v2".into(), tenant_id: "t".into(), organizer_id: "o".into(), meeting_data: None },
            returning,
        }
    }

    fn main_target() -> calling::MeetingTarget {
        calling::MeetingTarget { thread_id: "19:meeting_main@thread.v2".into(), tenant_id: "t".into(), organizer_id: "o".into(), meeting_data: None }
    }

    #[gpui_kit::test]
    fn being_moved_to_a_room_leaves_the_call_and_remembers_the_way_back(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.call.as_mut().unwrap().meeting_target = Some(main_target());
                state.apply_call_update(7, CallUpdate::BreakoutMove(room_move(false)), cx);
                let pending = state.call.as_ref().unwrap().pending_move.clone().expect("a move is pending");
                assert_eq!(pending.target.thread_id, "19:meeting_room@thread.v2");
                assert_eq!(pending.title, "Room 1");
                let room = pending.breakout.expect("a room");
                assert_eq!(room.main, Some(main_target()));
                assert_eq!(room.main_title, "Standup");
                assert_eq!(state.notice.as_ref().map(|notice| notice.text.as_str()), Some("Moving you to Room 1"));
                state.apply_call_update(7, CallUpdate::BreakoutMove(room_move(false)), cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::Hangup));
        assert_eq!(control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn the_return_button_and_a_closed_room_both_lead_back_to_the_main_meeting(cx: &mut TestAppContext) {
        let mut room = live_meeting(false);
        room.breakout = Some(BreakoutState { room_name: "Room 1".into(), main: Some(main_target()), main_title: "Standup".into() });
        let (app, mut control) = app_with_commands(cx, room.clone(), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.return_to_main_meeting(cx);
                let pending = state.call.as_ref().unwrap().pending_move.clone().expect("a move is pending");
                assert_eq!(pending.target, main_target());
                assert_eq!(pending.title, "Standup");
                assert!(pending.breakout.is_none());
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::Hangup));
        let (closed, _control) = app_with_commands(cx, room, None);
        cx.update(|cx| {
            closed.update(cx, |state, _| {
                let call = state.call.as_ref().unwrap();
                let moved = room_closed_move(call, &EndReason::Remote(EndKind::RoomClosed)).expect("goes back");
                assert_eq!(moved.target, main_target());
                assert!(room_closed_move(call, &EndReason::Remote(EndKind::Normal)).is_none());
                assert!(room_closed_move(call, &EndReason::LocalHangup).is_none());
            })
        });
        let (plain, _control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| plain.update(cx, |state, _| assert!(room_closed_move(state.call.as_ref().unwrap(), &EndReason::Remote(EndKind::RoomClosed)).is_none())));
    }

    #[gpui_kit::test]
    fn the_whiteboard_menu_asks_for_the_board_unless_someone_already_shares_one(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_commands(cx, live_meeting(false), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.open_call_whiteboard(cx);
                state.apply_call_update(
                    7,
                    CallUpdate::Whiteboard(Some(calling::ContentShare {
                        session_id: "s".into(),
                        presenter: Some("8:orgid:ana".into()),
                        subject: "Whiteboard".into(),
                        url: Some("https://app.whiteboard.microsoft.com/x".into()),
                        whiteboard: true,
                    })),
                    cx,
                );
                state.open_call_whiteboard(cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::OpenWhiteboard { title: "Standup".into() }));
        assert_eq!(control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn the_chat_panel_opens_the_meeting_thread_and_the_badge_counts_only_while_closed(cx: &mut TestAppContext) {
        let (app, _control) = app_with_commands(cx, live_meeting(false), Some("19:list@thread.v2"));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.directory.unread_counts.insert("19:meeting_x@thread.v2".into(), 3);
                assert_eq!(state.call_chat_thread(), None);
                assert_eq!(state.call_chat_unread(), 0);
                state.apply_call_update(7, CallUpdate::MeetingChat("19:meeting_x@thread.v2".into()), cx);
                assert_eq!(state.call_chat_unread(), 3);
                state.toggle_call_chat(cx);
                assert_eq!(state.call_chat_thread().as_deref(), Some("19:meeting_x@thread.v2"));
                assert_eq!(state.call_chat_unread(), 0);
                state.toggle_call_chat(cx);
                assert_eq!(state.call_chat_thread(), None);
            })
        });
    }

    #[gpui_kit::test]
    fn a_call_without_a_chat_has_no_panel(cx: &mut TestAppContext) {
        let app = app_with_call(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.toggle_call_chat(cx);
                assert!(!state.call.as_ref().unwrap().chat_open);
            })
        });
    }

    fn app_with_chats(cx: &mut TestAppContext) -> Entity<AppState> {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        cx.update(|cx| {
            app.update(cx, |state, _| {
                state.directory.me = Some(Person {
                    user_id: "me-id".into(),
                    display_name: "Me".into(),
                });
                state.sidebar.chats = vec![
                    ChatRecord {
                        id: "19:meeting@thread.v2".into(),
                        kind: "meeting".into(),
                        members: vec![MemberRecord { user_id: Some("bea-id".into()), display_name: "Bea".into() }],
                        ..Default::default()
                    },
                    ChatRecord {
                        id: "19:dm@unq.gbl.spaces".into(),
                        kind: "oneOnOne".into(),
                        members: vec![
                            MemberRecord { user_id: Some("me-id".into()), display_name: "Me".into() },
                            MemberRecord { user_id: Some("bea-id".into()), display_name: "Bea".into() },
                        ],
                        ..Default::default()
                    },
                ];
            })
        });
        app
    }

    #[gpui_kit::test]
    fn declining_by_shortcut_only_acts_while_a_ring_shows(cx: &mut TestAppContext) {
        let app = app_with_chats(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.decline_ringing_call(cx);
                assert!(!state.rings.is_ringing());
                state.demo_incoming_ring(cx);
                assert!(state.rings.is_ringing());
                state.decline_ringing_call(cx);
                assert!(!state.rings.is_ringing());
            })
        });
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
    fn full_window_exists_only_while_someone_shares_and_escape_leaves_it(cx: &mut TestAppContext) {
        let app = app_with_call(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.toggle_stage_fullscreen(cx);
                assert!(!state.stage_fullscreen());
                state.apply_call_update(7, CallUpdate::ScreenShare(Some("8:orgid:a".into())), cx);
                state.toggle_stage_fullscreen(cx);
                assert!(state.stage_fullscreen());
                state.leave_stage_fullscreen(cx);
                assert!(!state.stage_fullscreen());
                state.toggle_stage_fullscreen(cx);
                state.apply_call_update(7, CallUpdate::ScreenShare(None), cx);
                assert!(!state.stage_fullscreen());
            })
        });
    }

    #[gpui_kit::test]
    fn a_declined_call_names_who_declined(cx: &mut TestAppContext) {
        let app = app_with_call(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.call.as_mut().unwrap().model.peer_name = "Bea".into();
                state.apply_call_update(
                    7,
                    CallUpdate::State(CallState::Ended {
                        reason: EndReason::Remote(EndKind::Declined),
                    }),
                    cx,
                );
                assert_eq!(state.notice.as_ref().unwrap().text, "Bea declined");
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

    #[gpui_kit::test]
    fn a_ring_that_times_out_becomes_a_missed_call_event(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.on_engine_event(EngineEvent::Incoming(demo_ring()), cx);
                assert!(state.rings.is_ringing());
                state.tick_rings(cx);
                assert!(state.rings.is_ringing());
                state.on_engine_event(
                    EngineEvent::RingEnded {
                        ring_id: demo_ring().ring_id,
                        kind: EndKind::Cancelled,
                        answered_by: None,
                    },
                    cx,
                );
                assert!(!state.rings.is_ringing());
            })
        });
    }

    #[gpui_kit::test]
    fn calls_are_refused_for_meeting_chats_and_missing_chats(cx: &mut TestAppContext) {
        let app = app_with_chats(cx);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.start_chat_call("19:meeting@thread.v2", cx);
                assert!(state.call.is_none());
                assert_eq!(state.notice.as_ref().unwrap().text, "This chat cannot be called");
                state.start_chat_call("19:unknown", cx);
                assert!(state.call.is_none());
            })
        });
    }

    #[gpui_kit::test]
    fn a_missed_call_finds_the_one_on_one_chat_of_the_caller(cx: &mut TestAppContext) {
        let app = app_with_chats(cx);
        cx.update(|cx| {
            app.update(cx, |state, _| {
                let missed = super::MissedCall {
                    caller_mri: "8:orgid:bea-id".into(),
                    caller_name: "Bea".into(),
                    thread_id: None,
                    at: chrono::Utc::now(),
                };
                assert_eq!(state.conversation_of_missed_call(&missed).as_deref(), Some("19:dm@unq.gbl.spaces"));
                let direct = super::MissedCall { thread_id: Some("19:dm@unq.gbl.spaces".into()), ..missed.clone() };
                assert_eq!(state.conversation_of_missed_call(&direct).as_deref(), Some("19:dm@unq.gbl.spaces"));
            })
        });
    }

    #[gpui_kit::test]
    fn only_meeting_chats_and_channels_watch_for_a_running_meeting(cx: &mut TestAppContext) {
        let app = app_with_chats(cx);
        cx.update(|cx| {
            app.update(cx, |state, _| {
                assert!(state.watches_live_meeting("19:meeting@thread.v2"));
                assert!(!state.watches_live_meeting("19:dm@unq.gbl.spaces"));
                assert!(!state.watches_live_meeting("19:unknown"));
            })
        });
    }

    #[gpui_kit::test]
    fn a_live_meeting_shows_until_it_ends(cx: &mut TestAppContext) {
        let app = app_with_chats(cx);
        let meeting = calling::meeting::LiveMeeting {
            thread_id: "19:meeting@thread.v2".into(),
            conversation_url: None,
            expiration: Some(chrono::Utc::now().timestamp() + 600),
            organizer_id: Some("org".into()),
            tenant_id: Some("tenant".into()),
            meeting_code: None,
            passcode: None,
        };
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                assert!(state.running_meeting("19:meeting@thread.v2").is_none());
                state.apply_live_meeting("19:meeting@thread.v2".into(), Some(meeting), cx);
                assert!(state.running_meeting("19:meeting@thread.v2").is_some());
                state.apply_live_meeting("19:meeting@thread.v2".into(), None, cx);
                assert!(state.running_meeting("19:meeting@thread.v2").is_none());
            })
        });
    }
}
