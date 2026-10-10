use std::time::{Duration, Instant};

use calling::meeting::LiveMeeting;
use calling::{
    CallCommand, CallHandle, CallSpec, CallUpdate, Callee, DeviceChoice, EngineEvent, MuteCommand, RingSignal, ShareSource, VideoKey,
};
use gpui_kit::*;

use super::demo::{DemoCall, demo_caller, demo_guests, demo_ring, start_demo_call};
use super::model::{ActiveCall, CallModel, ended_notice};
use super::pictures::CallPictures;
use super::ring::{MissedCall, RingOutcome};
use super::target::{is_organizer, plan_for_chat};
use crate::app_state::{AppEvent, AppState, Selection, chat_title};
use crate::notify::selection_for;

const TIMER_REFRESH: Duration = Duration::from_millis(500);
const SHARE_SOURCES_EVERY_TICKS: u32 = 10;
const NOT_CONNECTED_NOTICE: &str = "Not connected to Teams yet";
const READ_ONLY_NOTICE: &str = "Calls are off in read-only mode";
const NOT_CALLABLE_NOTICE: &str = "This chat cannot be called";
const MEETING_UNAVAILABLE_NOTICE: &str = "This meeting cannot be joined";
const ANSWER_FAILED_NOTICE: &str = "Could not answer the call";
const MEETING_KIND: &str = "meeting";
const DEMO_LOBBY_MEETING: &str = "demo-chat-meeting-retro";
const DEMO_CROWD: usize = 11;
const DEMO_SMALL_MEETING: usize = 4;
const DEMO_LIVE_MINUTES: i64 = 60;
const ORGID_PREFIX: &str = "8:orgid:";

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
        };
        let Some(handle) = self.place_call(CallSpec::Meeting(target), demo, cx) else {
            return;
        };
        let title = selection_for(&self.sidebar, conversation_id)
            .map(|selection| crate::app_state::selection_title(&self.sidebar, &selection))
            .unwrap_or_else(|| "Meeting".to_owned());
        let model = CallModel::meeting(&title, is_organizer(&meeting, self.directory.me.as_ref()));
        self.begin_call(handle, model, Some(conversation_id.to_owned()), cx);
    }

    fn place_call(&mut self, spec: CallSpec, demo: DemoCall, cx: &mut Context<Self>) -> Option<CallHandle> {
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

    fn begin_call(
        &mut self,
        handle: CallHandle,
        model: CallModel,
        conversation_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.call_count += 1;
        let call_id = self.call_count;
        let CallHandle { commands, updates, video } = handle;
        self.call = Some(ActiveCall {
            id: call_id,
            model,
            commands,
            viewing: true,
            conversation_id,
            video,
            pictures: CallPictures::default(),
            stage_fullscreen: false,
        });
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
        call.pictures.clear(cx);
        self.call = None;
        self.raise_notice(notice, None, cx);
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

    pub fn open_call_chat(&mut self, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.call.as_ref().and_then(|call| call.conversation_id.clone()) else {
            return;
        };
        if let Some(selection) = selection_for(&self.sidebar, &conversation_id) {
            self.select(selection, cx);
        }
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
            self.accept_ring(ring_id, cx);
        }
    }

    pub fn decline_ringing_call(&mut self, cx: &mut Context<Self>) {
        if let Some(ring_id) = self.rings.first_ringing_id() {
            self.decline_ring(ring_id, cx);
        }
    }

    pub fn accept_ring(&mut self, ring_id: u64, cx: &mut Context<Self>) {
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
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            self.raise_notice(ANSWER_FAILED_NOTICE.to_owned(), None, cx);
            return;
        };
        let receiver = crate::runtime::spawn(async move { launcher.accept_ring(ring_id).await });
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

    use calling::{CallState, CallUpdate, EndKind, EndReason, EngineEvent, call_channel};
    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use store::{ChatRecord, MemberRecord, Store};

    use super::{ActiveCall, CallModel, CallPictures};
    use crate::app_state::{AppState, Mode, Selection};
    use crate::call::demo::demo_ring;
    use crate::data::Person;

    fn app_with_call(cx: &mut TestAppContext) -> Entity<AppState> {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let (handle, _control) = call_channel();
        cx.update(|cx| {
            app.update(cx, |state, _| {
                state.call = Some(ActiveCall {
                    id: 7,
                    model: CallModel::test(),
                    commands: handle.commands,
                    viewing: true,
                    conversation_id: None,
                    video: handle.video,
                    pictures: CallPictures::default(),
                    stage_fullscreen: false,
                });
            })
        });
        app
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
