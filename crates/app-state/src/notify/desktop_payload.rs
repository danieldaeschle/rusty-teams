use super::stack::{MENTION_DURATION, MESSAGE_DURATION, ToastModel};
use super::text::describe;
use crate::call::ReminderPhase;
use crate::call::reminder::MeetingToast;

pub mod action {
    pub const DEFAULT: &str = "default";
    pub const MARK_READ: &str = "mark-read";
    pub const REPLY: &str = "inline-reply";
    pub const DECLINE: &str = "decline";
    pub const ACCEPT_AUDIO: &str = "accept-audio";
    pub const ACCEPT_VIDEO: &str = "accept-video";
    pub const JOIN: &str = "join";
    pub const SNOOZE: &str = "snooze";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low = 0,
    Normal = 1,
    Critical = 2,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DesktopKey {
    Message(u64),
    Ring(u64),
    Meeting(u64),
}

impl DesktopKey {
    pub fn replace_token(&self) -> String {
        match self {
            DesktopKey::Message(id) => format!("message:{id}"),
            DesktopKey::Ring(id) => format!("ring:{id}"),
            DesktopKey::Meeting(id) => format!("meeting:{id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAction {
    pub id: &'static str,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPayload {
    pub key: DesktopKey,
    pub replace_key: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<DesktopAction>,
    /// Milliseconds. `0` means do not expire.
    pub timeout_ms: i32,
    pub urgency: Urgency,
    pub category: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLabels {
    pub mark_read: String,
    pub reply: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingLabels {
    pub decline: String,
    pub accept: String,
    pub audio: String,
    pub video: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingLabels {
    pub join: String,
    pub snooze: String,
}

pub fn message_payload(
    model: &ToastModel,
    preview_on: bool,
    labels: &MessageLabels,
) -> DesktopPayload {
    let text = describe(model, preview_on);
    let body = if model.agent.is_some() {
        if text.preview.is_empty() {
            model
                .agent
                .as_ref()
                .map(|agent| agent.body.clone())
                .unwrap_or_default()
        } else {
            text.preview.clone()
        }
    } else {
        text.preview.clone()
    };
    let summary = if let Some(agent) = &model.agent {
        if agent.title.is_empty() {
            text.title.clone()
        } else {
            agent.title.clone()
        }
    } else {
        text.title.clone()
    };
    let mention = model.mentions_me;
    let timeout_ms = if mention {
        MENTION_DURATION.as_millis() as i32
    } else {
        MESSAGE_DURATION.as_millis() as i32
    };
    let urgency = if model.urgent {
        Urgency::Critical
    } else {
        Urgency::Normal
    };
    let mut actions = Vec::new();
    if model.org.is_none() {
        actions.push(DesktopAction {
            id: action::MARK_READ,
            label: labels.mark_read.clone(),
        });
    }
    if model.agent.is_none() && !model.status_change && model.org.is_none() {
        actions.push(DesktopAction {
            id: action::REPLY,
            label: labels.reply.clone(),
        });
    }
    DesktopPayload {
        key: DesktopKey::Message(model.id),
        replace_key: format!("conversation:{}", model.conversation_id),
        summary,
        body,
        actions,
        timeout_ms,
        urgency,
        category: if model.agent.is_some() {
            "im"
        } else {
            "im.received"
        },
    }
}

pub fn ring_payload(
    ring_id: u64,
    caller_name: &str,
    subtitle: &str,
    video: bool,
    labels: &RingLabels,
) -> DesktopPayload {
    let accept_label = if video {
        labels.audio.clone()
    } else {
        labels.accept.clone()
    };
    let mut actions = vec![
        DesktopAction {
            id: action::DECLINE,
            label: labels.decline.clone(),
        },
        DesktopAction {
            id: action::ACCEPT_AUDIO,
            label: accept_label,
        },
    ];
    if video {
        actions.push(DesktopAction {
            id: action::ACCEPT_VIDEO,
            label: labels.video.clone(),
        });
    }
    DesktopPayload {
        key: DesktopKey::Ring(ring_id),
        replace_key: format!("ring:{ring_id}"),
        summary: caller_name.to_owned(),
        body: subtitle.to_owned(),
        actions,
        timeout_ms: 0,
        urgency: Urgency::Critical,
        category: "x-teams.call.incoming",
    }
}

pub fn meeting_payload(toast: &MeetingToast, body: &str, labels: &MeetingLabels) -> DesktopPayload {
    let mut actions = Vec::new();
    if toast.phase == ReminderPhase::Soon {
        actions.push(DesktopAction {
            id: action::SNOOZE,
            label: labels.snooze.clone(),
        });
    }
    actions.push(DesktopAction {
        id: action::JOIN,
        label: labels.join.clone(),
    });
    DesktopPayload {
        key: DesktopKey::Meeting(toast.id),
        replace_key: format!("meeting:{}", toast.event_key),
        summary: toast.subject.clone(),
        body: body.to_owned(),
        actions,
        timeout_ms: 0,
        urgency: Urgency::Normal,
        category: "x-teams.meeting.reminder",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::notify::rules::{ChatKind, Preview};
    use crate::notify::stack::{Fade, ReplyState, ToastTimer};

    fn message_labels() -> MessageLabels {
        MessageLabels {
            mark_read: "Mark as read".into(),
            reply: "Reply".into(),
        }
    }

    fn ring_labels() -> RingLabels {
        RingLabels {
            decline: "Decline".into(),
            accept: "Accept".into(),
            audio: "Audio".into(),
            video: "Video".into(),
        }
    }

    fn meeting_labels() -> MeetingLabels {
        MeetingLabels {
            join: "Join".into(),
            snooze: "Snooze".into(),
        }
    }

    fn toast(mention: bool, urgent: bool, preview: Preview) -> ToastModel {
        ToastModel {
            id: 7,
            conversation_id: "chat-a".into(),
            message_id: "m1".into(),
            kind: ChatKind::Direct,
            chat_title: String::new(),
            sender_id: Some("u1".into()),
            sender_name: "Priya Nair".into(),
            preview,
            mentions_me: mention,
            count: 1,
            timer: ToastTimer::new(Instant::now(), mention),
            reply: ReplyState::Closed,
            reply_text: String::new(),
            hovered: false,
            time: String::new(),
            queued: false,
            dismissed: false,
            fade: Fade::shown(Instant::now()),
            agent: None,
            status_change: false,
            urgent,
            org: None,
        }
    }

    #[test]
    fn preview_off_hides_message_text() {
        let model = toast(false, false, Preview::Text("secret".into()));
        let payload = message_payload(&model, false, &message_labels());
        assert_eq!(payload.body, "New message");
        assert_eq!(payload.replace_key, "conversation:chat-a");
        assert_eq!(payload.timeout_ms, MESSAGE_DURATION.as_millis() as i32);
    }

    #[test]
    fn mention_uses_longer_timeout() {
        let model = toast(true, false, Preview::Text("hi".into()));
        let payload = message_payload(&model, true, &message_labels());
        assert_eq!(payload.timeout_ms, MENTION_DURATION.as_millis() as i32);
    }

    #[test]
    fn urgent_sets_critical_urgency() {
        let model = toast(false, true, Preview::Text("down".into()));
        let payload = message_payload(&model, true, &message_labels());
        assert_eq!(payload.urgency, Urgency::Critical);
    }

    #[test]
    fn a_message_from_another_org_offers_no_actions() {
        let mut model = toast(false, false, Preview::Text("hi".into()));
        model.org = Some(app_base::orgs::Org {
            id: "guest".into(),
            name: "Guest Org".into(),
            guest: true,
        });
        assert!(
            message_payload(&model, true, &message_labels())
                .actions
                .is_empty()
        );
    }

    #[test]
    fn message_offers_mark_read_and_reply() {
        let model = toast(false, false, Preview::Text("hi".into()));
        let payload = message_payload(&model, true, &message_labels());
        let ids: Vec<_> = payload.actions.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![action::MARK_READ, action::REPLY]);
    }

    #[test]
    fn bundling_shares_conversation_replace_key() {
        let first = toast(false, false, Preview::Text("one".into()));
        let mut second = toast(false, false, Preview::Text("two".into()));
        second.id = 99;
        let a = message_payload(&first, true, &message_labels());
        let b = message_payload(&second, true, &message_labels());
        assert_eq!(a.replace_key, b.replace_key);
        assert_ne!(a.key, b.key);
    }

    #[test]
    fn audio_ring_has_no_timeout_and_no_video_action() {
        let payload = ring_payload(3, "Alex", "Incoming audio call", false, &ring_labels());
        assert_eq!(payload.timeout_ms, 0);
        assert_eq!(payload.urgency, Urgency::Critical);
        let ids: Vec<_> = payload.actions.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![action::DECLINE, action::ACCEPT_AUDIO]);
        assert_eq!(payload.actions[1].label, "Accept");
    }

    #[test]
    fn video_ring_offers_audio_and_video() {
        let payload = ring_payload(3, "Alex", "Incoming video call", true, &ring_labels());
        let ids: Vec<_> = payload.actions.iter().map(|a| a.id).collect();
        assert_eq!(
            ids,
            vec![action::DECLINE, action::ACCEPT_AUDIO, action::ACCEPT_VIDEO]
        );
        assert_eq!(payload.actions[1].label, "Audio");
    }

    #[test]
    fn meeting_soon_offers_snooze_and_join() {
        let toast = MeetingToast {
            id: 1,
            event_key: "e@1".into(),
            event_id: "e".into(),
            phase: ReminderPhase::Soon,
            subject: "Standup".into(),
            start: chrono::Utc::now(),
            join_url: "https://teams.microsoft.com/l/meetup-join/x".into(),
        };
        let payload = meeting_payload(&toast, "in 5 minutes", &meeting_labels());
        let ids: Vec<_> = payload.actions.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![action::SNOOZE, action::JOIN]);
        assert_eq!(payload.timeout_ms, 0);
    }

    #[test]
    fn meeting_live_drops_snooze_same_replace_key() {
        let soon = MeetingToast {
            id: 1,
            event_key: "e@1".into(),
            event_id: "e".into(),
            phase: ReminderPhase::Soon,
            subject: "Standup".into(),
            start: chrono::Utc::now(),
            join_url: "https://example".into(),
        };
        let live = MeetingToast {
            id: 2,
            phase: ReminderPhase::Live,
            ..soon.clone()
        };
        let a = meeting_payload(&soon, "soon", &meeting_labels());
        let b = meeting_payload(&live, "live now", &meeting_labels());
        assert_eq!(a.replace_key, b.replace_key);
        assert_eq!(
            b.actions.iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![action::JOIN]
        );
    }
}
