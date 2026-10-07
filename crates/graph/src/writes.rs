use serde_json::{Value, json};
use session::{GRAPH, Method, Scope};

use crate::client::Graph;
use crate::error::Result;
use crate::models::{Chat, Message};
use crate::outgoing::{MessageExtras, OutgoingMention, message_body};
use crate::target::MessageTarget;
use crate::urls;

fn conversation_member(user_id: &str) -> Value {
    json!({
        "@odata.type": "#microsoft.graph.aadUserConversationMember",
        "roles": ["owner"],
        "user@odata.bind": format!("{GRAPH}/v1.0/users('{user_id}')"),
    })
}

impl Graph {
    pub async fn send_chat_message(
        &self,
        chat_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        self.write_for_message(
            Method::Post,
            &urls::chat_message_collection(chat_id),
            "Chat.ReadWrite",
            Some(message_body(html, mentions, extras)),
        )
        .await
    }

    pub async fn send_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        let mut body = message_body(html, mentions, extras);
        if let Some(subject) = subject.filter(|subject| !subject.is_empty()) {
            body["subject"] = json!(subject);
        }
        self.write_for_message(
            Method::Post,
            &urls::channel_post(team_id, channel_id),
            "ChannelMessage.Send",
            Some(body),
        )
        .await
    }

    pub async fn reply_to_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        self.write_for_message(
            Method::Post,
            &urls::channel_reply_post(team_id, channel_id, message_id),
            "ChannelMessage.Send",
            Some(message_body(html, mentions, extras)),
        )
        .await
    }

    /// Beta API `chats/{id}/messages/replyWithQuote`; Teams renders the quote from the `messageReference` attachment.
    pub async fn reply_with_quote(
        &self,
        chat_id: &str,
        quoted_message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        let body = json!({
            "messageIds": [quoted_message_id],
            "replyMessage": message_body(html, mentions, extras),
        });
        self.write_for_message(
            Method::Post,
            &urls::reply_with_quote(chat_id),
            "Chat.ReadWrite",
            Some(body),
        )
        .await
    }

    pub async fn mark_chat_read(
        &self,
        chat_id: &str,
        user_id: &str,
        tenant_id: &str,
    ) -> Result<()> {
        let body = json!({"user": {"id": user_id, "tenantId": tenant_id}});
        self.write(
            Method::Post,
            &urls::mark_chat_read(chat_id),
            "Chat.ReadWrite",
            Some(body),
        )
        .await
    }

    pub async fn set_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        self.write(
            Method::Post,
            &target.reaction_url("setReaction"),
            target.react_scope(),
            Some(json!({"reactionType": reaction_type})),
        )
        .await
    }

    pub async fn unset_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        self.write(
            Method::Post,
            &target.reaction_url("unsetReaction"),
            target.react_scope(),
            Some(json!({"reactionType": reaction_type})),
        )
        .await
    }

    pub async fn edit_message(
        &self,
        target: &MessageTarget,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<()> {
        self.write(
            Method::Patch,
            &target.url(),
            target.write_scope(),
            Some(message_body(html, mentions, extras)),
        )
        .await
    }

    /// `user_id` is only used for chats.
    pub async fn soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()> {
        self.write(
            Method::Post,
            &target.soft_delete_url(user_id),
            target.write_scope(),
            None,
        )
        .await
    }

    pub async fn create_one_on_one(&self, my_user_id: &str, user_id: &str) -> Result<Chat> {
        let body = json!({
            "chatType": "oneOnOne",
            "members": [conversation_member(my_user_id), conversation_member(user_id)],
        });
        self.write_for_chat(body).await
    }

    pub async fn create_group(
        &self,
        my_user_id: &str,
        user_ids: &[String],
        topic: Option<&str>,
    ) -> Result<Chat> {
        let members: Vec<Value> = std::iter::once(my_user_id)
            .chain(user_ids.iter().map(String::as_str))
            .map(conversation_member)
            .collect();
        let mut body = json!({"chatType": "group", "members": members});
        if let Some(topic) = topic.filter(|topic| !topic.is_empty()) {
            body["topic"] = json!(topic);
        }
        self.write_for_chat(body).await
    }

    async fn write_for_chat(&self, body: Value) -> Result<Chat> {
        let answer = self
            .session()
            .request(
                Method::Post,
                &urls::create_chat(),
                &Scope::graph("Chat.ReadWrite"),
                Some(body),
            )
            .await?;
        Ok(serde_json::from_value(answer.body)?)
    }

    async fn write_for_message(
        &self,
        method: Method,
        url: &str,
        scope: &'static str,
        body: Option<Value>,
    ) -> Result<Message> {
        let answer = self
            .session()
            .request(method, url, &Scope::graph(scope), body)
            .await?;
        Ok(serde_json::from_value(answer.body)?)
    }

    async fn write(
        &self,
        method: Method,
        url: &str,
        scope: &'static str,
        body: Option<Value>,
    ) -> Result<()> {
        self.session()
            .request(method, url, &Scope::graph(scope), body)
            .await?;
        Ok(())
    }
}
