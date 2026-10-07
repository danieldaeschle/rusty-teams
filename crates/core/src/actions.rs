use chrono::Utc;
use graph::{MessageTarget, OutgoingMention};
use store::MessageRecord;

use crate::engine::{Conversation, META_USER_ID, SyncEngine};
use crate::error::{Error, Result};
use crate::mapping::{chat_record, message_record};
use crate::markdown::markdown_to_html;
use crate::mentions::{MentionInput, apply_mentions, ensure_allowed_in_chat};
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    pub async fn send_message(
        &self,
        conversation_id: &str,
        markdown: &str,
        thread_root_id: Option<&str>,
    ) -> Result<MessageRecord> {
        self.send_message_with_mentions(conversation_id, markdown, thread_root_id, &[])
            .await
    }

    pub async fn send_message_with_mentions(
        &self,
        conversation_id: &str,
        markdown: &str,
        thread_root_id: Option<&str>,
        mentions: &[MentionInput],
    ) -> Result<MessageRecord> {
        let conversation = self.resolve(conversation_id)?;
        let (html, mentions) = outgoing(&conversation, markdown, mentions)?;
        let sent = match (conversation, thread_root_id) {
            (Conversation::Chat, None) => {
                self.remote
                    .send_chat_message(conversation_id, &html, &mentions)
                    .await?
            }
            (Conversation::Channel { team_id }, Some(root_id)) => {
                self.remote
                    .reply_to_channel_message(&team_id, conversation_id, root_id, &html, &mentions)
                    .await?
            }
            (Conversation::Chat, Some(_)) => {
                return Err(Error::Unsupported("a thread reply in a chat"));
            }
            (Conversation::Channel { .. }, None) => {
                return Err(Error::Unsupported(
                    "a channel send without a thread root; use post_to_channel",
                ));
            }
        };
        self.cache_sent(conversation_id, &sent)
    }

    /// Chat: quoted reply (Graph beta `replyWithQuote`). Channel: reply in the thread of `message_id`.
    pub async fn reply_to(
        &self,
        conversation_id: &str,
        message_id: &str,
        markdown: &str,
    ) -> Result<MessageRecord> {
        self.reply_to_with_mentions(conversation_id, message_id, markdown, &[])
            .await
    }

    pub async fn reply_to_with_mentions(
        &self,
        conversation_id: &str,
        message_id: &str,
        markdown: &str,
        mentions: &[MentionInput],
    ) -> Result<MessageRecord> {
        let conversation = self.resolve(conversation_id)?;
        let (html, mentions) = outgoing(&conversation, markdown, mentions)?;
        let sent = match conversation {
            Conversation::Chat => {
                self.remote
                    .reply_with_quote(conversation_id, message_id, &html, &mentions)
                    .await?
            }
            Conversation::Channel { team_id } => {
                let root_id = self
                    .store
                    .messages_by_id(conversation_id, &[message_id.to_owned()])?
                    .remove(message_id)
                    .and_then(|record| record.reply_to_id)
                    .unwrap_or_else(|| message_id.to_owned());
                self.remote
                    .reply_to_channel_message(&team_id, conversation_id, &root_id, &html, &mentions)
                    .await?
            }
        };
        self.cache_sent(conversation_id, &sent)
    }

    /// Own and not deleted. Chat messages, channel posts and channel replies alike.
    pub fn can_edit(&self, record: &MessageRecord) -> bool {
        self.is_own(record)
    }

    pub fn can_delete(&self, record: &MessageRecord) -> bool {
        self.is_own(record)
    }

    fn is_own(&self, record: &MessageRecord) -> bool {
        let my_user_id = self
            .store
            .meta(META_USER_ID)
            .ok()
            .flatten()
            .unwrap_or_default();
        crate::stored::can_edit(record, &my_user_id)
    }

    pub async fn post_to_channel(
        &self,
        channel_id: &str,
        markdown: &str,
        subject: Option<&str>,
    ) -> Result<MessageRecord> {
        self.post_to_channel_with_mentions(channel_id, markdown, subject, &[])
            .await
    }

    pub async fn post_to_channel_with_mentions(
        &self,
        channel_id: &str,
        markdown: &str,
        subject: Option<&str>,
        mentions: &[MentionInput],
    ) -> Result<MessageRecord> {
        let conversation = self.resolve(channel_id)?;
        let Conversation::Channel { team_id } = &conversation else {
            return Err(Error::Unsupported("posting a root message to a chat"));
        };
        let (html, mentions) = outgoing(&conversation, markdown, mentions)?;
        let sent = self
            .remote
            .send_channel_message(team_id, channel_id, &html, subject, &mentions)
            .await?;
        self.cache_sent(channel_id, &sent)
    }

    pub async fn refresh_message(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageRecord>> {
        let fetched = match self.resolve(conversation_id)? {
            Conversation::Chat => self.remote.chat_message(conversation_id, message_id).await,
            Conversation::Channel { team_id } => {
                self.fetch_channel_message(&team_id, conversation_id, message_id)
                    .await
            }
        };
        let message = match fetched {
            Ok(message) => message,
            Err(error) => {
                self.report(format!("cannot refresh a message: {error}"));
                return Err(error);
            }
        };
        let Some(record) = message_record(conversation_id, &message) else {
            return Ok(None);
        };
        let delta = self.ingest(conversation_id, vec![record.clone()])?;
        self.announce(conversation_id, !delta.is_empty());
        Ok(Some(record))
    }

    pub async fn mark_read(&self, conversation_id: &str) -> Result<()> {
        if !matches!(self.resolve(conversation_id)?, Conversation::Chat) {
            return Err(Error::Unsupported("marking a channel as read"));
        }
        let user_id = self.my_user_id().await?;
        let tenant_id = self.my_tenant_id(conversation_id).await?;
        self.remote
            .mark_chat_read(conversation_id, &user_id, &tenant_id)
            .await?;
        self.store.mark_chat_read(conversation_id, Utc::now())?;
        let _ = self.events.send(crate::events::CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn set_reaction(
        &self,
        conversation_id: &str,
        message_id: &str,
        reaction_type: &str,
    ) -> Result<()> {
        let target = self.message_target(conversation_id, message_id)?;
        self.remote.set_reaction(&target, reaction_type).await?;
        self.refresh_message(conversation_id, message_id).await?;
        Ok(())
    }

    pub async fn unset_reaction(
        &self,
        conversation_id: &str,
        message_id: &str,
        reaction_type: &str,
    ) -> Result<()> {
        let target = self.message_target(conversation_id, message_id)?;
        self.remote.unset_reaction(&target, reaction_type).await?;
        self.refresh_message(conversation_id, message_id).await?;
        Ok(())
    }

    pub async fn edit_message(
        &self,
        conversation_id: &str,
        message_id: &str,
        markdown: &str,
    ) -> Result<()> {
        self.edit_message_with_mentions(conversation_id, message_id, markdown, &[])
            .await
    }

    pub async fn edit_message_with_mentions(
        &self,
        conversation_id: &str,
        message_id: &str,
        markdown: &str,
        mentions: &[MentionInput],
    ) -> Result<()> {
        let conversation = self.resolve(conversation_id)?;
        let (html, mentions) = outgoing(&conversation, markdown, mentions)?;
        let target = self.message_target(conversation_id, message_id)?;
        self.remote.edit_message(&target, &html, &mentions).await?;
        self.refresh_message(conversation_id, message_id).await?;
        Ok(())
    }

    pub async fn soft_delete_message(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        let target = self.message_target(conversation_id, message_id)?;
        let user_id = self.my_user_id().await?;
        self.remote.soft_delete_message(&user_id, &target).await?;
        self.refresh_message(conversation_id, message_id).await?;
        Ok(())
    }

    fn message_target(&self, conversation_id: &str, message_id: &str) -> Result<MessageTarget> {
        Ok(match self.resolve(conversation_id)? {
            Conversation::Chat => MessageTarget::chat(conversation_id, message_id),
            Conversation::Channel { team_id } => {
                let root_id = self
                    .store
                    .messages_by_id(conversation_id, &[message_id.to_owned()])?
                    .remove(message_id)
                    .and_then(|record| record.reply_to_id);
                MessageTarget::channel(&team_id, conversation_id, message_id, root_id.as_deref())
            }
        })
    }

    pub async fn create_one_on_one(&self, user_id: &str) -> Result<String> {
        let my_user_id = self.my_user_id().await?;
        let chat = self.remote.create_one_on_one(&my_user_id, user_id).await?;
        self.store_new_chat(&chat, &my_user_id)
    }

    pub async fn create_group(&self, user_ids: &[String], topic: Option<&str>) -> Result<String> {
        let my_user_id = self.my_user_id().await?;
        let chat = self
            .remote
            .create_group(&my_user_id, user_ids, topic)
            .await?;
        self.store_new_chat(&chat, &my_user_id)
    }

    fn store_new_chat(&self, chat: &graph::Chat, my_user_id: &str) -> Result<String> {
        if let Some(record) = chat_record(chat, my_user_id) {
            self.store.upsert_chats(&[record])?;
            let _ = self.events.send(crate::events::CoreEvent::SidebarChanged);
        }
        Ok(chat.id.clone())
    }

    fn cache_sent(&self, conversation_id: &str, sent: &graph::Message) -> Result<MessageRecord> {
        let record = message_record(conversation_id, sent)
            .ok_or(Error::Unsupported("a send answer without a creation time"))?;
        self.store.upsert_messages(std::slice::from_ref(&record))?;
        self.update_chat_preview(std::slice::from_ref(&record))?;
        let mut state = self.store.sync_state(conversation_id)?.unwrap_or_default();
        if state
            .newest_seen
            .is_some_and(|newest| newest < record.created_at)
        {
            state.newest_seen = Some(record.created_at);
            self.store.set_sync_state(conversation_id, &state)?;
        }
        self.announce(conversation_id, true);
        Ok(record)
    }

    async fn fetch_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> Result<graph::Message> {
        let known = self
            .store
            .messages_by_id(channel_id, &[message_id.to_owned()])?;
        match known
            .get(message_id)
            .and_then(|record| record.reply_to_id.clone())
        {
            Some(root_id) => {
                self.remote
                    .channel_reply(team_id, channel_id, &root_id, message_id)
                    .await
            }
            None => {
                self.remote
                    .channel_message(team_id, channel_id, message_id)
                    .await
            }
        }
    }
}

fn outgoing(
    conversation: &Conversation,
    markdown: &str,
    mentions: &[MentionInput],
) -> Result<(String, Vec<OutgoingMention>)> {
    if matches!(conversation, Conversation::Chat) {
        ensure_allowed_in_chat(mentions)?;
    }
    Ok(apply_mentions(&markdown_to_html(markdown), mentions))
}
