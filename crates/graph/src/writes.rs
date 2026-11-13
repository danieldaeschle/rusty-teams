use serde_json::{Value, json};
use session::{Method, Scope};

use crate::client::Graph;
use crate::error::Result;
use crate::models::Message;
use crate::outgoing::{MessageExtras, OutgoingMention, message_body};
use crate::target::MessageTarget;
use crate::urls;

impl Graph {
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

    /// `user_id` is only used for chats.
    pub async fn undo_soft_delete_message(
        &self,
        user_id: &str,
        target: &MessageTarget,
    ) -> Result<()> {
        self.write(
            Method::Post,
            &target.undo_soft_delete_url(user_id),
            target.write_scope(),
            None,
        )
        .await
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
