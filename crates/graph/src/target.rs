use crate::urls;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageTarget {
    Chat {
        chat_id: String,
        message_id: String,
    },
    /// `root_id` is `Some` when `message_id` is a reply in the thread of `root_id`.
    Channel {
        team_id: String,
        channel_id: String,
        message_id: String,
        root_id: Option<String>,
    },
}

impl MessageTarget {
    pub fn chat(chat_id: &str, message_id: &str) -> Self {
        MessageTarget::Chat {
            chat_id: chat_id.to_owned(),
            message_id: message_id.to_owned(),
        }
    }

    pub fn channel(
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        root_id: Option<&str>,
    ) -> Self {
        MessageTarget::Channel {
            team_id: team_id.to_owned(),
            channel_id: channel_id.to_owned(),
            message_id: message_id.to_owned(),
            root_id: root_id.map(str::to_owned),
        }
    }

    pub fn message_id(&self) -> &str {
        match self {
            MessageTarget::Chat { message_id, .. } | MessageTarget::Channel { message_id, .. } => {
                message_id
            }
        }
    }

    pub(crate) fn url(&self) -> String {
        match self {
            MessageTarget::Chat {
                chat_id,
                message_id,
            } => urls::chat_message(chat_id, message_id),
            MessageTarget::Channel {
                team_id,
                channel_id,
                message_id,
                root_id: None,
            } => urls::channel_message(team_id, channel_id, message_id),
            MessageTarget::Channel {
                team_id,
                channel_id,
                message_id,
                root_id: Some(root_id),
            } => urls::channel_reply(team_id, channel_id, root_id, message_id),
        }
    }

    pub(crate) fn soft_delete_url(&self, user_id: &str) -> String {
        match self {
            MessageTarget::Chat {
                chat_id,
                message_id,
            } => urls::soft_delete(user_id, chat_id, message_id),
            channel => format!("{}/softDelete", channel.url()),
        }
    }

    pub(crate) fn reaction_url(&self, action: &str) -> String {
        format!("{}/{action}", self.url())
    }

    pub(crate) fn react_scope(&self) -> &'static str {
        match self {
            MessageTarget::Chat { .. } => "Chat.ReadWrite",
            MessageTarget::Channel { .. } => "ChannelMessage.Send",
        }
    }

    pub(crate) fn write_scope(&self) -> &'static str {
        match self {
            MessageTarget::Chat { .. } => "Chat.ReadWrite",
            MessageTarget::Channel { .. } => "ChannelMessage.ReadWrite",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_urls_cover_root_and_reply() {
        let root = MessageTarget::channel("t", "19:c@thread.tacv2", "m1", None);
        let reply = MessageTarget::channel("t", "19:c@thread.tacv2", "r1", Some("m1"));
        assert_eq!(
            root.url(),
            "https://graph.microsoft.com/v1.0/teams/t/channels/19%3Ac%40thread.tacv2/messages/m1"
        );
        assert_eq!(
            reply.reaction_url("setReaction"),
            "https://graph.microsoft.com/v1.0/teams/t/channels/19%3Ac%40thread.tacv2/messages/m1/replies/r1/setReaction"
        );
        assert!(
            root.soft_delete_url("u")
                .ends_with("/messages/m1/softDelete")
        );
        assert!(!root.soft_delete_url("u").contains("/users/"));
    }

    #[test]
    fn chat_soft_delete_goes_through_the_user() {
        let chat = MessageTarget::chat("19:a@thread.v2", "m1");
        assert_eq!(
            chat.soft_delete_url("u1"),
            "https://graph.microsoft.com/v1.0/users/u1/chats/19%3Aa%40thread.v2/messages/m1/softDelete"
        );
        assert!(
            chat.reaction_url("unsetReaction")
                .ends_with("/messages/m1/unsetReaction")
        );
    }

    #[test]
    fn scopes_follow_the_documented_permissions() {
        let channel = MessageTarget::channel("t", "c", "m", None);
        assert_eq!(channel.react_scope(), "ChannelMessage.Send");
        assert_eq!(channel.write_scope(), "ChannelMessage.ReadWrite");
        assert_eq!(
            MessageTarget::chat("c", "m").write_scope(),
            "Chat.ReadWrite"
        );
    }
}
