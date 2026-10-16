use serde_json::Value;
use session::Request;

use crate::error::Result;
use crate::messages::{MessageTransport, Messages, ensure_success};
use crate::window::WindowDirection;

const ORGID_PREFIX: &str = "8:orgid:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmotionReaction {
    pub key: String,
    pub user_id: Option<String>,
    pub time_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEmotions {
    pub message_id: String,
    pub reactions: Vec<EmotionReaction>,
}

pub fn user_id_from_mri(mri: &str) -> &str {
    mri.strip_prefix(ORGID_PREFIX).unwrap_or(mri)
}

fn reactions(emotions: Option<&Value>) -> Vec<EmotionReaction> {
    emotions
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            let key = entry.get("key").and_then(Value::as_str).unwrap_or_default();
            entry
                .get("users")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(move |_| !key.is_empty())
                .map(move |user| EmotionReaction {
                    key: key.to_owned(),
                    user_id: user
                        .get("mri")
                        .and_then(Value::as_str)
                        .map(|mri| user_id_from_mri(mri).to_owned()),
                    time_ms: user.get("time").and_then(Value::as_i64),
                })
        })
        .collect()
}

/// Every listed message, including those without an `emotions` property: the chat service
/// leaves it out when nobody reacted, and an entry whose `users` is empty means removed.
pub fn parse_message_emotions(body: &Value) -> Vec<MessageEmotions> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| {
            let message_id = match message.get("id")? {
                Value::String(text) if !text.is_empty() => text.clone(),
                Value::Number(number) => number.to_string(),
                _ => return None,
            };
            Some(MessageEmotions {
                message_id,
                reactions: reactions(message.pointer("/properties/emotions")),
            })
        })
        .collect()
}

impl<T: MessageTransport> Messages<T> {
    pub async fn message_emotions(
        &self,
        conversation_id: &str,
        newest_message_id: &str,
        page_size: usize,
    ) -> Result<Vec<MessageEmotions>> {
        let url = self.message_window_url(
            conversation_id,
            newest_message_id,
            WindowDirection::Backward,
            page_size,
        );
        let answer = self.transport.send(Request::get(url)).await?;
        ensure_success(&answer)?;
        Ok(parse_message_emotions(&answer.body))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn emotions_expand_per_user_and_skip_removed_entries() {
        let body = json!({"messages": [
            {"id": "1", "properties": {"emotions": [
                {"key": "yes-tone2", "users": [
                    {"mri": "8:orgid:aaa", "time": 1792135855841_i64, "value": "1792135855835"},
                    {"mri": "8:orgid:bbb", "time": 1792135855900_i64, "value": "1"}
                ]},
                {"key": "heart", "users": []}
            ]}},
            {"id": "2", "properties": {}}
        ]});
        let parsed = parse_message_emotions(&body);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].reactions.len(), 2);
        assert_eq!(parsed[0].reactions[0].key, "yes-tone2");
        assert_eq!(parsed[0].reactions[0].user_id.as_deref(), Some("aaa"));
        assert_eq!(parsed[0].reactions[1].time_ms, Some(1792135855900));
        assert!(parsed[1].reactions.is_empty());
    }
}
