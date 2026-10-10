use chatsvc::{CallLogEntry, EventKind, ForwardResult, MessageEvent, PinnedMessage, SavedMessage};

use crate::engine::{Conversation, SyncEngine};
use crate::error::{Error, Result};
use crate::events::CoreEvent;
use crate::markdown::markdown_to_html;
use crate::message_link::{ChannelLinkInput, channel_message_link, chat_message_link};
use crate::remote::Remote;
use crate::scheduled::NOTES_CHAT_ID;

impl<R: Remote> SyncEngine<R> {
    /// `message_ids` holds 1 to 5 ids, oldest first.
    pub async fn forward_messages(
        &self,
        source_conversation_id: &str,
        target_conversation_id: &str,
        message_ids: &[String],
        comment_markdown: &str,
    ) -> Result<ForwardResult> {
        let comment_html = if comment_markdown.is_empty() {
            String::new()
        } else {
            markdown_to_html(comment_markdown)
        };
        let result = self
            .remote
            .forward_messages(
                source_conversation_id,
                target_conversation_id,
                message_ids,
                &comment_html,
            )
            .await?;
        let refreshed = [result.thread_id.as_str(), target_conversation_id]
            .into_iter()
            .find(|conversation_id| self.resolve(conversation_id).is_ok());
        if let Some(conversation_id) = refreshed
            && let Err(error) = self.fetch_newer(conversation_id).await
        {
            self.report(format!("cannot refresh a forward target: {error}"));
        }
        Ok(result)
    }

    /// `root_id` is the channel root post id, `None` in chats.
    pub async fn set_saved(
        &self,
        conversation_id: &str,
        message_id: &str,
        root_id: Option<&str>,
        saved: bool,
    ) -> Result<()> {
        self.remote
            .set_saved(
                conversation_id,
                root_id.unwrap_or(message_id),
                message_id,
                saved,
            )
            .await
    }

    pub async fn list_saved(&self) -> Result<Vec<SavedMessage>> {
        self.remote.list_saved().await
    }

    pub async fn list_call_logs(&self) -> Result<Vec<CallLogEntry>> {
        self.remote.list_call_logs().await
    }

    pub async fn chat_pins(&self, chat_id: &str) -> Result<Vec<PinnedMessage>> {
        self.ensure_chat(chat_id)?;
        self.remote.chat_pins(chat_id).await
    }

    pub async fn pin_message(&self, chat_id: &str, message_id: &str) -> Result<()> {
        self.ensure_chat(chat_id)?;
        self.remote.pin_message(chat_id, message_id).await?;
        self.announce_pins(chat_id);
        Ok(())
    }

    pub async fn unpin_message(
        &self,
        chat_id: &str,
        message_id: &str,
        parent_id: Option<&str>,
    ) -> Result<()> {
        self.ensure_chat(chat_id)?;
        self.remote
            .unpin_message(chat_id, message_id, parent_id)
            .await?;
        self.announce_pins(chat_id);
        Ok(())
    }

    pub async fn message_link(&self, conversation_id: &str, message_id: &str) -> Result<String> {
        let Conversation::Channel { team_id } = self.resolve(conversation_id)? else {
            let notes_oid = if conversation_id == NOTES_CHAT_ID {
                Some(self.my_user_id().await?)
            } else {
                None
            };
            return Ok(chat_message_link(
                conversation_id,
                message_id,
                notes_oid.as_deref(),
            ));
        };
        let channel = self
            .store
            .channel(conversation_id)?
            .ok_or_else(|| Error::UnknownConversation(conversation_id.to_owned()))?;
        let team = self
            .store
            .team(&team_id)?
            .ok_or_else(|| Error::UnknownConversation(team_id.clone()))?;
        let record = self
            .store
            .messages_by_id(conversation_id, &[message_id.to_owned()])?
            .remove(message_id)
            .ok_or(Error::Unsupported("a link to an uncached message"))?;
        let any_chat_id = self
            .store
            .sidebar()?
            .chats
            .into_iter()
            .next()
            .map(|chat| chat.id)
            .ok_or(Error::Unsupported("a channel link without a known chat"))?;
        let tenant_id = self.my_tenant_id(&any_chat_id).await?;
        Ok(channel_message_link(&ChannelLinkInput {
            tenant_id,
            group_id: team_id,
            channel_id: conversation_id.to_owned(),
            team_name: team.name,
            channel_name: channel.name,
            root_id: record
                .reply_to_id
                .clone()
                .unwrap_or_else(|| record.message_id.clone()),
            message_id: record.message_id,
            created_ms: record.created_at.timestamp_millis(),
        }))
    }

    pub fn handle_pin_event(&self, event: &MessageEvent) {
        if event.kind != EventKind::PinsChanged {
            return;
        }
        if let Some(conversation_id) = event.conversation_id.as_deref() {
            self.announce_pins(conversation_id);
        }
    }

    fn ensure_chat(&self, conversation_id: &str) -> Result<()> {
        match self.resolve(conversation_id)? {
            Conversation::Chat => Ok(()),
            Conversation::Channel { .. } => Err(Error::Unsupported("pinned messages in a channel")),
        }
    }

    fn announce_pins(&self, conversation_id: &str) {
        let _ = self.events.send(CoreEvent::PinsChanged {
            conversation_id: conversation_id.to_owned(),
        });
    }
}
