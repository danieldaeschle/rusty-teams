use chatsvc::ConversationRef;
use chrono::{Duration, Utc};
use graph::{
    FileReference, MessageExtras, MessageTarget, OutgoingMention, UploadDestination, UploadedFile,
};
use store::MessageRecord;

use crate::engine::{Conversation, META_USER_ID, SyncEngine};
use crate::error::{Error, Result};
use crate::links::LinkPreview;
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
        self.send_message_with_extras(
            conversation_id,
            &markdown_to_html(markdown),
            thread_root_id,
            mentions,
            &MessageExtras::default(),
        )
        .await
    }

    /// `html` is the message body; mentions are matched as `@name` in it.
    pub async fn send_message_with_extras(
        &self,
        conversation_id: &str,
        html: &str,
        thread_root_id: Option<&str>,
        mentions: &[MentionInput],
        extras: &MessageExtras,
    ) -> Result<MessageRecord> {
        let conversation = self.resolve(conversation_id)?;
        let (html, mentions) = outgoing(&conversation, html, mentions)?;
        let sent = match (conversation, thread_root_id) {
            (Conversation::Chat, None) => {
                self.remote
                    .send_chat_message(conversation_id, &html, &mentions, extras)
                    .await?
            }
            (Conversation::Channel { team_id }, Some(root_id)) => {
                self.remote
                    .reply_to_channel_message(
                        &team_id,
                        conversation_id,
                        root_id,
                        &html,
                        &mentions,
                        extras,
                    )
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
        self.reply_to_with_extras(
            conversation_id,
            message_id,
            &markdown_to_html(markdown),
            mentions,
            &MessageExtras::default(),
        )
        .await
    }

    pub async fn reply_to_with_extras(
        &self,
        conversation_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[MentionInput],
        extras: &MessageExtras,
    ) -> Result<MessageRecord> {
        let conversation = self.resolve(conversation_id)?;
        let (html, mentions) = outgoing(&conversation, html, mentions)?;
        let sent = match conversation {
            Conversation::Chat => {
                self.remote
                    .reply_with_quote(conversation_id, message_id, &html, &mentions, extras)
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
                    .reply_to_channel_message(
                        &team_id,
                        conversation_id,
                        &root_id,
                        &html,
                        &mentions,
                        extras,
                    )
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
        self.post_to_channel_with_extras(
            channel_id,
            &markdown_to_html(markdown),
            subject,
            mentions,
            &MessageExtras::default(),
        )
        .await
    }

    pub async fn post_to_channel_with_extras(
        &self,
        channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[MentionInput],
        extras: &MessageExtras,
    ) -> Result<MessageRecord> {
        let conversation = self.resolve(channel_id)?;
        let Conversation::Channel { team_id } = &conversation else {
            return Err(Error::Unsupported("posting a root message to a chat"));
        };
        let (html, mentions) = outgoing(&conversation, html, mentions)?;
        let sent = self
            .remote
            .send_channel_message(team_id, channel_id, &html, subject, &mentions, extras)
            .await?;
        self.cache_sent(channel_id, &sent)
    }

    /// Upload then share; the app uses the two steps apart so a failed share does not upload again.
    pub async fn upload_attachment(
        &self,
        conversation_id: &str,
        file_name: &str,
        bytes: &[u8],
        progress: impl Fn(u8) + Send + Sync,
    ) -> Result<FileReference> {
        let uploaded = self
            .upload_attachment_file(conversation_id, file_name, bytes, progress)
            .await?;
        self.share_attachment(conversation_id, &uploaded).await?;
        uploaded.reference().ok_or_else(missing_attachment_id)
    }

    /// Chat files go to the sender's "Microsoft Teams Chat Files", channel files to the channel's folder.
    pub async fn upload_attachment_file(
        &self,
        conversation_id: &str,
        file_name: &str,
        bytes: &[u8],
        progress: impl Fn(u8) + Send + Sync,
    ) -> Result<UploadedFile> {
        let destination = match self.resolve(conversation_id)? {
            Conversation::Chat => UploadDestination::ChatFiles,
            Conversation::Channel { team_id } => {
                UploadDestination::Folder(self.channel_folder(&team_id, conversation_id).await?)
            }
        };
        let uploaded = self
            .remote
            .upload_file(&destination, file_name, bytes, &progress)
            .await?;
        if uploaded.reference().is_none() {
            let _ = self.remote.delete_file(&uploaded).await;
            return Err(missing_attachment_id());
        }
        Ok(uploaded)
    }

    /// Read-only for the other chat members; channels need nothing.
    pub async fn share_attachment(
        &self,
        conversation_id: &str,
        uploaded: &UploadedFile,
    ) -> Result<()> {
        if !matches!(self.resolve(conversation_id)?, Conversation::Chat) {
            return Ok(());
        }
        let other_members = self.other_member_ids(conversation_id).await?;
        self.remote.share_file(uploaded, &other_members).await
    }

    pub async fn discard_attachment(&self, uploaded: &UploadedFile) -> Result<()> {
        self.remote.delete_file(uploaded).await
    }

    async fn channel_folder(&self, team_id: &str, channel_id: &str) -> Result<graph::DriveFolder> {
        let cached = self
            .channel_folders
            .lock()
            .expect("channel folder cache poisoned")
            .get(channel_id)
            .cloned();
        if let Some(folder) = cached {
            return Ok(folder);
        }
        let folder = self
            .remote
            .channel_files_folder(team_id, channel_id)
            .await?;
        self.channel_folders
            .lock()
            .expect("channel folder cache poisoned")
            .insert(channel_id.to_owned(), folder.clone());
        Ok(folder)
    }

    async fn other_member_ids(&self, chat_id: &str) -> Result<Vec<String>> {
        let my_user_id = self.my_user_id().await?;
        let members = self
            .store
            .chat(chat_id)?
            .map(|chat| chat.members)
            .filter(|members| !members.is_empty())
            .ok_or_else(|| graph::Error::Upload("Chat members not loaded yet.".into()))?;
        Ok(members
            .into_iter()
            .filter_map(|member| member.user_id)
            .filter(|user_id| *user_id != my_user_id)
            .collect())
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
        let mut record = record;
        let delta = self.ingest(conversation_id, vec![record.clone()])?;
        let links_changed = self.attach_links(conversation_id, &mut [&mut record]).await;
        self.announce(conversation_id, !delta.is_empty() || links_changed);
        Ok(Some(record))
    }

    pub async fn attach_link_preview(
        &self,
        conversation_id: &str,
        message_id: &str,
        preview: &LinkPreview,
    ) -> Result<()> {
        let target = self.message_target(conversation_id, message_id)?;
        let links_json = preview.links_json();
        self.remote.set_message_links(&target, &links_json).await?;
        if self
            .store
            .set_message_links(conversation_id, message_id, &links_json)?
        {
            self.announce(conversation_id, true);
        }
        Ok(())
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

    pub async fn mark_unread(&self, chat_id: &str) -> Result<()> {
        let last_message_at = self
            .store
            .chat(chat_id)?
            .and_then(|chat| chat.last_message_at)
            .ok_or(Error::Unsupported(
                "marking a chat without messages as unread",
            ))?;
        let last_read_at = last_message_at - Duration::milliseconds(1);
        let user_id = self.my_user_id().await?;
        let tenant_id = self.my_tenant_id(chat_id).await?;
        self.remote
            .mark_chat_unread(chat_id, &user_id, &tenant_id, last_read_at)
            .await?;
        self.store.mark_chat_unread(chat_id, last_read_at)?;
        let _ = self.events.send(crate::events::CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn set_chat_muted(&self, chat_id: &str, muted: bool) -> Result<()> {
        self.remote.set_chat_muted(chat_id, muted).await?;
        self.store.set_chat_muted(chat_id, muted)?;
        let _ = self.events.send(crate::events::CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn hide_chat(&self, chat_id: &str) -> Result<()> {
        let user_id = self.my_user_id().await?;
        let tenant_id = self.my_tenant_id(chat_id).await?;
        self.remote.hide_chat(chat_id, &user_id, &tenant_id).await?;
        self.store.remove_chats(&[chat_id.to_owned()])?;
        let _ = self.events.send(crate::events::CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn unhide_chat(&self, chat_id: &str) -> Result<()> {
        let user_id = self.my_user_id().await?;
        let tenant_id = self.my_tenant_id(chat_id).await?;
        self.remote
            .unhide_chat(chat_id, &user_id, &tenant_id)
            .await?;
        self.refresh_sidebar_full().await?;
        Ok(())
    }

    pub async fn leave_chat(&self, chat_id: &str) -> Result<()> {
        let user_id = self.my_user_id().await?;
        self.remote.leave_chat(chat_id, &user_id).await?;
        self.store.remove_chats(&[chat_id.to_owned()])?;
        let _ = self.events.send(crate::events::CoreEvent::SidebarChanged);
        Ok(())
    }

    pub async fn send_typing(
        &self,
        conversation_id: &str,
        thread_root_id: Option<&str>,
        active: bool,
    ) -> Result<()> {
        let conversation = match (self.resolve(conversation_id)?, thread_root_id) {
            (Conversation::Chat, _) => ConversationRef::chat(conversation_id),
            (Conversation::Channel { .. }, None) => ConversationRef::channel_root(conversation_id),
            (Conversation::Channel { .. }, Some(root_id)) => {
                ConversationRef::channel_reply(conversation_id, root_id)
            }
        };
        self.remote.send_typing(&conversation, active).await
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
        self.edit_message_html(
            conversation_id,
            message_id,
            &markdown_to_html(markdown),
            mentions,
        )
        .await
    }

    pub async fn edit_message_html(
        &self,
        conversation_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[MentionInput],
    ) -> Result<()> {
        let conversation = self.resolve(conversation_id)?;
        let (mut html, mentions) = outgoing(&conversation, html, mentions)?;
        let target = self.message_target(conversation_id, message_id)?;
        let mut extras = MessageExtras::default();
        if let Some(record) = self
            .store
            .messages_by_id(conversation_id, &[message_id.to_owned()])?
            .remove(message_id)
        {
            let preserved = crate::stored::edit_preserved(&record);
            html.push_str(&preserved.images_html);
            extras.files = preserved.files;
            extras.kept = preserved.kept;
        }
        self.remote
            .edit_message(&target, &html, &mentions, &extras)
            .await?;
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
    html: &str,
    mentions: &[MentionInput],
) -> Result<(String, Vec<OutgoingMention>)> {
    if matches!(conversation, Conversation::Chat) {
        ensure_allowed_in_chat(mentions)?;
    }
    Ok(apply_mentions(html, mentions))
}

fn missing_attachment_id() -> Error {
    graph::Error::Upload("the uploaded file has no attachment id".into()).into()
}
