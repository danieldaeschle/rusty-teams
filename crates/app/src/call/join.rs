use calling::{CallSpec, MeetingTarget};
use gpui_kit::*;
use teams_core::{
    MeetingCode, MeetingLink, ThreadMeeting, find_meeting_link, meeting_code_from_id,
    parse_meeting_link,
};

use super::actions::{MEETING_UNAVAILABLE_NOTICE, NOT_CONNECTED_NOTICE};
use super::demo::{DemoCall, demo_guests};
use super::model::CallModel;
use crate::app_state::{AppState, selection_title};
use crate::notify::selection_for;
use crate::runtime;

const NO_LINK_NOTICE: &str = "No Teams meeting link in the clipboard";
const NOT_FOUND_NOTICE: &str = "Could not find that meeting";
const BAD_ID_MESSAGE: &str = "Enter the meeting ID from the invitation.";
const DEMO_GUEST_COUNT: usize = 4;
const DEMO_CODE_THREAD: &str = "19:meeting_demo@thread.v2";
const MEETING_TITLE: &str = "Meeting";

pub fn meeting_target(meeting: &ThreadMeeting) -> MeetingTarget {
    MeetingTarget {
        thread_id: meeting.thread_id.clone(),
        tenant_id: meeting.tenant_id.clone(),
        organizer_id: meeting.organizer_id.clone(),
        meeting_data: None,
    }
}

impl AppState {
    fn join_resolved_meeting(&mut self, target: MeetingTarget, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
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
        let thread_id = target.thread_id.clone();
        let demo = DemoCall::Meeting {
            guests: demo_guests(DEMO_GUEST_COUNT),
            lobby: false,
            organizer: false,
        };
        let Some(handle) = self.place_call(CallSpec::Meeting(target), demo, cx) else {
            return;
        };
        self.begin_call(
            handle,
            CallModel::meeting(&title, organizer),
            Some(thread_id),
            cx,
        );
    }

    fn join_meeting_code(&mut self, code: MeetingCode, cx: &mut Context<Self>) {
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
            self.join_resolved_meeting(target, cx);
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            self.raise_notice(NOT_CONNECTED_NOTICE.to_owned(), None, cx);
            return;
        };
        let receiver = runtime::spawn(async move { launcher.resolve_meeting(&meeting_data).await });
        cx.spawn(async move |this, cx| {
            let resolved = receiver.await.ok().and_then(Result::ok);
            this.update(cx, |state, cx| match resolved {
                Some(target) => state.join_resolved_meeting(target, cx),
                None => state.raise_notice(NOT_FOUND_NOTICE.to_owned(), None, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn join_meeting_link(&mut self, text: &str, cx: &mut Context<Self>) {
        match parse_meeting_link(text) {
            Some(MeetingLink::Thread(meeting)) => {
                self.join_resolved_meeting(meeting_target(&meeting), cx)
            }
            Some(MeetingLink::Code(code)) => self.join_meeting_code(code, cx),
            None => self.raise_notice(MEETING_UNAVAILABLE_NOTICE.to_owned(), None, cx),
        }
    }

    pub fn join_meeting_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let link = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| find_meeting_link(&text));
        match link {
            Some((url, _)) => self.join_meeting_link(&url, cx),
            None => self.raise_notice(NO_LINK_NOTICE.to_owned(), None, cx),
        }
    }

    pub fn join_meeting_by_id(
        &mut self,
        meeting_id: &str,
        passcode: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let code =
            meeting_code_from_id(meeting_id, passcode).ok_or_else(|| BAD_ID_MESSAGE.to_owned())?;
        self.join_meeting_code(code, cx);
        Ok(())
    }
}
