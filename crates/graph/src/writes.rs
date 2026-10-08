use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use session::{GRAPH, Method, Scope};

use crate::client::Graph;
use crate::error::{Error, Result};
use crate::models::{Chat, Member, Message};
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

fn user_body(user_id: &str, tenant_id: &str) -> Value {
    json!({"user": {"id": user_id, "tenantId": tenant_id}})
}

fn mark_unread_body(user_id: &str, tenant_id: &str, last_read_at: DateTime<Utc>) -> Value {
    json!({
        "user": {"id": user_id, "tenantId": tenant_id},
        "lastMessageReadDateTime": last_read_at.to_rfc3339_opts(SecondsFormat::Millis, true),
    })
}

fn own_membership_id<'a>(members: &'a [Member], user_id: &str) -> Option<&'a str> {
    members
        .iter()
        .find(|member| member.user_id.as_deref() == Some(user_id))
        .and_then(|member| member.id.as_deref())
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
        self.write(
            Method::Post,
            &urls::mark_chat_read(chat_id),
            "Chat.ReadWrite",
            Some(user_body(user_id, tenant_id)),
        )
        .await
    }

    pub async fn mark_chat_unread(
        &self,
        chat_id: &str,
        user_id: &str,
        tenant_id: &str,
        last_read_at: DateTime<Utc>,
    ) -> Result<()> {
        let body = mark_unread_body(user_id, tenant_id, last_read_at);
        self.write(
            Method::Post,
            &urls::mark_chat_unread(chat_id),
            "Chat.ReadWrite",
            Some(body),
        )
        .await
    }

    pub async fn hide_chat(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()> {
        self.write(
            Method::Post,
            &urls::hide_chat(chat_id),
            "Chat.ReadWrite",
            Some(user_body(user_id, tenant_id)),
        )
        .await
    }

    pub async fn unhide_chat(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()> {
        self.write(
            Method::Post,
            &urls::unhide_chat(chat_id),
            "Chat.ReadWrite",
            Some(user_body(user_id, tenant_id)),
        )
        .await
    }

    pub async fn leave_chat(&self, chat_id: &str, user_id: &str) -> Result<()> {
        let members = self.chat_members(chat_id).await?;
        let membership_id = own_membership_id(&members, user_id).ok_or(Error::NotAMember)?;
        self.write(
            Method::Delete,
            &urls::chat_member(chat_id, membership_id),
            "Chat.ReadWrite",
            None,
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn member(id: &str, user_id: &str) -> Member {
        Member {
            id: Some(id.to_owned()),
            user_id: Some(user_id.to_owned()),
            tenant_id: None,
            display_name: None,
            email: None,
        }
    }

    #[test]
    fn own_membership_is_found_by_user_id() {
        let members = [member("m1", "u1"), member("m2", "u2")];
        assert_eq!(own_membership_id(&members, "u2"), Some("m2"));
        assert_eq!(own_membership_id(&members, "u3"), None);
    }

    #[test]
    fn mark_unread_body_carries_the_read_horizon() {
        let at = Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 0).unwrap();
        let body = mark_unread_body("u1", "t1", at);
        assert_eq!(body["user"]["id"], "u1");
        assert_eq!(body["user"]["tenantId"], "t1");
        assert_eq!(body["lastMessageReadDateTime"], "2026-10-06T09:00:00.000Z");
    }
}
