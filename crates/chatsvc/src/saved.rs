use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use session::{Method, Request};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, Messages, encode, ensure_success};

const SAVED_CONVERSATION: &str = "48:saved";
const SAVED_ACTIVITY_TYPE: &str = "savedMessage";
const LIST_QUERY: &str = "view=msnp24Equivalent&pageSize=200&startTime=1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedMessage {
    pub conversation_id: String,
    pub message_id: String,
    pub root_id: String,
    pub author_id: Option<String>,
    pub author_name: Option<String>,
    pub preview: String,
    pub saved_at: DateTime<Utc>,
    pub topic: Option<String>,
}

fn text_or_number(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn activity_of(message: &Value) -> Option<Value> {
    match message.pointer("/properties/activity")? {
        Value::String(text) => serde_json::from_str(text).ok(),
        object @ Value::Object(_) => Some(object.clone()),
        _ => None,
    }
}

fn saved_message(message: &Value) -> Option<SavedMessage> {
    let activity = activity_of(message)?;
    if activity.get("activityType")?.as_str()? != SAVED_ACTIVITY_TYPE {
        return None;
    }
    let message_id = text_or_number(&activity, "sourceMessageId")?;
    let saved_at = DateTime::parse_from_rfc3339(activity.get("activityTimestamp")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    Some(SavedMessage {
        conversation_id: text_or_number(&activity, "sourceThreadId")?,
        root_id: text_or_number(&activity, "sourceReplyChainId")
            .unwrap_or_else(|| message_id.clone()),
        message_id,
        author_id: text_or_number(&activity, "sourceUserId"),
        author_name: text_or_number(&activity, "sourceUserImDisplayName"),
        preview: text_or_number(&activity, "messagePreview").unwrap_or_default(),
        saved_at,
        topic: text_or_number(&activity, "sourceThreadTopic"),
    })
}

pub fn parse_saved_messages(body: &Value) -> Result<Vec<SavedMessage>> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("no messages list".into()))?;
    let mut saved: Vec<SavedMessage> = messages.iter().filter_map(saved_message).collect();
    saved.sort_by_key(|entry| Reverse(entry.saved_at));
    Ok(saved)
}

impl<T: MessageTransport> Messages<T> {
    pub fn rc_metadata_url(&self, conversation_id: &str, root_id: &str) -> String {
        format!(
            "{}/{}/rcmetadata/{}",
            self.base_url,
            encode(conversation_id),
            encode(root_id)
        )
    }

    pub fn saved_list_url(&self) -> String {
        format!(
            "{}/{}/messages?{LIST_QUERY}",
            self.base_url,
            encode(SAVED_CONVERSATION)
        )
    }

    /// `root_id` is the reply chain id: the message id in chats, the root post id in channels.
    pub async fn set_saved(
        &self,
        conversation_id: &str,
        root_id: &str,
        message_id: &str,
        saved: bool,
    ) -> Result<()> {
        let message_number: u64 = message_id.parse().map_err(|_| {
            Error::UnexpectedAnswer(format!("message id {message_id} is not numeric"))
        })?;
        let body = json!({"s": u8::from(saved), "mid": message_number});
        let answer = self
            .transport
            .send(Request::with_body(
                Method::Put,
                self.rc_metadata_url(conversation_id, root_id),
                body,
            ))
            .await?;
        ensure_success(&answer)
    }

    pub async fn list_saved(&self) -> Result<Vec<SavedMessage>> {
        let answer = self
            .transport
            .send(Request::get(self.saved_list_url()))
            .await?;
        ensure_success(&answer)?;
        parse_saved_messages(&answer.body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    struct Canned {
        status: u16,
        body: Value,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: self.status,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn canned(status: u16, body: Value) -> Messages<Canned> {
        Messages::with_transport(
            Canned {
                status,
                body,
                requests: Mutex::new(Vec::new()),
            },
            "emea",
        )
    }

    fn saved_entry(activity: Value) -> Value {
        json!({"messagetype": "Text", "content": "", "properties": {"activity": activity}})
    }

    fn activity(message_id: u64, saved_at: &str) -> Value {
        json!({
            "activityType": "savedMessage",
            "activitySubtype": "saved",
            "sourceThreadId": "19:abc@thread.v2",
            "sourceMessageId": message_id,
            "sourceReplyChainId": 1791368000000u64,
            "sourceUserId": "8:orgid:a",
            "sourceUserImDisplayName": "Ada",
            "messagePreview": "hello",
            "activityTimestamp": saved_at,
            "sourceThreadTopic": "Project",
        })
    }

    #[tokio::test]
    async fn save_puts_numeric_message_id() {
        let messages = canned(200, json!({"conversationId": "c"}));
        messages
            .set_saved("19:abc@thread.v2", "1791368000000", "1791368000001", true)
            .await
            .unwrap();
        messages
            .set_saved("19:abc@thread.v2", "1791368000000", "1791368000001", false)
            .await
            .unwrap();
        let requests = messages.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Put);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/19%3Aabc%40thread.v2/rcmetadata/1791368000000"
        );
        assert_eq!(
            requests[0].body,
            Some(json!({"s": 1, "mid": 1791368000001u64}))
        );
        assert_eq!(
            requests[1].body,
            Some(json!({"s": 0, "mid": 1791368000001u64}))
        );
    }

    #[tokio::test]
    async fn save_rejects_non_numeric_ids_and_failures() {
        let messages = canned(200, json!({}));
        assert!(messages.set_saved("c", "1", "abc", true).await.is_err());
        assert!(messages.transport.requests.lock().unwrap().is_empty());
        let failing = canned(403, json!({}));
        assert!(failing.set_saved("c", "1", "1", true).await.is_err());
    }

    #[tokio::test]
    async fn lists_saved_newest_first_from_object_and_string_activity() {
        let body = json!({"messages": [
            saved_entry(activity(1791368000001, "2026-10-01T10:00:00.000Z")),
            saved_entry(json!(activity(1791368000002, "2026-10-03T10:00:00.000Z").to_string())),
            saved_entry(json!({"activityType": "mention", "sourceThreadId": "x"})),
            json!({"messagetype": "Text", "properties": {}}),
            saved_entry(json!("not json")),
        ]});
        let messages = canned(200, body);
        let saved = messages.list_saved().await.unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].message_id, "1791368000002");
        assert_eq!(saved[1].message_id, "1791368000001");
        assert_eq!(saved[1].conversation_id, "19:abc@thread.v2");
        assert_eq!(saved[1].root_id, "1791368000000");
        assert_eq!(saved[1].author_id.as_deref(), Some("8:orgid:a"));
        assert_eq!(saved[1].author_name.as_deref(), Some("Ada"));
        assert_eq!(saved[1].preview, "hello");
        assert_eq!(saved[1].topic.as_deref(), Some("Project"));
        assert_eq!(saved[1].saved_at.timestamp_millis(), 1_790_848_800_000);
        let requests = messages.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Get);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Asaved/messages?view=msnp24Equivalent&pageSize=200&startTime=1"
        );
    }

    #[tokio::test]
    async fn list_without_messages_is_unexpected() {
        let messages = canned(200, json!({}));
        assert!(matches!(
            messages.list_saved().await,
            Err(Error::UnexpectedAnswer(_))
        ));
    }
}
