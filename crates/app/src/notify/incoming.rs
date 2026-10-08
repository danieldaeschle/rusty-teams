use std::collections::HashMap;

use chrono::{DateTime, Utc};
use store::{MessageRecord, Sidebar, Store};

use super::rules::{ChatKind, Incoming, Preview};
use crate::app_state::chat_title;

const RECENT_WINDOW: usize = 20;
const FILE_FALLBACK: &str = "File";

pub struct IncomingTracker {
    started: DateTime<Utc>,
    watermarks: HashMap<String, DateTime<Utc>>,
}

impl IncomingTracker {
    pub fn new(started: DateTime<Utc>) -> Self {
        IncomingTracker {
            started,
            watermarks: HashMap::new(),
        }
    }

    pub fn collect(
        &mut self,
        store: &Store,
        sidebar: &Sidebar,
        my_user_id: Option<&str>,
        conversation_id: &str,
    ) -> Vec<Incoming> {
        let Ok(records) = store.messages(conversation_id, None, RECENT_WINDOW) else {
            return Vec::new();
        };
        self.fresh(records, sidebar, my_user_id)
    }

    fn fresh(
        &mut self,
        records: Vec<MessageRecord>,
        sidebar: &Sidebar,
        my_user_id: Option<&str>,
    ) -> Vec<Incoming> {
        let mut incoming = Vec::new();
        for record in records {
            let watermark = self
                .watermarks
                .get(&record.conversation_id)
                .copied()
                .unwrap_or(self.started)
                .max(self.started);
            if record.created_at <= watermark {
                continue;
            }
            self.watermarks
                .insert(record.conversation_id.clone(), record.created_at);
            if record.deleted || is_own(&record, my_user_id) {
                continue;
            }
            if let Some(entry) = build_incoming(&record, sidebar, my_user_id) {
                incoming.push(entry);
            }
        }
        incoming
    }
}

fn is_own(record: &MessageRecord, my_user_id: Option<&str>) -> bool {
    matches!((record.sender_id.as_deref(), my_user_id), (Some(sender), Some(me)) if sender == me)
}

pub fn build_incoming(
    record: &MessageRecord,
    sidebar: &Sidebar,
    my_user_id: Option<&str>,
) -> Option<Incoming> {
    let (kind, chat_name) = chat_context(sidebar, &record.conversation_id)?;
    Some(Incoming {
        conversation_id: record.conversation_id.clone(),
        message_id: record.message_id.clone(),
        kind,
        chat_title: chat_name,
        sender_id: record.sender_id.clone(),
        sender_name: record.sender_name.clone().unwrap_or_default(),
        preview: preview_of(record),
        mentions_me: mentions_me(record, my_user_id),
        muted: false,
    })
}

fn chat_context(sidebar: &Sidebar, conversation_id: &str) -> Option<(ChatKind, String)> {
    if let Some(chat) = sidebar.chats.iter().find(|chat| chat.id == conversation_id) {
        let kind = if chat.kind.eq_ignore_ascii_case("oneOnOne") {
            ChatKind::Direct
        } else {
            ChatKind::Group {
                member_count: chat.members.len().max(2),
            }
        };
        return Some((kind, chat_title(chat)));
    }
    sidebar.teams.iter().find_map(|team| {
        team.channels
            .iter()
            .find(|channel| channel.id == conversation_id)
            .map(|channel| {
                (
                    ChatKind::Channel {
                        team: team.team.name.clone(),
                        channel: channel.name.clone(),
                    },
                    channel.name.clone(),
                )
            })
    })
}

fn mentions_me(record: &MessageRecord, my_user_id: Option<&str>) -> bool {
    let Some(me) = my_user_id else {
        return false;
    };
    teams_core::mentions(record)
        .iter()
        .any(|mention| mention.user_id.as_deref() == Some(me))
}

fn preview_of(record: &MessageRecord) -> Preview {
    if let Some(text) = teams_core::preview_text(&record.body_html) {
        return Preview::Text(text);
    }
    if !teams_core::images(record).is_empty() {
        return Preview::Image;
    }
    let file_name = teams_core::attachments(record)
        .into_iter()
        .find_map(|attachment| attachment.name);
    Preview::Text(file_name.unwrap_or_else(|| FILE_FALLBACK.to_owned()))
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use store::{ChatRecord, MemberRecord};

    use super::*;

    fn sidebar() -> Sidebar {
        Sidebar {
            chats: vec![
                ChatRecord {
                    id: "direct".into(),
                    kind: "oneOnOne".into(),
                    title: "Mara".into(),
                    ..Default::default()
                },
                ChatRecord {
                    id: "group".into(),
                    kind: "group".into(),
                    title: "Team".into(),
                    members: vec![MemberRecord::default(); 5],
                    ..Default::default()
                },
            ],
            teams: Vec::new(),
        }
    }

    fn record(conversation: &str, sender: &str, at: DateTime<Utc>, html: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: conversation.into(),
            message_id: format!("{conversation}-{}", at.timestamp_millis()),
            reply_to_id: None,
            sender_id: Some(sender.into()),
            sender_name: Some("Sender".into()),
            sender_application_id: None,
            created_at: at,
            edited_at: None,
            deleted: false,
            body_html: html.into(),
            attachments_json: "[]".into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
        }
    }

    #[test]
    fn history_before_start_is_ignored() {
        let start = Utc::now();
        let mut tracker = IncomingTracker::new(start);
        let old = record("direct", "u1", start - Duration::minutes(5), "alt");
        let new = record("direct", "u1", start + Duration::seconds(5), "neu");
        let found = tracker.fresh(vec![old, new], &sidebar(), Some("me"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].preview, Preview::Text("neu".into()));
    }

    #[test]
    fn same_message_is_not_reported_twice() {
        let start = Utc::now();
        let mut tracker = IncomingTracker::new(start);
        let message = record("direct", "u1", start + Duration::seconds(5), "neu");
        assert_eq!(tracker.fresh(vec![message.clone()], &sidebar(), None).len(), 1);
        assert!(tracker.fresh(vec![message], &sidebar(), None).is_empty());
    }

    #[test]
    fn own_messages_from_other_devices_are_skipped() {
        let start = Utc::now();
        let mut tracker = IncomingTracker::new(start);
        let own = record("direct", "me", start + Duration::seconds(5), "ich");
        assert!(tracker.fresh(vec![own], &sidebar(), Some("me")).is_empty());
    }

    #[test]
    fn detects_mention_of_me_and_group_size() {
        let start = Utc::now();
        let mut message = record("group", "u1", start + Duration::seconds(5), "<at>Me</at> hi");
        message.mentions_json = r#"[{"user_id":"me","name":"Me"}]"#.into();
        let found = build_incoming(&message, &sidebar(), Some("me")).unwrap();
        assert!(found.mentions_me);
        assert_eq!(found.kind, ChatKind::Group { member_count: 5 });
        assert!(!build_incoming(&message, &sidebar(), Some("other")).unwrap().mentions_me);
    }

    #[test]
    fn unknown_conversation_yields_nothing() {
        let message = record("nowhere", "u1", Utc::now(), "x");
        assert!(build_incoming(&message, &sidebar(), None).is_none());
    }

    #[test]
    fn empty_body_without_image_falls_back_to_file_label() {
        let message = record("direct", "u1", Utc::now(), "");
        let found = build_incoming(&message, &sidebar(), None).unwrap();
        assert_eq!(found.preview, Preview::Text(FILE_FALLBACK.into()));
    }
}
