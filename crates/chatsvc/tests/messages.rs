use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use chatsvc::messages::{ConversationRef, Messages};
use chatsvc::{Error, Result};
use serde_json::Value;
use session::{ApiResponse, Method, Request};

struct Mock {
    answers: Mutex<VecDeque<u16>>,
    requests: Mutex<Vec<Request>>,
}

impl Mock {
    fn new(statuses: &[u16]) -> Self {
        Mock {
            answers: Mutex::new(statuses.iter().copied().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl chatsvc::messages::MessageTransport for &Mock {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        self.requests.lock().unwrap().push(request);
        Ok(ApiResponse {
            status: self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .expect("mock exhausted"),
            body: Value::Null,
            retry_after: None,
        })
    }
}

fn messages(mock: &Mock) -> Messages<&Mock> {
    Messages::with_transport(mock, "emea").with_retry_delay(Duration::ZERO)
}

const BASE: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations";

#[tokio::test]
async fn edit_chat_puts_the_message_object() {
    let mock = Mock::new(&[200]);
    messages(&mock)
        .edit_message(&ConversationRef::chat("19:a@thread.v2"), "m1", "<p>hi</p>")
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::Put);
    assert_eq!(
        requests[0].url,
        format!("{BASE}/19%3Aa%40thread.v2/messages/m1")
    );
    let body = requests[0].body.as_ref().unwrap();
    assert_eq!(body["id"], "m1");
    assert_eq!(body["content"], "<p>hi</p>");
    assert_eq!(body["messagetype"], "RichText/Html");
    assert_eq!(body["contenttype"], "text");
    assert!(
        body["properties"]["edittime"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .is_ok()
    );
}

#[tokio::test]
async fn channel_reply_encodes_the_messageid_suffix() {
    let mock = Mock::new(&[200]);
    messages(&mock)
        .edit_message(
            &ConversationRef::channel_reply("19:c@thread.tacv2", "root1"),
            "r1",
            "x",
        )
        .await
        .unwrap();
    assert_eq!(
        mock.requests()[0].url,
        format!("{BASE}/19%3Ac%40thread.tacv2%3Bmessageid%3Droot1/messages/r1")
    );
}

#[tokio::test]
async fn channel_root_uses_the_channel_id() {
    let mock = Mock::new(&[200]);
    messages(&mock)
        .edit_message(
            &ConversationRef::channel_root("19:c@thread.tacv2"),
            "m1",
            "x",
        )
        .await
        .unwrap();
    assert_eq!(
        mock.requests()[0].url,
        format!("{BASE}/19%3Ac%40thread.tacv2/messages/m1")
    );
}

#[tokio::test]
async fn delete_is_soft_without_a_body() {
    let mock = Mock::new(&[200]);
    messages(&mock)
        .soft_delete_message(&ConversationRef::chat("19:a@thread.v2"), "m1")
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests[0].method, Method::Delete);
    assert_eq!(
        requests[0].url,
        format!("{BASE}/19%3Aa%40thread.v2/messages/m1?behavior=softDelete")
    );
    assert!(requests[0].body.is_none());
}

#[tokio::test]
async fn edit_retries_once_on_404() {
    let mock = Mock::new(&[404, 200]);
    messages(&mock)
        .edit_message(&ConversationRef::chat("19:a"), "m1", "x")
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url, requests[1].url);
}

#[tokio::test]
async fn edit_gives_up_after_the_second_404() {
    let mock = Mock::new(&[404, 404]);
    let error = messages(&mock)
        .edit_message(&ConversationRef::chat("19:a"), "m1", "x")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Session(session::Error::Api { status: 404, .. })
    ));
    assert_eq!(mock.requests().len(), 2);
}

#[tokio::test]
async fn delete_does_not_retry_on_404() {
    let mock = Mock::new(&[404]);
    assert!(
        messages(&mock)
            .soft_delete_message(&ConversationRef::chat("19:a"), "m1")
            .await
            .is_err()
    );
    assert_eq!(mock.requests().len(), 1);
}

#[test]
fn messages_use_the_ic3_scope() {
    let scope = chatsvc::messages::messages_scope();
    assert_eq!(scope.resource, session::IC3);
    assert_eq!(scope.name, "Teams.AccessAsUser.All");
}

#[tokio::test]
async fn set_emotion_puts_key_and_numeric_timestamp() {
    let mock = Mock::new(&[200]);
    messages(&mock)
        .set_emotion(
            &ConversationRef::chat("19:a@thread.v2"),
            "m1",
            "1f603_grinningfacewithbigeyes",
        )
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests[0].method, Method::Put);
    assert!(
        requests[0]
            .url
            .ends_with("/messages/m1/properties?name=emotions")
    );
    let emotions = &requests[0].body.as_ref().unwrap()["emotions"];
    assert_eq!(emotions["key"], "1f603_grinningfacewithbigeyes");
    assert!(emotions["value"].is_number());
}

#[tokio::test]
async fn unset_emotion_deletes_with_key_body_and_retries_a_500() {
    let mock = Mock::new(&[500, 200]);
    messages(&mock)
        .unset_emotion(&ConversationRef::chat("19:a@thread.v2"), "m1", "like")
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].method, Method::Delete);
    assert!(requests[1].url.ends_with("/properties?name=emotions"));
    assert_eq!(
        requests[1].body.as_ref().unwrap(),
        &serde_json::json!({"emotions": {"key": "like"}})
    );
}

#[tokio::test]
async fn emotion_gives_up_after_the_second_500() {
    let mock = Mock::new(&[500, 500]);
    assert!(
        messages(&mock)
            .set_emotion(&ConversationRef::chat("19:a"), "m1", "like")
            .await
            .is_err()
    );
    assert_eq!(mock.requests().len(), 2);
}

#[tokio::test]
async fn unset_emotions_tries_every_key_and_succeeds_if_one_does() {
    let mock = Mock::new(&[404, 200]);
    messages(&mock)
        .unset_emotions(&ConversationRef::chat("19:a"), "m1", &["primary", "alias"])
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].body.as_ref().unwrap()["emotions"]["key"],
        "primary"
    );
    assert_eq!(
        requests[1].body.as_ref().unwrap()["emotions"]["key"],
        "alias"
    );
}

#[tokio::test]
async fn unset_emotions_fails_when_every_key_fails() {
    let mock = Mock::new(&[404, 404]);
    assert!(
        messages(&mock)
            .unset_emotions(&ConversationRef::chat("19:a"), "m1", &["primary", "alias"])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn send_typing_posts_a_control_message_without_retrying() {
    let mock = Mock::new(&[201, 201, 500]);
    let chat = ConversationRef::chat("19:a@thread.v2");
    messages(&mock).send_typing(&chat, true).await.unwrap();
    messages(&mock).send_typing(&chat, false).await.unwrap();
    assert!(messages(&mock).send_typing(&chat, true).await.is_err());
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, Method::Post);
    assert_eq!(
        requests[0].url,
        format!("{BASE}/19%3Aa%40thread.v2/messages")
    );
    let start = requests[0].body.as_ref().unwrap();
    assert_eq!(start["messagetype"], "Control/Typing");
    assert_eq!(start["content"], "");
    assert_eq!(start["contenttype"], "Application/Message");
    assert_eq!(
        requests[1].body.as_ref().unwrap()["messagetype"],
        "Control/ClearTyping"
    );
}

#[tokio::test]
async fn send_typing_addresses_a_channel_reply_thread() {
    let mock = Mock::new(&[201]);
    messages(&mock)
        .send_typing(
            &ConversationRef::channel_reply("19:c@thread.tacv2", "99"),
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        mock.requests()[0].url,
        format!("{BASE}/19%3Ac%40thread.tacv2%3Bmessageid%3D99/messages")
    );
}
