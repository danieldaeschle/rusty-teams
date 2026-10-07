use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionTarget {
    User { user_id: String },
    Channel { channel_id: String },
    Team { team_id: String },
}

/// `id` matches the `<at id="n">` tag in the html body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMention {
    pub id: u32,
    pub text: String,
    pub target: MentionTarget,
}

impl OutgoingMention {
    pub fn to_json(&self) -> Value {
        let mentioned = match &self.target {
            MentionTarget::User { user_id } => json!({
                "user": {"id": user_id, "displayName": self.text, "userIdentityType": "aadUser"}
            }),
            MentionTarget::Channel { channel_id } => json!({
                "conversation": {"id": channel_id, "displayName": self.text, "conversationIdentityType": "channel"}
            }),
            MentionTarget::Team { team_id } => json!({
                "conversation": {"id": team_id, "displayName": self.text, "conversationIdentityType": "team"}
            }),
        };
        json!({"id": self.id, "mentionText": self.text, "mentioned": mentioned})
    }
}

pub(crate) fn message_body(html: &str, mentions: &[OutgoingMention]) -> Value {
    let mut body = json!({"body": {"contentType": "html", "content": html}});
    if !mentions.is_empty() {
        body["mentions"] = Value::Array(mentions.iter().map(OutgoingMention::to_json).collect());
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(id: u32, text: &str, target: MentionTarget) -> OutgoingMention {
        OutgoingMention {
            id,
            text: text.to_owned(),
            target,
        }
    }

    #[test]
    fn user_mention_matches_the_documented_payload() {
        let value = mention(
            0,
            "Ada",
            MentionTarget::User {
                user_id: "u1".into(),
            },
        )
        .to_json();
        assert_eq!(
            value,
            json!({"id": 0, "mentionText": "Ada", "mentioned": {"user": {"id": "u1", "displayName": "Ada", "userIdentityType": "aadUser"}}})
        );
    }

    #[test]
    fn channel_and_team_mentions_use_conversation_identity() {
        let channel = mention(
            1,
            "General",
            MentionTarget::Channel {
                channel_id: "19:c".into(),
            },
        )
        .to_json();
        let team = mention(
            2,
            "Squad",
            MentionTarget::Team {
                team_id: "t".into(),
            },
        )
        .to_json();
        assert_eq!(
            channel["mentioned"]["conversation"]["conversationIdentityType"],
            "channel"
        );
        assert_eq!(
            team["mentioned"]["conversation"]["conversationIdentityType"],
            "team"
        );
        assert_eq!(team["mentioned"]["conversation"]["id"], "t");
    }

    #[test]
    fn body_without_mentions_has_no_mentions_key() {
        assert!(message_body("<p>x</p>", &[]).get("mentions").is_none());
        let with = message_body(
            "<at id=\"0\">A</at>",
            &[mention(
                0,
                "A",
                MentionTarget::User {
                    user_id: "u".into(),
                },
            )],
        );
        assert_eq!(with["mentions"].as_array().unwrap().len(), 1);
    }
}
