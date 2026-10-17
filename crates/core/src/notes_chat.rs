use chatsvc::ConversationState;
use chrono::DateTime;
use store::{ChatRecord, MemberRecord};

use crate::engine::SyncEngine;
use crate::remote::Remote;
use crate::scheduled::NOTES_CHAT_ID;

const ONE_ON_ONE_KIND: &str = "oneOnOne";

pub fn is_notes_chat(chat_id: &str) -> bool {
    chat_id == NOTES_CHAT_ID
}

pub fn notes_chat_title(display_name: &str) -> String {
    format!("{display_name} (You)")
}

pub(crate) fn notes_chat_record(
    state: &ConversationState,
    my_user_id: &str,
    display_name: &str,
    stored: Option<&ChatRecord>,
) -> ChatRecord {
    let last_message_at = state.read.last_message_at();
    let last_read_at = (state.read.consumed_until_ms > 0)
        .then(|| DateTime::from_timestamp_millis(state.read.consumed_until_ms))
        .flatten();
    let mut record = ChatRecord {
        id: NOTES_CHAT_ID.to_owned(),
        kind: ONE_ON_ONE_KIND.to_owned(),
        title: notes_chat_title(display_name),
        member_summary: String::new(),
        last_message_at,
        last_read_at,
        unread: state.read.is_unread(my_user_id),
        muted: false,
        members: vec![MemberRecord {
            user_id: Some(my_user_id.to_owned()),
            display_name: display_name.to_owned(),
        }],
        ..ChatRecord::default()
    };
    if let Some(stored) = stored.filter(|stored| stored.last_message_at == last_message_at) {
        record.last_message_preview = stored.last_message_preview.clone();
        record.last_message_sender_id = stored.last_message_sender_id.clone();
        record.last_message_sender_name = stored.last_message_sender_name.clone();
        record.last_message_deleted = stored.last_message_deleted;
    }
    record
}

fn needs_messages(record: &ChatRecord) -> bool {
    record.last_message_at.is_some()
        && record.last_message_preview.is_none()
        && !record.last_message_deleted
}

impl<R: Remote> SyncEngine<R> {
    /// Graph `/me/chats` leaves the notes chat out; chatsvc lists it like Teams reads it.
    pub(crate) async fn refresh_notes_chat(
        &self,
        states: Option<&[ConversationState]>,
        my_user_id: &str,
    ) -> Option<String> {
        let stored = match self.store.chat(NOTES_CHAT_ID) {
            Ok(stored) => stored,
            Err(error) => {
                self.report(format!("cannot read the notes chat: {error}"));
                return None;
            }
        };
        let Some(states) = states else {
            return stored.map(|stored| stored.id);
        };
        let state = states
            .iter()
            .find(|state| is_notes_chat(&state.id) && state.hidden != Some(true));
        let Some(state) = state else {
            if stored.is_some()
                && let Err(error) = self.store.remove_chats(&[NOTES_CHAT_ID.to_owned()])
            {
                self.report(format!("cannot remove the notes chat: {error}"));
            }
            return None;
        };
        let display_name = self.me().map(|me| me.display_name).unwrap_or_default();
        let record = notes_chat_record(state, my_user_id, &display_name, stored.as_ref());
        if let Err(error) = self.store.upsert_chats(std::slice::from_ref(&record)) {
            self.report(format!("cannot store the notes chat: {error}"));
            return None;
        }
        if needs_messages(&record)
            && let Err(error) = self.fetch_newer(NOTES_CHAT_ID).await
        {
            self.report(format!("cannot read the notes chat: {error}"));
        }
        Some(record.id)
    }
}

#[cfg(test)]
mod tests {
    use chatsvc::ReadHorizon;

    use super::*;

    fn state(last_message_ms: i64, consumed_until_ms: i64, from: &str) -> ConversationState {
        ConversationState {
            id: NOTES_CHAT_ID.to_owned(),
            muted: false,
            hidden: None,
            read: ReadHorizon {
                consumed_until_ms,
                last_message_ms,
                last_message_from: Some(format!("8:orgid:{from}")),
                ..ReadHorizon::default()
            },
            history_cleared_ms: None,
        }
    }

    #[test]
    fn the_record_is_a_one_on_one_with_only_me_titled_like_teams() {
        let record = notes_chat_record(&state(9, 5, "me"), "me", "Ada Example", None);
        assert_eq!(record.title, "Ada Example (You)");
        assert_eq!(record.kind, "oneOnOne");
        assert_eq!(record.members.len(), 1);
        assert_eq!(record.members[0].user_id.as_deref(), Some("me"));
        assert_eq!(record.last_message_at, DateTime::from_timestamp_millis(9));
        assert_eq!(record.last_read_at, DateTime::from_timestamp_millis(5));
        assert!(!record.unread);
        assert!(needs_messages(&record));
    }

    #[test]
    fn the_stored_preview_stays_while_the_last_message_is_the_same() {
        let stored = ChatRecord {
            last_message_at: DateTime::from_timestamp_millis(9),
            last_message_preview: Some("plan".to_owned()),
            last_message_sender_id: Some("me".to_owned()),
            ..ChatRecord::default()
        };
        let same = notes_chat_record(&state(9, 9, "me"), "me", "Ada", Some(&stored));
        assert_eq!(same.last_message_preview.as_deref(), Some("plan"));
        assert!(!needs_messages(&same));
        let newer = notes_chat_record(&state(12, 9, "me"), "me", "Ada", Some(&stored));
        assert_eq!(newer.last_message_preview, None);
        assert!(needs_messages(&newer));
    }

    #[test]
    fn an_empty_notes_chat_needs_no_message_read() {
        let record = notes_chat_record(&state(0, 0, "me"), "me", "Ada", None);
        assert_eq!(record.last_message_at, None);
        assert!(!needs_messages(&record));
    }
}
