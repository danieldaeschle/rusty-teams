use gpui_kit::*;

use super::end_screen::{CallEndScreen, EndCause};
use super::model::{CallKind, PendingMove};
use crate::app_state::{AppEvent, AppState};

const DEMO_MEETING_THREAD: &str = "demo-chat-meeting-standup";

impl AppState {
    pub fn dismiss_call_end(&mut self, cx: &mut Context<Self>) {
        if self.call_end.take().is_some() {
            cx.emit(AppEvent::Call);
            cx.notify();
        }
    }

    pub fn retry_call_end(&mut self, cx: &mut Context<Self>) {
        let Some(screen) = self.call_end.take() else {
            return;
        };
        cx.emit(AppEvent::Call);
        cx.notify();
        match (screen.kind, screen.meeting_target, screen.conversation_id) {
            (CallKind::Test, _, _) => self.start_test_call(cx),
            (CallKind::Meeting, Some(target), _) => self.join_moved_call(
                PendingMove {
                    target,
                    title: screen.title,
                    breakout: None,
                },
                cx,
            ),
            (CallKind::Group, _, Some(conversation_id))
                if self.running_meeting(&conversation_id).is_some() =>
            {
                self.join_meeting_call(&conversation_id, cx)
            }
            (CallKind::Direct | CallKind::Group, _, Some(conversation_id)) => {
                self.start_chat_call(&conversation_id, cx)
            }
            _ => {}
        }
    }

    pub fn demo_call_end(&mut self, cause: EndCause, cx: &mut Context<Self>) {
        let screen = match cause {
            EndCause::Removed => CallEndScreen {
                cause,
                kind: CallKind::Meeting,
                title: "Standup".to_owned(),
                conversation_id: Some(DEMO_MEETING_THREAD.to_owned()),
                meeting_target: Some(calling::MeetingTarget {
                    thread_id: DEMO_MEETING_THREAD.to_owned(),
                    tenant_id: "demo-tenant".to_owned(),
                    organizer_id: "demo-me".to_owned(),
                    meeting_data: None,
                }),
                peer: None,
            },
            EndCause::Dropped => CallEndScreen {
                cause,
                kind: CallKind::Direct,
                title: "Mara Lindqvist".to_owned(),
                conversation_id: Some("demo-chat-mara".to_owned()),
                meeting_target: None,
                peer: Some(super::end_screen::EndPeer {
                    user_id: Some("demo-mara".to_owned()),
                    name: "Mara Lindqvist".to_owned(),
                }),
            },
        };
        self.call_end = Some(screen);
        cx.emit(AppEvent::Call);
        cx.notify();
    }
}
