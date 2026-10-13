use agents::{AgentAction, MessageContext, ThreadMessage, build_prompt};
use chrono::Local;
use store::MessageRecord;

use crate::app_state::{AppState, Selection, selection_title};
use crate::notify::selection_for;
use crate::people::local_names;
use crate::rows::message_text;

const THREAD_LIMIT: usize = 20;
const UNKNOWN_SENDER: &str = "Unknown";
const SESSION_PLACEHOLDER: &str = "(session id)";
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M";

fn earlier_records(
    state: &AppState,
    selection: Option<&Selection>,
    record: &MessageRecord,
) -> Vec<MessageRecord> {
    let conversation_id = record.conversation_id.as_str();
    let mut earlier = match selection {
        Some(Selection::Channel(_)) => {
            let root_id = record
                .reply_to_id
                .clone()
                .unwrap_or_else(|| record.message_id.clone());
            let mut thread = state
                .store
                .thread_messages(conversation_id, &root_id)
                .unwrap_or_default();
            thread.retain(|entry| entry.created_at < record.created_at);
            thread
        }
        _ => state
            .store
            .messages(conversation_id, Some(record.created_at), THREAD_LIMIT)
            .unwrap_or_default(),
    };
    earlier.retain(|entry| !entry.deleted);
    let skipped = earlier.len().saturating_sub(THREAD_LIMIT);
    earlier.split_off(skipped)
}

pub fn message_line(state: &AppState, conversation_id: &str, message_id: &str) -> Option<String> {
    let record = state
        .store
        .messages_by_id(conversation_id, &[message_id.to_owned()])
        .ok()?
        .remove(message_id)?;
    message_text(&record)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

pub fn build(state: &AppState, conversation_id: &str, message_id: &str) -> Option<MessageContext> {
    let record = state
        .store
        .messages_by_id(conversation_id, &[message_id.to_owned()])
        .ok()?
        .remove(message_id)
        .filter(|record| !record.deleted)?;
    let names = local_names(state);
    let sender_of = |entry: &MessageRecord| {
        entry
            .sender_name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                entry
                    .sender_id
                    .as_ref()
                    .and_then(|user_id| names.get(user_id).cloned())
            })
            .unwrap_or_else(|| UNKNOWN_SENDER.to_owned())
    };
    let selection = selection_for(&state.sidebar, conversation_id);
    let chat = selection
        .as_ref()
        .map(|selection| selection_title(&state.sidebar, selection))
        .unwrap_or_else(|| "Chat".to_owned());
    let thread = earlier_records(state, selection.as_ref(), &record)
        .iter()
        .map(|entry| ThreadMessage {
            sender: sender_of(entry),
            text: message_text(entry),
            time: entry
                .created_at
                .with_timezone(&Local)
                .format(TIME_FORMAT)
                .to_string(),
        })
        .collect();
    Some(MessageContext {
        sender: sender_of(&record),
        chat,
        conversation_id: conversation_id.to_owned(),
        message_id: message_id.to_owned(),
        message: message_text(&record),
        thread,
    })
}

pub fn preview_prompt(
    state: &AppState,
    action: &AgentAction,
    conversation_id: &str,
    message_id: &str,
    extra: &str,
) -> Option<String> {
    let context = build(state, conversation_id, message_id)?;
    Some(build_prompt(
        action,
        &context,
        extra,
        SESSION_PLACEHOLDER,
        &action.agent_id,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use store::{ChatRecord, Store};

    use super::*;
    use crate::app_state::Mode;

    fn message(id: &str, reply_to: Option<&str>, minute: u32, sender: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: "19:chat".to_owned(),
            message_id: id.to_owned(),
            reply_to_id: reply_to.map(str::to_owned),
            sender_name: Some(sender.to_owned()),
            created_at: Utc.with_ymd_and_hms(2026, 10, 1, 9, minute, 0).unwrap(),
            body_html: format!("<p>text {id}</p>"),
            attachments_json: "[]".to_owned(),
            reactions_json: "[]".to_owned(),
            mentions_json: "[]".to_owned(),
            links_json: "[]".to_owned(),
            ..MessageRecord::default()
        }
    }

    fn state_with(records: &[MessageRecord]) -> AppState {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store.upsert_messages(records).unwrap();
        let mut state = AppState::new(store, Mode::default());
        state.sidebar.chats.push(ChatRecord {
            id: "19:chat".to_owned(),
            kind: "chat".to_owned(),
            title: "Ops".to_owned(),
            member_summary: String::new(),
            last_message_at: None,
            last_read_at: None,
            unread: false,
            muted: false,
            members: Vec::new(),
            last_message_preview: None,
            last_message_sender_id: None,
            last_message_sender_name: None,
            last_message_deleted: false,
            last_event_system: false,
        });
        state
    }

    #[test]
    fn chat_context_takes_up_to_twenty_earlier_messages_oldest_first() {
        let records: Vec<MessageRecord> = (0..30)
            .map(|minute| message(&format!("m{minute}"), None, minute, "Ada"))
            .collect();
        let state = state_with(&records);
        let context = build(&state, "19:chat", "m25").unwrap();
        assert_eq!(context.chat, "Ops");
        assert_eq!(context.sender, "Ada");
        assert_eq!(context.message, "text m25");
        assert_eq!(context.thread.len(), 20);
        assert_eq!(context.thread[0].text, "text m5");
        assert_eq!(context.thread[19].text, "text m24");
    }

    #[test]
    fn deleted_and_missing_messages_give_no_context() {
        let mut deleted = message("m1", None, 1, "Ada");
        deleted.deleted = true;
        let state = state_with(&[deleted]);
        assert!(build(&state, "19:chat", "m1").is_none());
        assert!(build(&state, "19:chat", "nope").is_none());
    }

    #[test]
    fn channel_context_follows_the_thread_root() {
        let records = [
            message("root", None, 1, "Ada"),
            message("other", None, 2, "Bo"),
            message("reply-1", Some("root"), 3, "Cy"),
            message("reply-2", Some("root"), 4, "Di"),
        ];
        let mut state = state_with(&records);
        state.sidebar.chats.clear();
        state.sidebar.teams.push(store::SidebarTeam {
            team: store::TeamRecord {
                id: "team".to_owned(),
                name: "Platform".to_owned(),
            },
            channels: vec![store::ChannelRecord {
                id: "19:chat".to_owned(),
                team_id: "team".to_owned(),
                name: "Releases".to_owned(),
                membership_type: None,
                last_message_at: None,
                unread: false,
            }],
            hidden: false,
            hidden_channel_ids: Vec::new(),
            notifications: Default::default(),
        });
        let context = build(&state, "19:chat", "reply-2").unwrap();
        let texts: Vec<&str> = context
            .thread
            .iter()
            .map(|entry| entry.text.as_str())
            .collect();
        assert_eq!(texts, ["text root", "text reply-1"]);
    }

    #[test]
    fn preview_renders_the_template_with_a_session_placeholder() {
        let state = state_with(&[message("m1", None, 1, "Ada")]);
        let action = AgentAction::answer_this("claude");
        let prompt = preview_prompt(&state, &action, "19:chat", "m1", "").unwrap();
        assert!(prompt.starts_with("Ada asked me in the Teams chat \"Ops\""));
        assert!(prompt.contains("session_id \"(session id)\""));
    }
}
