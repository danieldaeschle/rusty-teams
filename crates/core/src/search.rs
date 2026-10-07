use store::{ConversationHit, SearchHit};

use crate::engine::{META_USER_ID, SyncEngine};
use crate::error::Result;
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    /// Newest first, prefix match on every word, `from:name` filters the sender. Snippets mark matches with `HIGHLIGHT_START` / `HIGHLIGHT_END`.
    pub fn search_messages(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        Ok(self.store.search_messages(query, None, limit)?)
    }

    pub fn search_messages_in(
        &self,
        conversation_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        Ok(self
            .store
            .search_messages(query, Some(conversation_id), limit)?)
    }

    pub fn search_conversations(&self, query: &str, limit: usize) -> Result<Vec<ConversationHit>> {
        Ok(self.store.search_conversations(query, limit)?)
    }

    /// First cached message from others after the chat's last read time, `None` for a read chat. Call before `mark_read`.
    pub fn first_unread_message_id(&self, conversation_id: &str) -> Option<String> {
        let chat = self
            .store
            .chat(conversation_id)
            .ok()
            .flatten()
            .filter(|chat| chat.unread)?;
        let my_user_id = self
            .store
            .meta(META_USER_ID)
            .ok()
            .flatten()
            .unwrap_or_default();
        self.store
            .first_message_after_from_others(conversation_id, chat.last_read_at, &my_user_id)
            .ok()
            .flatten()
    }
}
