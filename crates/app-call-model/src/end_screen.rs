use calling::{EndKind, EndReason, MeetingTarget};

use super::model::{ActiveCall, CallKind};

const ORGID_PREFIX: &str = "8:orgid:";
pub const DISMISS_LABEL: &str = "Dismiss";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndTone {
    CallEnded,
    MeetingLeft,
}

impl EndTone {
    pub const ALL: [EndTone; 2] = [EndTone::CallEnded, EndTone::MeetingLeft];

    pub fn file_name(self) -> &'static str {
        match self {
            EndTone::CallEnded => "Teams_Call_Ended.mp3",
            EndTone::MeetingLeft => "Teams_Meet_Up_Leave.mp3",
        }
    }
}

pub fn end_tone(kind: CallKind, moving_rooms: bool) -> Option<EndTone> {
    if moving_rooms {
        return None;
    }
    Some(match kind {
        CallKind::Meeting => EndTone::MeetingLeft,
        CallKind::Test | CallKind::Direct | CallKind::Group => EndTone::CallEnded,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCause {
    Removed,
    Dropped,
}

pub fn end_cause(reason: &EndReason, kind: CallKind) -> Option<EndCause> {
    match reason {
        EndReason::Dropped => Some(EndCause::Dropped),
        EndReason::Remote(EndKind::Removed) if kind == CallKind::Meeting => Some(EndCause::Removed),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndPeer {
    pub user_id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallEndScreen {
    pub cause: EndCause,
    pub kind: CallKind,
    pub title: String,
    pub conversation_id: Option<String>,
    pub meeting_target: Option<MeetingTarget>,
    pub peer: Option<EndPeer>,
}

impl CallEndScreen {
    pub fn for_call(call: &ActiveCall, reason: &EndReason) -> Option<Self> {
        let model = &call.model;
        let cause = end_cause(reason, model.kind)?;
        let peer = (model.kind == CallKind::Direct)
            .then(|| model.tiles.first())
            .flatten()
            .map(|tile| EndPeer {
                user_id: tile
                    .user_id
                    .clone()
                    .or_else(|| tile.mri.strip_prefix(ORGID_PREFIX).map(str::to_owned)),
                name: tile.name.clone(),
            });
        Some(CallEndScreen {
            cause,
            kind: model.kind,
            title: model.title.clone(),
            conversation_id: call.conversation_id.clone(),
            meeting_target: call.meeting_target.clone(),
            peer,
        })
    }

    fn is_two_party(&self) -> bool {
        matches!(self.kind, CallKind::Direct | CallKind::Test)
    }

    pub fn heading(&self) -> &'static str {
        match (self.cause, self.kind) {
            (EndCause::Removed, CallKind::Meeting) => "Removed from the meeting",
            (EndCause::Removed, _) => "Removed from the call",
            (EndCause::Dropped, _) => "Call dropped",
        }
    }

    pub fn description(&self) -> &'static str {
        match self.cause {
            EndCause::Removed if self.kind == CallKind::Meeting => {
                "Someone just removed you from the meeting. You can try to rejoin."
            }
            EndCause::Removed => "Someone just removed you from the call. You can try to rejoin.",
            EndCause::Dropped if self.is_two_party() => {
                "Sorry we couldn't reconnect your call. Make sure you have an internet connection and try calling back."
            }
            EndCause::Dropped => {
                "Sorry we couldn't get you back into the meeting. Make sure you have an internet connection and try to rejoin."
            }
        }
    }

    pub fn retry_label(&self) -> &'static str {
        match self.cause {
            EndCause::Dropped if self.is_two_party() => "Call back",
            EndCause::Removed | EndCause::Dropped => "Rejoin",
        }
    }

    pub fn can_retry(&self) -> bool {
        match self.kind {
            CallKind::Test => true,
            CallKind::Meeting => self.meeting_target.is_some(),
            CallKind::Direct | CallKind::Group => self.conversation_id.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(cause: EndCause, kind: CallKind) -> CallEndScreen {
        CallEndScreen {
            cause,
            kind,
            title: "Standup".to_owned(),
            conversation_id: Some("19:chat".to_owned()),
            meeting_target: None,
            peer: None,
        }
    }

    #[test]
    fn meetings_leave_with_the_meetup_tone_and_calls_with_the_call_tone() {
        assert_eq!(
            end_tone(CallKind::Meeting, false),
            Some(EndTone::MeetingLeft)
        );
        assert_eq!(end_tone(CallKind::Direct, false), Some(EndTone::CallEnded));
        assert_eq!(end_tone(CallKind::Group, false), Some(EndTone::CallEnded));
        assert_eq!(end_tone(CallKind::Test, false), Some(EndTone::CallEnded));
        assert_eq!(end_tone(CallKind::Meeting, true), None);
    }

    #[test]
    fn only_drops_and_removals_from_meetings_get_an_end_screen() {
        let removed = EndReason::Remote(EndKind::Removed);
        assert_eq!(
            end_cause(&EndReason::Dropped, CallKind::Direct),
            Some(EndCause::Dropped)
        );
        assert_eq!(
            end_cause(&removed, CallKind::Meeting),
            Some(EndCause::Removed)
        );
        assert_eq!(end_cause(&removed, CallKind::Direct), None);
        assert_eq!(end_cause(&EndReason::LocalHangup, CallKind::Meeting), None);
        assert_eq!(
            end_cause(&EndReason::Remote(EndKind::Normal), CallKind::Direct),
            None
        );
    }

    #[test]
    fn texts_follow_teams_per_case() {
        let removed = screen(EndCause::Removed, CallKind::Meeting);
        assert_eq!(removed.heading(), "Removed from the meeting");
        assert_eq!(removed.retry_label(), "Rejoin");
        let dropped_call = screen(EndCause::Dropped, CallKind::Direct);
        assert_eq!(dropped_call.heading(), "Call dropped");
        assert_eq!(dropped_call.retry_label(), "Call back");
        assert!(dropped_call.description().contains("try calling back"));
        let dropped_meeting = screen(EndCause::Dropped, CallKind::Meeting);
        assert_eq!(dropped_meeting.retry_label(), "Rejoin");
        assert!(
            dropped_meeting
                .description()
                .contains("back into the meeting")
        );
    }

    #[test]
    fn a_meeting_needs_its_target_to_rejoin() {
        let mut meeting = screen(EndCause::Removed, CallKind::Meeting);
        assert!(!meeting.can_retry());
        meeting.meeting_target = Some(MeetingTarget {
            thread_id: "19:meeting".to_owned(),
            tenant_id: "tenant".to_owned(),
            organizer_id: "organizer".to_owned(),
            meeting_data: None,
        });
        assert!(meeting.can_retry());
        assert!(screen(EndCause::Dropped, CallKind::Group).can_retry());
    }
}
