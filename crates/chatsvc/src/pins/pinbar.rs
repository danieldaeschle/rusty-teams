use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use session::{Method, Request};

use super::Pins;
use super::chats::{api_error, ensure_success};
use super::transport::CsaTransport;
use crate::error::{Error, Result};
use crate::messages::encode;

const PIN_MESSAGE_TYPE: &str = "pinmsg";
const FOLDER_TYPE: &str = "folder";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedMessage {
    pub message_id: String,
    pub pinned_at: Option<DateTime<Utc>>,
    pub parent_id: Option<String>,
}

fn pinned_message(item: &Value, parent_id: Option<&str>) -> Option<PinnedMessage> {
    if item.get("type")?.as_str()? != PIN_MESSAGE_TYPE {
        return None;
    }
    let message_id = item
        .get("pinBarItemId")
        .and_then(Value::as_str)
        .or_else(|| item.pointer("/pinnedMessageProperties/messageId")?.as_str())?
        .to_owned();
    let pinned_at = item
        .pointer("/pinnedMessageProperties/pinnedDateTime")
        .and_then(|value| match value {
            Value::String(text) => text.parse::<i64>().ok(),
            other => other.as_i64(),
        })
        .and_then(DateTime::from_timestamp_millis);
    Some(PinnedMessage {
        message_id,
        pinned_at,
        parent_id: parent_id.map(str::to_owned),
    })
}

pub fn parse_chat_pins(body: &Value) -> Result<Vec<PinnedMessage>> {
    let children = body
        .get("children")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("pinbar children missing".into()))?;
    let mut pins = Vec::new();
    for child in children {
        if child.get("type").and_then(Value::as_str) == Some(FOLDER_TYPE) {
            let folder_id = child.get("pinBarItemId").and_then(Value::as_str);
            let nested = child.get("children").and_then(Value::as_array);
            pins.extend(
                nested
                    .into_iter()
                    .flatten()
                    .filter_map(|item| pinned_message(item, folder_id)),
            );
        } else {
            pins.extend(pinned_message(child, None));
        }
    }
    pins.sort_by_key(|pin| Reverse(pin.pinned_at));
    Ok(pins)
}

fn pin_body(chat_id: &str, message_id: &str, parent_id: &str, operation: &str) -> Value {
    let mut body = json!({
        "pinbarId": chat_id,
        "pinbarItemType": PIN_MESSAGE_TYPE,
        "pinbarItemId": message_id,
        "pinbarParentItemId": parent_id,
        "pinbarOperation": operation,
        "pinnedMessageProperties": {"messageId": message_id, "rootMessageId": message_id},
    });
    if operation == "add" {
        body["pinbarItemPosition"] = json!(0);
    }
    body
}

impl<T: CsaTransport> Pins<T> {
    pub fn pinbar_url(&self, chat_id: &str) -> String {
        format!("{}/chats/{}/pinbar", self.api_url, encode(chat_id))
    }

    /// Newest pin first, with the folder id of pins that live in a folder.
    pub async fn chat_pins(&self, chat_id: &str) -> Result<Vec<PinnedMessage>> {
        let answer = self
            .transport
            .send(Request::get(self.pinbar_url(chat_id)))
            .await?;
        ensure_success(&answer)?;
        parse_chat_pins(&answer.body)
    }

    pub async fn pin_message(&self, chat_id: &str, message_id: &str) -> Result<()> {
        let body = pin_body(chat_id, message_id, "", "add");
        self.write_pinbar(Method::Post, chat_id, body).await
    }

    /// `parent_id` is the folder id from `chat_pins`, `None` for a pin at the root.
    pub async fn unpin_message(
        &self,
        chat_id: &str,
        message_id: &str,
        parent_id: Option<&str>,
    ) -> Result<()> {
        let body = pin_body(chat_id, message_id, parent_id.unwrap_or_default(), "remove");
        self.write_pinbar(Method::Delete, chat_id, body).await
    }

    async fn write_pinbar(&self, method: Method, chat_id: &str, body: Value) -> Result<()> {
        let answer = self
            .transport
            .send(Request::with_body(method, self.pinbar_url(chat_id), body))
            .await?;
        if answer.is_success() {
            Ok(())
        } else {
            Err(api_error(&answer))
        }
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

    impl CsaTransport for Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: self.status,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn canned(status: u16, body: Value) -> Pins<Canned> {
        Pins::with_transport(
            Canned {
                status,
                body,
                requests: Mutex::new(Vec::new()),
            },
            "emea",
        )
    }

    fn pinmsg(message_id: &str, pinned_at: &str) -> Value {
        json!({
            "type": "pinmsg",
            "pinBarItemId": message_id,
            "pinnedMessageProperties": {
                "messageId": message_id,
                "rootMessageId": message_id,
                "pinnedDateTime": pinned_at,
            },
        })
    }

    #[tokio::test]
    async fn reads_root_and_folder_pins_newest_first() {
        let body = json!({
            "pinBarId": "19:abc@thread.v2",
            "children": [
                pinmsg("100", "1791368000000"),
                {"type": "folder", "pinBarItemId": "default-pinmsg-folder::g1", "children": [
                    pinmsg("300", "1791368300000"),
                    {"type": "tab", "pinBarItemId": "t"},
                    pinmsg("200", "1791368200000"),
                ]},
                {"type": "tab", "pinBarItemId": "t2"},
            ]
        });
        let pins = canned(200, body);
        let found = pins.chat_pins("19:abc@thread.v2").await.unwrap();
        let ids: Vec<&str> = found.iter().map(|pin| pin.message_id.as_str()).collect();
        assert_eq!(ids, ["300", "200", "100"]);
        assert_eq!(
            found[0].parent_id.as_deref(),
            Some("default-pinmsg-folder::g1")
        );
        assert_eq!(found[2].parent_id, None);
        assert_eq!(
            found[2].pinned_at.map(|time| time.timestamp_millis()),
            Some(1791368000000)
        );
        let requests = pins.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Get);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/csa/emea/api/v1/chats/19%3Aabc%40thread.v2/pinbar"
        );
    }

    #[tokio::test]
    async fn pin_posts_add_body_at_position_zero() {
        let pins = canned(201, json!({"pinBarItemId": "100"}));
        pins.pin_message("19:abc@thread.v2", "100").await.unwrap();
        let requests = pins.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Post);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/csa/emea/api/v1/chats/19%3Aabc%40thread.v2/pinbar"
        );
        assert_eq!(
            requests[0].body,
            Some(json!({
                "pinbarId": "19:abc@thread.v2",
                "pinbarItemType": "pinmsg",
                "pinbarItemId": "100",
                "pinbarParentItemId": "",
                "pinbarItemPosition": 0,
                "pinbarOperation": "add",
                "pinnedMessageProperties": {"messageId": "100", "rootMessageId": "100"},
            }))
        );
    }

    #[tokio::test]
    async fn unpin_deletes_with_remove_and_folder_parent() {
        let pins = canned(204, Value::Null);
        pins.unpin_message("c", "100", Some("default-pinmsg-folder::g1"))
            .await
            .unwrap();
        pins.unpin_message("c", "100", None).await.unwrap();
        let requests = pins.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Delete);
        let body = requests[0].body.as_ref().unwrap();
        assert_eq!(body["pinbarOperation"], "remove");
        assert_eq!(body["pinbarParentItemId"], "default-pinmsg-folder::g1");
        assert!(body.get("pinbarItemPosition").is_none());
        assert_eq!(requests[1].body.as_ref().unwrap()["pinbarParentItemId"], "");
    }

    #[tokio::test]
    async fn failures_are_errors() {
        let pins = canned(403, json!({}));
        assert!(pins.chat_pins("c").await.is_err());
        assert!(pins.pin_message("c", "1").await.is_err());
        assert!(pins.unpin_message("c", "1", None).await.is_err());
    }

    #[tokio::test]
    async fn pinbar_without_children_is_unexpected() {
        let pins = canned(200, json!({"pinBarId": "c"}));
        assert!(matches!(
            pins.chat_pins("c").await,
            Err(Error::UnexpectedAnswer(_))
        ));
    }
}
