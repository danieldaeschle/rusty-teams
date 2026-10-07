use chatsvc::{ConversationRef, MemberHorizon, Messages, Receipts};
use chrono::{DateTime, Utc};
use graph::{
    Channel, Chat, Graph, Member, Message, MessageTarget, OutgoingMention, Photo, Presence, Team,
    User,
};

use crate::error::{Error, Result};

pub const CHATSVC_CHAT_WRITES: bool = false;

#[derive(Debug, Clone)]
pub struct RemotePage {
    pub items: Vec<Message>,
    pub next_link: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeltaPage {
    pub items: Vec<Message>,
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatsPage {
    pub items: Vec<Chat>,
    pub next_link: Option<String>,
}

#[allow(async_fn_in_trait)]
pub trait Remote {
    async fn me(&self) -> Result<User>;
    async fn chats_page(&self, top: usize) -> Result<ChatsPage>;
    async fn chats_at(&self, next_link: &str) -> Result<ChatsPage>;
    async fn chat(&self, chat_id: &str) -> Result<Chat>;
    async fn chat_members(&self, chat_id: &str) -> Result<Vec<Member>>;
    async fn joined_teams(&self) -> Result<Vec<Team>>;
    async fn channels_for_teams(&self, team_ids: &[String]) -> Result<Vec<Result<Vec<Channel>>>>;
    async fn chat_messages(
        &self,
        chat_id: &str,
        before: Option<DateTime<Utc>>,
        top: usize,
    ) -> Result<RemotePage>;
    async fn chat_message(&self, chat_id: &str, message_id: &str) -> Result<Message>;
    async fn channel_messages(
        &self,
        team_id: &str,
        channel_id: &str,
        top: usize,
    ) -> Result<RemotePage>;
    async fn channel_messages_at(&self, next_link: &str) -> Result<RemotePage>;
    async fn channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> Result<Message>;
    async fn channel_reply(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        reply_id: &str,
    ) -> Result<Message>;
    async fn channel_delta(
        &self,
        team_id: &str,
        channel_id: &str,
        modified_after: Option<DateTime<Utc>>,
    ) -> Result<DeltaPage>;
    async fn channel_delta_at(&self, link: &str) -> Result<DeltaPage>;
    async fn channel_replies(
        &self,
        team_id: &str,
        channel_id: &str,
        root_ids: &[String],
    ) -> Result<Vec<Result<Vec<Message>>>>;
    async fn send_chat_message(
        &self,
        chat_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
    ) -> Result<Message>;
    async fn send_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[OutgoingMention],
    ) -> Result<Message>;
    async fn reply_to_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
    ) -> Result<Message>;
    async fn mark_chat_read(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()>;
    async fn set_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()>;
    async fn unset_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()>;
    async fn edit_message(
        &self,
        target: &MessageTarget,
        html: &str,
        mentions: &[OutgoingMention],
    ) -> Result<()>;
    async fn soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()>;
    async fn create_one_on_one(&self, my_user_id: &str, user_id: &str) -> Result<Chat>;
    async fn create_group(
        &self,
        my_user_id: &str,
        user_ids: &[String],
        topic: Option<&str>,
    ) -> Result<Chat>;
    async fn search_people(&self, query: &str) -> Result<Vec<User>>;

    async fn reply_with_quote(
        &self,
        _chat_id: &str,
        _quoted_message_id: &str,
        _html: &str,
        _mentions: &[OutgoingMention],
    ) -> Result<Message> {
        Err(Error::Unsupported("a quoted reply"))
    }

    async fn hosted_content(&self, _url: &str) -> Result<Photo> {
        Err(Error::Unsupported("hosted content"))
    }

    async fn user_photos(&self, _user_ids: &[String]) -> Result<Vec<Result<Option<Photo>>>> {
        Err(Error::Unsupported("profile photos"))
    }

    async fn presences(&self, _user_ids: &[String]) -> Result<Vec<Presence>> {
        Err(Error::Unsupported("presence"))
    }

    async fn consumption_horizons(&self, _conversation_id: &str) -> Result<Vec<MemberHorizon>> {
        Err(Error::Unsupported("read receipts"))
    }
}

impl Remote for Graph {
    async fn me(&self) -> Result<User> {
        Ok(Graph::me(self).await?)
    }

    async fn chats_page(&self, top: usize) -> Result<ChatsPage> {
        Ok(Graph::chats_page(self, top).await?.into())
    }

    async fn chats_at(&self, next_link: &str) -> Result<ChatsPage> {
        Ok(Graph::chats_at(self, next_link).await?.into())
    }

    async fn chat(&self, chat_id: &str) -> Result<Chat> {
        Ok(Graph::chat(self, chat_id).await?)
    }

    async fn chat_members(&self, chat_id: &str) -> Result<Vec<Member>> {
        Ok(Graph::chat_members(self, chat_id).await?)
    }

    async fn joined_teams(&self) -> Result<Vec<Team>> {
        Ok(Graph::joined_teams(self).await?)
    }

    async fn channels_for_teams(&self, team_ids: &[String]) -> Result<Vec<Result<Vec<Channel>>>> {
        let listings = Graph::channels_for_teams(self, team_ids).await?;
        Ok(listings
            .into_iter()
            .map(|listing| listing.map_err(Into::into))
            .collect())
    }

    async fn chat_messages(
        &self,
        chat_id: &str,
        before: Option<DateTime<Utc>>,
        top: usize,
    ) -> Result<RemotePage> {
        Ok(Graph::chat_messages(self, chat_id, before, top)
            .await?
            .into())
    }

    async fn chat_message(&self, chat_id: &str, message_id: &str) -> Result<Message> {
        Ok(Graph::chat_message(self, chat_id, message_id).await?)
    }

    async fn channel_messages(
        &self,
        team_id: &str,
        channel_id: &str,
        top: usize,
    ) -> Result<RemotePage> {
        Ok(Graph::channel_messages(self, team_id, channel_id, top)
            .await?
            .into())
    }

    async fn channel_messages_at(&self, next_link: &str) -> Result<RemotePage> {
        Ok(Graph::channel_messages_at(self, next_link).await?.into())
    }

    async fn channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        Ok(Graph::channel_message(self, team_id, channel_id, message_id).await?)
    }

    async fn channel_reply(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        reply_id: &str,
    ) -> Result<Message> {
        Ok(Graph::channel_reply(self, team_id, channel_id, message_id, reply_id).await?)
    }

    async fn channel_delta(
        &self,
        team_id: &str,
        channel_id: &str,
        modified_after: Option<DateTime<Utc>>,
    ) -> Result<DeltaPage> {
        Ok(
            Graph::channel_delta(self, team_id, channel_id, modified_after)
                .await?
                .into(),
        )
    }

    async fn channel_delta_at(&self, link: &str) -> Result<DeltaPage> {
        Ok(Graph::channel_delta_at(self, link).await?.into())
    }

    async fn channel_replies(
        &self,
        team_id: &str,
        channel_id: &str,
        root_ids: &[String],
    ) -> Result<Vec<Result<Vec<Message>>>> {
        let listings = Graph::channel_replies(self, team_id, channel_id, root_ids).await?;
        Ok(listings
            .into_iter()
            .map(|listing| listing.map_err(Into::into))
            .collect())
    }

    async fn send_chat_message(
        &self,
        chat_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
    ) -> Result<Message> {
        Ok(Graph::send_chat_message(self, chat_id, html, mentions).await?)
    }

    async fn send_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[OutgoingMention],
    ) -> Result<Message> {
        Ok(Graph::send_channel_message(self, team_id, channel_id, html, subject, mentions).await?)
    }

    async fn reply_to_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
    ) -> Result<Message> {
        Ok(
            Graph::reply_to_channel_message(self, team_id, channel_id, message_id, html, mentions)
                .await?,
        )
    }

    async fn mark_chat_read(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()> {
        Ok(Graph::mark_chat_read(self, chat_id, user_id, tenant_id).await?)
    }

    async fn set_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        Ok(Graph::set_reaction(self, target, reaction_type).await?)
    }

    async fn unset_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        Ok(Graph::unset_reaction(self, target, reaction_type).await?)
    }

    async fn edit_message(
        &self,
        target: &MessageTarget,
        html: &str,
        mentions: &[OutgoingMention],
    ) -> Result<()> {
        match chatsvc_conversation(target) {
            Some(conversation) if mentions.is_empty() => Ok(Messages::new(self.session())
                .edit_message(&conversation, target.message_id(), html)
                .await?),
            Some(_) => Err(Error::Unsupported(
                "editing a channel message with mentions",
            )),
            None => Ok(Graph::edit_message(self, target, html, mentions).await?),
        }
    }

    async fn soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()> {
        match chatsvc_conversation(target) {
            Some(conversation) => Ok(Messages::new(self.session())
                .soft_delete_message(&conversation, target.message_id())
                .await?),
            None => Ok(Graph::soft_delete_message(self, user_id, target).await?),
        }
    }

    async fn create_one_on_one(&self, my_user_id: &str, user_id: &str) -> Result<Chat> {
        Ok(Graph::create_one_on_one(self, my_user_id, user_id).await?)
    }

    async fn create_group(
        &self,
        my_user_id: &str,
        user_ids: &[String],
        topic: Option<&str>,
    ) -> Result<Chat> {
        Ok(Graph::create_group(self, my_user_id, user_ids, topic).await?)
    }

    async fn search_people(&self, query: &str) -> Result<Vec<User>> {
        Ok(Graph::search_people(self, query).await?)
    }

    async fn reply_with_quote(
        &self,
        chat_id: &str,
        quoted_message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
    ) -> Result<Message> {
        Ok(Graph::reply_with_quote(self, chat_id, quoted_message_id, html, mentions).await?)
    }

    async fn hosted_content(&self, url: &str) -> Result<Photo> {
        Ok(Graph::download_hosted_content(self, url).await?)
    }

    async fn user_photos(&self, user_ids: &[String]) -> Result<Vec<Result<Option<Photo>>>> {
        let photos = Graph::user_photos(self, user_ids).await?;
        Ok(photos
            .into_iter()
            .map(|photo| photo.map_err(Into::into))
            .collect())
    }

    async fn presences(&self, user_ids: &[String]) -> Result<Vec<Presence>> {
        Ok(Graph::presences(self, user_ids).await?)
    }

    async fn consumption_horizons(&self, conversation_id: &str) -> Result<Vec<MemberHorizon>> {
        Ok(Receipts::new(self.session())
            .consumption_horizons(conversation_id)
            .await?)
    }
}

fn chatsvc_conversation(target: &MessageTarget) -> Option<ConversationRef> {
    match target {
        MessageTarget::Chat { chat_id, .. } => {
            CHATSVC_CHAT_WRITES.then(|| ConversationRef::chat(chat_id))
        }
        MessageTarget::Channel {
            channel_id,
            root_id: None,
            ..
        } => Some(ConversationRef::channel_root(channel_id)),
        MessageTarget::Channel {
            channel_id,
            root_id: Some(root_id),
            ..
        } => Some(ConversationRef::channel_reply(channel_id, root_id)),
    }
}

impl From<graph::Page<Message>> for RemotePage {
    fn from(page: graph::Page<Message>) -> Self {
        RemotePage {
            items: page.items,
            next_link: page.next_link,
        }
    }
}

impl From<graph::Page<Message>> for DeltaPage {
    fn from(page: graph::Page<Message>) -> Self {
        DeltaPage {
            items: page.items,
            next_link: page.next_link,
            delta_link: page.delta_link,
        }
    }
}

impl From<graph::Page<Chat>> for ChatsPage {
    fn from(page: graph::Page<Chat>) -> Self {
        ChatsPage {
            items: page.items,
            next_link: page.next_link,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_targets_route_to_chatsvc() {
        let root = MessageTarget::channel("t", "19:c@thread.tacv2", "m1", None);
        let reply = MessageTarget::channel("t", "19:c@thread.tacv2", "r1", Some("m1"));
        assert_eq!(
            chatsvc_conversation(&root),
            Some(ConversationRef::channel_root("19:c@thread.tacv2"))
        );
        assert_eq!(
            chatsvc_conversation(&reply),
            Some(ConversationRef::channel_reply("19:c@thread.tacv2", "m1"))
        );
    }

    #[test]
    fn chats_follow_the_feature_switch() {
        let chat = MessageTarget::chat("19:a@thread.v2", "m1");
        assert_eq!(chatsvc_conversation(&chat).is_some(), CHATSVC_CHAT_WRITES);
    }
}
