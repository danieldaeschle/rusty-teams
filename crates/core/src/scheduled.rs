use chatsvc::{ConversationRef, ScheduledDraft};
use chrono::{DateTime, Utc};

use crate::engine::{Conversation, SyncEngine};
use crate::error::{Error, Result};
use crate::remote::Remote;

pub const NOTES_CHAT_ID: &str = "48:notes";

impl<R: Remote> SyncEngine<R> {
    pub async fn schedule_message(
        &self,
        conversation_id: &str,
        thread_root_id: Option<&str>,
        html: &str,
        send_at: DateTime<Utc>,
    ) -> Result<ScheduledDraft> {
        let inner_thread_id = self.inner_thread_id(conversation_id, thread_root_id)?;
        let display_name = self.display_name().await?;
        self.remote
            .create_scheduled(&inner_thread_id, html, send_at, &display_name)
            .await
    }

    pub async fn scheduled_messages(&self) -> Result<Vec<ScheduledDraft>> {
        let mut drafts = self.remote.scheduled_drafts().await?;
        drafts.sort_by_key(|draft| draft.send_at);
        Ok(drafts)
    }

    pub async fn update_scheduled(
        &self,
        draft: &ScheduledDraft,
        html: &str,
    ) -> Result<ScheduledDraft> {
        let display_name = self.display_name().await?;
        self.remote
            .update_scheduled(draft, html, &display_name)
            .await
    }

    pub async fn cancel_scheduled(&self, draft_id: &str) -> Result<()> {
        self.remote.cancel_scheduled(draft_id).await
    }

    fn inner_thread_id(
        &self,
        conversation_id: &str,
        thread_root_id: Option<&str>,
    ) -> Result<String> {
        if conversation_id == NOTES_CHAT_ID {
            return Err(Error::Unsupported("schedule send in the notes chat"));
        }
        let conversation = match (self.resolve(conversation_id)?, thread_root_id) {
            (Conversation::Chat, None) => ConversationRef::chat(conversation_id),
            (Conversation::Channel { .. }, None) => ConversationRef::channel_root(conversation_id),
            (Conversation::Channel { .. }, Some(root_id)) => {
                ConversationRef::channel_reply(conversation_id, root_id)
            }
            (Conversation::Chat, Some(_)) => {
                return Err(Error::Unsupported("a thread reply in a chat"));
            }
        };
        Ok(conversation.conversation_id())
    }

    async fn display_name(&self) -> Result<String> {
        self.ensure_display_name().await?;
        Ok(self.me().map(|me| me.display_name).unwrap_or_default())
    }
}
