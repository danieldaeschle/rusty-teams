use serde_json::{Value, json};
use session::{Method, Request, Session};

use crate::channel_notifications::{ChannelNotifications, PROPERTY_NAME};
use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::DEFAULT_REGION;

const PAGE_SIZE: usize = 200;
const MAX_PAGES: usize = 100;
const ALERTS_OFF: &str = "false";

pub struct Conversations<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    base_url: String,
}

impl Conversations<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        Conversations::with_transport(SessionMessageTransport::new(session), region)
    }
}

impl<T: MessageTransport> Conversations<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        Conversations {
            transport,
            base_url: format!(
                "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/conversations"
            ),
        }
    }

    pub fn alerts_url(&self, conversation_id: &str) -> String {
        format!(
            "{}/{}/properties?name=alerts",
            self.base_url,
            encode(conversation_id)
        )
    }

    pub async fn conversation_mute_states(&self) -> Result<Vec<(String, bool)>> {
        let url = format!(
            "{}?view=msnp24Equivalent&pageSize={PAGE_SIZE}",
            self.base_url
        );
        let mut states = Vec::new();
        let mut next_url = Some(url);
        for _ in 0..MAX_PAGES {
            let Some(url) = next_url.take() else {
                break;
            };
            let answer = self.transport.send(Request::get(&url)).await?;
            ensure_success(&answer)?;
            states.extend(mute_states_in_page(&answer.body)?);
            next_url = backward_link(&answer.body).filter(|link| *link != url);
        }
        Ok(states)
    }

    pub async fn set_alerts(&self, conversation_id: &str, enabled: bool) -> Result<()> {
        let body = json!({"alerts": if enabled { "true" } else { ALERTS_OFF }});
        let answer = self
            .transport
            .send(Request::with_body(
                Method::Put,
                self.alerts_url(conversation_id),
                body,
            ))
            .await?;
        ensure_success(&answer)
    }

    pub async fn set_channel_notifications(
        &self,
        channel_id: &str,
        notifications: ChannelNotifications,
    ) -> Result<()> {
        let url = format!(
            "{}/{}/properties?name={PROPERTY_NAME}",
            self.base_url,
            encode(channel_id)
        );
        let body = json!({ PROPERTY_NAME: notifications.property_value() });
        let answer = self
            .transport
            .send(Request::with_body(Method::Put, url, body))
            .await?;
        ensure_success(&answer)
    }
}

fn backward_link(body: &Value) -> Option<String> {
    body.pointer("/_metadata/backwardLink")
        .and_then(Value::as_str)
        .filter(|link| !link.is_empty())
        .map(str::to_owned)
}

fn mute_states_in_page(body: &Value) -> Result<Vec<(String, bool)>> {
    let conversations = body
        .get("conversations")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("no conversations list".into()))?;
    Ok(conversations
        .iter()
        .filter_map(|conversation| {
            let id = conversation["id"].as_str()?;
            let muted = conversation["properties"]["alerts"].as_str() == Some(ALERTS_OFF);
            Some((id.to_owned(), muted))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    struct Canned {
        answers: Mutex<VecDeque<(u16, Value)>>,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            let (status, body) = self.answers.lock().unwrap().pop_front().expect("exhausted");
            Ok(ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    fn canned(answers: Vec<(u16, Value)>) -> Canned {
        Canned {
            answers: Mutex::new(answers.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    const BASE: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations";

    #[tokio::test]
    async fn mute_states_pair_every_conversation_with_its_flag() {
        let transport = canned(vec![(
            200,
            json!({
                "conversations": [
                    {"id": "19:a", "properties": {"alerts": "false"}},
                    {"id": "19:b", "properties": {"alerts": "true"}},
                    {"id": "19:c", "properties": {}},
                    {"id": "19:d"},
                ],
            }),
        )]);
        let states = Conversations::with_transport(&transport, "emea")
            .conversation_mute_states()
            .await
            .unwrap();
        assert_eq!(
            states,
            vec![
                ("19:a".to_owned(), true),
                ("19:b".to_owned(), false),
                ("19:c".to_owned(), false),
                ("19:d".to_owned(), false),
            ]
        );
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url,
            format!("{BASE}?view=msnp24Equivalent&pageSize=200")
        );
    }

    #[tokio::test]
    async fn mute_states_follow_the_backward_link_through_every_page() {
        let transport = canned(vec![
            (
                200,
                json!({
                    "conversations": [{"id": "19:a", "properties": {"alerts": "true"}}],
                    "_metadata": {"backwardLink": "https://example.test/page2"},
                }),
            ),
            (
                200,
                json!({
                    "conversations": [{"id": "19:old", "properties": {"alerts": "false"}}],
                    "_metadata": {"backwardLink": "https://example.test/page3"},
                }),
            ),
            (200, json!({"conversations": [{"id": "19:oldest"}]})),
        ]);
        let states = Conversations::with_transport(&transport, "emea")
            .conversation_mute_states()
            .await
            .unwrap();
        assert_eq!(
            states,
            vec![
                ("19:a".to_owned(), false),
                ("19:old".to_owned(), true),
                ("19:oldest".to_owned(), false),
            ]
        );
        let requests = transport.requests.lock().unwrap();
        let urls: Vec<&str> = requests
            .iter()
            .map(|request| request.url.as_str())
            .collect();
        assert_eq!(urls[1], "https://example.test/page2");
        assert_eq!(urls[2], "https://example.test/page3");
    }

    #[tokio::test]
    async fn mute_states_fail_on_an_error_status() {
        let transport = canned(vec![(500, Value::Null)]);
        let outcome = Conversations::with_transport(&transport, "emea")
            .conversation_mute_states()
            .await;
        assert!(outcome.is_err());
    }

    #[tokio::test]
    async fn set_alerts_puts_the_flag_as_text() {
        let transport = canned(vec![(200, Value::Null), (200, Value::Null)]);
        let conversations = Conversations::with_transport(&transport, "emea");
        conversations
            .set_alerts("19:a@thread.v2", false)
            .await
            .unwrap();
        conversations
            .set_alerts("19:a@thread.v2", true)
            .await
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Put);
        assert_eq!(
            requests[0].url,
            format!("{BASE}/19%3Aa%40thread.v2/properties?name=alerts")
        );
        assert_eq!(requests[0].body, Some(json!({"alerts": "false"})));
        assert_eq!(requests[1].body, Some(json!({"alerts": "true"})));
    }
}
