use calling::MeetingTarget;
use gpui_kit::*;
use teams_core::{
    MeetingCode, MeetingLink, ThreadMeeting, find_meeting_link, meeting_code_from_id,
    parse_meeting_link,
};

use super::actions::{MEETING_UNAVAILABLE_NOTICE, NOT_CONNECTED_NOTICE};
use super::launcher::NOT_READY;
use super::prejoin::{DemoMeeting, MeetingJoin};
use crate::app_state::{AppState, selection_for, selection_title};
use crate::runtime;

const NOT_FOUND_NOTICE: &str = "Could not find that meeting";
const BAD_ID_MESSAGE: &str = "Enter the meeting ID from the invitation.";
const DEMO_GUEST_COUNT: usize = 4;
const DEMO_CODE_THREAD: &str = "19:meeting_demo@thread.v2";
const MEETING_TITLE: &str = "Meeting";
const MEET_NOW_SUBJECT: &str = "Meet now";
const MEET_NOW_FAILED_NOTICE: &str = "Could not start the meeting";

pub fn meeting_target(meeting: &ThreadMeeting) -> MeetingTarget {
    MeetingTarget {
        thread_id: meeting.thread_id.clone(),
        tenant_id: meeting.tenant_id.clone(),
        organizer_id: meeting.organizer_id.clone(),
        meeting_data: None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinMode {
    Prejoin,
    Direct,
}

impl AppState {
    fn join_resolved_meeting(
        &mut self,
        target: MeetingTarget,
        mode: JoinMode,
        cx: &mut Context<Self>,
    ) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        if self.try_web_call(super::web_call::meeting_web_url(&target), cx) {
            return;
        }
        let title = selection_for(&self.sidebar, &target.thread_id)
            .map(|selection| selection_title(&self.sidebar, &selection))
            .unwrap_or_else(|| MEETING_TITLE.to_owned());
        let organizer = self
            .directory
            .me
            .as_ref()
            .is_some_and(|me| me.user_id.eq_ignore_ascii_case(&target.organizer_id));
        let join = MeetingJoin {
            conversation_id: Some(target.thread_id.clone()),
            target,
            title,
            organizer,
            demo: DemoMeeting {
                guests: DEMO_GUEST_COUNT,
                lobby: false,
                organizer: false,
            },
        };
        match mode {
            JoinMode::Prejoin => self.stage_meeting_join(join, cx),
            JoinMode::Direct => {
                let options = self.join_options.clone();
                self.start_meeting(join, Some(&options), cx)
            }
        }
    }

    fn join_meeting_code(&mut self, code: MeetingCode, mode: JoinMode, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        let meeting_data = code.meeting_data();
        if self.mode.demo {
            let target = MeetingTarget {
                thread_id: DEMO_CODE_THREAD.to_owned(),
                tenant_id: "demo-tenant".to_owned(),
                organizer_id: "demo-organizer".to_owned(),
                meeting_data: Some(meeting_data),
            };
            self.join_resolved_meeting(target, mode, cx);
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            self.raise_notice(NOT_CONNECTED_NOTICE.to_owned(), None, cx);
            return;
        };
        let receiver = runtime::spawn(async move { launcher.resolve_meeting(&meeting_data).await });
        cx.spawn(async move |this, cx| {
            let resolved = receiver.await.ok().unwrap_or_else(|| Err(String::new()));
            this.update(cx, |state, cx| match resolved {
                Ok(target) => state.join_resolved_meeting(target, mode, cx),
                Err(message) if message == NOT_READY => state.raise_notice(message, None, cx),
                Err(_) => state.raise_notice(NOT_FOUND_NOTICE.to_owned(), None, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn join_meeting_link(&mut self, text: &str, cx: &mut Context<Self>) {
        self.join_link(text, JoinMode::Prejoin, cx);
    }

    pub(crate) fn join_link(&mut self, text: &str, mode: JoinMode, cx: &mut Context<Self>) {
        match parse_meeting_link(text) {
            Some(MeetingLink::Thread(meeting)) => {
                self.join_resolved_meeting(meeting_target(&meeting), mode, cx)
            }
            Some(MeetingLink::Code(code)) => self.join_meeting_code(code, mode, cx),
            None => self.raise_notice(MEETING_UNAVAILABLE_NOTICE.to_owned(), None, cx),
        }
    }

    pub fn start_meet_now(&mut self, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        if self.meet_now_pending {
            return;
        }
        if self.mode.demo {
            let target = MeetingTarget {
                thread_id: DEMO_CODE_THREAD.to_owned(),
                tenant_id: "demo-tenant".to_owned(),
                organizer_id: "demo-organizer".to_owned(),
                meeting_data: None,
            };
            self.join_resolved_meeting(target, JoinMode::Prejoin, cx);
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            self.raise_notice(NOT_CONNECTED_NOTICE.to_owned(), None, cx);
            return;
        };
        self.meet_now_pending = true;
        cx.notify();
        let receiver =
            runtime::spawn(async move { launcher.create_meet_now(MEET_NOW_SUBJECT).await });
        cx.spawn(async move |this, cx| {
            let created = receiver.await.ok().and_then(Result::ok);
            this.update(cx, |state, cx| {
                state.meet_now_pending = false;
                match created {
                    Some(url) => state.join_link(&url, JoinMode::Prejoin, cx),
                    None => state.raise_notice(MEET_NOW_FAILED_NOTICE.to_owned(), None, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn join_meeting_by_id(
        &mut self,
        meeting_id: &str,
        passcode: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some((url, _)) = find_meeting_link(meeting_id) {
            self.join_meeting_link(&url, cx);
            return Ok(());
        }
        let code =
            meeting_code_from_id(meeting_id, passcode).ok_or_else(|| BAD_ID_MESSAGE.to_owned())?;
        self.join_meeting_code(code, JoinMode::Prejoin, cx);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::{AppContext as _, TestAppContext};
    use store::Store;

    use crate::app_state::{AppState, Mode};

    #[gpui_kit::test]
    fn meet_now_opens_the_prejoin_like_teams(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let mode = Mode {
            demo: true,
            ..Mode::default()
        };
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, mode)));
        app.update(cx, |state, cx| state.start_meet_now(cx));
        app.read_with(cx, |state, _| {
            assert!(state.prejoin.is_some());
            assert!(state.call.is_none());
        });
    }
}
