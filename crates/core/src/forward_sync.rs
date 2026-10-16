use crate::engine::{Conversation, SyncEngine};
use crate::forwarded::{FORWARDED_CONTENT_TYPE, lacks_forward_content};
use crate::mapping::message_record;
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    /// Once per chat and session, for cached forwards stored before their original message was kept.
    pub(crate) async fn heal_forwards(&self, conversation_id: &str) -> bool {
        let first = self
            .forwards_healed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(conversation_id.to_owned());
        if !first || !matches!(self.resolve_stored(conversation_id), Ok(Conversation::Chat)) {
            return false;
        }
        let Ok(cached) = self
            .store
            .messages_with_attachment_type(conversation_id, FORWARDED_CONTENT_TYPE)
        else {
            return false;
        };
        let stale: Vec<String> = cached
            .iter()
            .filter(|record| lacks_forward_content(record))
            .map(|record| record.message_id.clone())
            .collect();
        if stale.is_empty() {
            return false;
        }
        let Ok(answers) = self.remote.chat_messages_by_id(conversation_id, &stale).await else {
            return false;
        };
        let mut records: Vec<_> = answers
            .iter()
            .flatten()
            .filter_map(|message| message_record(conversation_id, message))
            .collect();
        self.apply_chat_emotions(conversation_id, &mut records).await;
        self.ingest(conversation_id, records)
            .is_ok_and(|delta| !delta.is_empty())
    }
}
