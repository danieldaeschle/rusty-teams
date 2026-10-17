use serde_json::Value;
use session::Request;

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, Messages, encode, ensure_success};

const EPOCH_SEGMENT: &str = "epochTimeStamp/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowDirection {
    Bidirectional,
    Backward,
    Forward,
}

impl WindowDirection {
    fn query_value(self) -> &'static str {
        match self {
            WindowDirection::Bidirectional => "BIDIRECTIONAL",
            WindowDirection::Backward => "BACKWARD",
            WindowDirection::Forward => "FORWARD",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageWindow {
    pub message_ids: Vec<String>,
    pub older_cursor: Option<String>,
    pub newer_cursor: Option<String>,
}

fn is_user_message(message: &Value) -> bool {
    message
        .get("messagetype")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "Text" || kind == "Event/Call" || kind.starts_with("RichText"))
}

fn message_id(message: &Value) -> Option<String> {
    crate::json_id::id_text(message.get("id")?)
}

fn cursor_of(body: &Value, link_key: &str) -> Option<String> {
    let link = body.pointer(&format!("/_metadata/{link_key}"))?.as_str()?;
    let tail = &link[link.find(EPOCH_SEGMENT)? + EPOCH_SEGMENT.len()..];
    let number = tail.split(['?', '/']).next()?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| number.to_owned())
}

pub fn parse_message_window(body: &Value) -> Result<MessageWindow> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("no messages list".into()))?;
    Ok(MessageWindow {
        message_ids: messages
            .iter()
            .filter(|message| is_user_message(message))
            .filter_map(message_id)
            .collect(),
        older_cursor: cursor_of(body, "backwardLink"),
        newer_cursor: cursor_of(body, "forwardLink"),
    })
}

impl<T: MessageTransport> Messages<T> {
    pub fn message_window_url(
        &self,
        chat_id: &str,
        anchor_id: &str,
        direction: WindowDirection,
        page_size: usize,
    ) -> String {
        format!(
            "{}/{}/messages/{EPOCH_SEGMENT}{}?pageSize={page_size}&direction={}&includeMetadata=true&locationType=Primary",
            self.base_url,
            encode(chat_id),
            encode(anchor_id),
            direction.query_value()
        )
    }

    pub async fn message_window(
        &self,
        chat_id: &str,
        anchor_id: &str,
        direction: WindowDirection,
        page_size: usize,
    ) -> Result<MessageWindow> {
        let url = self.message_window_url(chat_id, anchor_id, direction, page_size);
        let answer = self.transport.send(Request::get(url)).await?;
        ensure_success(&answer)?;
        parse_message_window(&answer.body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;
    use session::{ApiResponse, Method};

    use super::*;

    struct Canned {
        body: Value,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: 200,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn fixture() -> Value {
        json!({
            "messages": [
                {"id": "1791368000030", "messagetype": "RichText/Html"},
                {"id": "1791368000025", "messagetype": "ThreadActivity/AddMember"},
                {"id": "1791368000020", "messagetype": "Text"},
                {"id": "1791368000015", "messagetype": "Event/Call"},
                {"id": 1791368000010u64, "messagetype": "RichText/Media_Card"},
            ],
            "_metadata": {
                "backwardLink": "https://emea.ng.msg.teams.microsoft.com/v1/users/ME/conversations/19%3Aabc%40thread.v2/messages/epochTimeStamp/1791368000009?pageSize=20&direction=BACKWARD&includeMetadata=true",
                "forwardLink": "https://other.host/v1/users/ME/conversations/19%3Aabc%40thread.v2/messages/epochTimeStamp/1791368000031?pageSize=20&direction=FORWARD",
            }
        })
    }

    #[test]
    fn parses_user_messages_and_cursors() {
        let window = parse_message_window(&fixture()).unwrap();
        assert_eq!(
            window.message_ids,
            [
                "1791368000030",
                "1791368000020",
                "1791368000015",
                "1791368000010"
            ]
        );
        assert_eq!(window.older_cursor.as_deref(), Some("1791368000009"));
        assert_eq!(window.newer_cursor.as_deref(), Some("1791368000031"));
    }

    #[test]
    fn absent_links_mean_no_more_pages() {
        let window = parse_message_window(&json!({
            "messages": [{"id": "1", "messagetype": "Text"}],
            "_metadata": {}
        }))
        .unwrap();
        assert_eq!(window.older_cursor, None);
        assert_eq!(window.newer_cursor, None);
        let without_metadata = parse_message_window(&json!({"messages": []})).unwrap();
        assert_eq!(without_metadata.older_cursor, None);
    }

    #[test]
    fn link_without_a_number_gives_no_cursor() {
        let window = parse_message_window(&json!({
            "messages": [],
            "_metadata": {"backwardLink": "https://h/messages/epochTimeStamp/?x=1"}
        }))
        .unwrap();
        assert_eq!(window.older_cursor, None);
    }

    #[test]
    fn missing_messages_is_unexpected() {
        assert!(matches!(
            parse_message_window(&json!({})),
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    #[tokio::test]
    async fn requests_the_window_around_the_anchor() {
        let messages = Messages::with_transport(
            Canned {
                body: fixture(),
                requests: Mutex::new(Vec::new()),
            },
            "emea",
        );
        messages
            .message_window(
                "19:abc@thread.v2",
                "1791368000020",
                WindowDirection::Bidirectional,
                20,
            )
            .await
            .unwrap();
        let requests = messages.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Get);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/19%3Aabc%40thread.v2/messages/epochTimeStamp/1791368000020?pageSize=20&direction=BIDIRECTIONAL&includeMetadata=true&locationType=Primary"
        );
    }
}
