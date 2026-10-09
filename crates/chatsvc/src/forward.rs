use chrono::Utc;
use serde_json::{Value, json};
use session::{Method, Request};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, Messages, encode, ensure_success};

pub const MAX_FORWARD_MESSAGES: usize = 5;
const FORWARD_TEMPLATE: &str = "basic_forward_message_template";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardResult {
    pub thread_id: String,
    pub message_id: String,
}

fn forward_body(target_conversation_id: &str, message_ids: &[String], comment_html: &str) -> Value {
    json!({
        "targetThreadIds": [target_conversation_id],
        "messageIds": message_ids,
        "additionalMessage": {
            "clientmessageid": Utc::now().timestamp_millis().to_string(),
            "content": comment_html,
            "messagetype": "RichText/Html",
            "contenttype": "text",
            "imdisplayname": "",
            "properties": {"importance": "", "subject": ""},
        },
        "templateId": FORWARD_TEMPLATE,
    })
}

fn parse_forward_result(body: &Value) -> Result<ForwardResult> {
    let entry = body
        .as_array()
        .and_then(|entries| entries.first())
        .ok_or_else(|| Error::UnexpectedAnswer("no forward result".into()))?;
    let status = entry
        .get("statusCode")
        .and_then(Value::as_u64)
        .and_then(|status| u16::try_from(status).ok())
        .ok_or_else(|| Error::UnexpectedAnswer("forward result has no statusCode".into()))?;
    if !(200..300).contains(&status) {
        return Err(Error::Session(session::Error::api(
            status,
            "chatsvc",
            entry.clone(),
        )));
    }
    let text = |key: &str| match entry.get(key) {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    };
    Ok(ForwardResult {
        thread_id: text("threadId")
            .ok_or_else(|| Error::UnexpectedAnswer("forward result has no threadId".into()))?,
        message_id: text("messageId")
            .ok_or_else(|| Error::UnexpectedAnswer("forward result has no messageId".into()))?,
    })
}

impl<T: MessageTransport> Messages<T> {
    pub fn forward_url(&self, source_conversation_id: &str) -> String {
        format!(
            "{}/{}/messages/forward",
            self.base_url,
            encode(source_conversation_id)
        )
    }

    /// `message_ids` holds 1 to 5 ids, oldest first.
    pub async fn forward_messages(
        &self,
        source_conversation_id: &str,
        target_conversation_id: &str,
        message_ids: &[String],
        comment_html: &str,
    ) -> Result<ForwardResult> {
        if message_ids.is_empty() || message_ids.len() > MAX_FORWARD_MESSAGES {
            return Err(Error::UnexpectedAnswer(format!(
                "forwarding needs 1 to {MAX_FORWARD_MESSAGES} messages"
            )));
        }
        let body = forward_body(target_conversation_id, message_ids, comment_html);
        let answer = self
            .transport
            .send(Request::with_body(
                Method::Post,
                self.forward_url(source_conversation_id),
                body,
            ))
            .await?;
        ensure_success(&answer)?;
        parse_forward_result(&answer.body)
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

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[tokio::test]
    async fn posts_forward_body_to_encoded_source() {
        let messages = canned(
            200,
            json!([{"statusCode": 201, "threadId": "19:notes@thread.v2", "messageId": "1791368146075"}]),
        );
        let result = messages
            .forward_messages(
                "19:src@thread.v2",
                "48:notes",
                &ids(&["1", "2"]),
                "<p>fyi</p>",
            )
            .await
            .unwrap();
        assert_eq!(
            result,
            ForwardResult {
                thread_id: "19:notes@thread.v2".into(),
                message_id: "1791368146075".into(),
            }
        );
        let requests = messages.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Post);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/19%3Asrc%40thread.v2/messages/forward"
        );
        let body = requests[0].body.as_ref().unwrap();
        assert_eq!(body["targetThreadIds"], json!(["48:notes"]));
        assert_eq!(body["messageIds"], json!(["1", "2"]));
        assert_eq!(body["templateId"], "basic_forward_message_template");
        assert_eq!(body["additionalMessage"]["content"], "<p>fyi</p>");
        assert_eq!(body["additionalMessage"]["messagetype"], "RichText/Html");
        assert_eq!(body["additionalMessage"]["contenttype"], "text");
        assert!(
            body["additionalMessage"]["clientmessageid"]
                .as_str()
                .unwrap()
                .parse::<i64>()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn failing_target_status_is_an_error() {
        let messages = canned(
            200,
            json!([{"statusCode": 403, "threadId": "t", "messageId": "1"}]),
        );
        assert!(
            messages
                .forward_messages("a", "b", &ids(&["1"]), "")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn http_failure_and_empty_answer_are_errors() {
        let failing = canned(500, json!({}));
        assert!(
            failing
                .forward_messages("a", "b", &ids(&["1"]), "")
                .await
                .is_err()
        );
        let empty = canned(200, json!([]));
        assert!(matches!(
            empty.forward_messages("a", "b", &ids(&["1"]), "").await,
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    #[tokio::test]
    async fn message_count_is_limited_before_sending() {
        let messages = canned(200, json!([]));
        assert!(messages.forward_messages("a", "b", &[], "").await.is_err());
        let six = ids(&["1", "2", "3", "4", "5", "6"]);
        assert!(messages.forward_messages("a", "b", &six, "").await.is_err());
        assert!(messages.transport.requests.lock().unwrap().is_empty());
    }
}
