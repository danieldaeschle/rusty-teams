use serde_json::{Value, json};
use session::{Method, Request, Session};

use crate::channel_notifications::{ChannelNotifications, PROPERTY_NAME};
use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::DEFAULT_REGION;
use crate::read_horizon::{self, ReadHorizon};

const PAGE_SIZE: usize = 200;
const MAX_PAGES: usize = 100;
const ALERTS_OFF: &str = "false";
const CLEAR_HISTORY_PROPERTY: &str = "clearHistoryTime";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationState {
    pub id: String,
    pub muted: bool,
    pub hidden: Option<bool>,
    pub read: ReadHorizon,
    pub history_cleared_ms: Option<i64>,
}

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

    pub async fn conversation_states(&self) -> Result<Vec<ConversationState>> {
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
            states.extend(states_in_page(&answer.body)?);
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

    /// Teams' "Remove chat history": everything up to this time disappears for the current user only.
    pub async fn clear_history(&self, conversation_id: &str, cleared_until_ms: i64) -> Result<()> {
        let answer = self
            .transport
            .send(clear_history_request(
                &self.base_url,
                conversation_id,
                cleared_until_ms,
            ))
            .await?;
        ensure_success(&answer)
    }

    pub async fn set_read_horizon(&self, conversation_id: &str, horizon: &str) -> Result<()> {
        let url = format!(
            "{}/{}/properties?name={}",
            self.base_url,
            encode(conversation_id),
            read_horizon::PROPERTY_NAME
        );
        let body = json!({ read_horizon::PROPERTY_NAME: horizon });
        let answer = self
            .transport
            .send(Request::with_body(Method::Put, url, body))
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

pub fn clear_history_request(
    base_url: &str,
    conversation_id: &str,
    cleared_until_ms: i64,
) -> Request {
    Request::with_body(
        Method::Put,
        format!(
            "{base_url}/{}/properties?name={CLEAR_HISTORY_PROPERTY}",
            encode(conversation_id)
        ),
        json!({ CLEAR_HISTORY_PROPERTY: cleared_until_ms }),
    )
}

fn history_cleared_ms(conversation: &Value) -> Option<i64> {
    let value = &conversation["properties"][CLEAR_HISTORY_PROPERTY];
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|millis| *millis > 0)
}

/// One conversation with its thread properties, as Teams reads a meeting or 1:1 thread.
pub fn thread_url(region: &str, thread_id: &str) -> String {
    format!(
        "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/conversations/{}?view=msnp24Equivalent",
        thread_id.replace(':', "%3A").replace('@', "%40")
    )
}

fn backward_link(body: &Value) -> Option<String> {
    body.pointer("/_metadata/backwardLink")
        .and_then(Value::as_str)
        .filter(|link| !link.is_empty())
        .map(str::to_owned)
}

fn hidden_flag(conversation: &Value) -> Option<bool> {
    let flags: Vec<bool> = [
        &conversation["properties"]["hidden"],
        &conversation["threadProperties"]["hidden"],
    ]
    .into_iter()
    .filter_map(|value| match value {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) => text.parse().ok(),
        _ => None,
    })
    .collect();
    if flags.contains(&true) {
        Some(true)
    } else {
        flags.first().copied()
    }
}

fn states_in_page(body: &Value) -> Result<Vec<ConversationState>> {
    let conversations = body
        .get("conversations")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("no conversations list".into()))?;
    Ok(conversations
        .iter()
        .filter_map(|conversation| {
            let id = conversation["id"].as_str()?;
            let muted = conversation["properties"]["alerts"].as_str() == Some(ALERTS_OFF);
            Some(ConversationState {
                id: id.to_owned(),
                muted,
                hidden: hidden_flag(conversation),
                read: ReadHorizon::from_conversation(conversation),
                history_cleared_ms: history_cleared_ms(conversation),
            })
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

    fn state(id: &str, muted: bool) -> ConversationState {
        ConversationState {
            id: id.to_owned(),
            muted,
            hidden: None,
            read: ReadHorizon::default(),
            history_cleared_ms: None,
        }
    }

    const BASE: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations";

    #[test]
    fn thread_url_escapes_the_thread_id() {
        assert_eq!(
            thread_url("emea", "19:meeting_a@thread.v2"),
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/19%3Ameeting_a%40thread.v2?view=msnp24Equivalent"
        );
    }

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
            .conversation_states()
            .await
            .unwrap();
        assert_eq!(
            states,
            vec![
                state("19:a", true),
                state("19:b", false),
                state("19:c", false),
                state("19:d", false),
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
            .conversation_states()
            .await
            .unwrap();
        assert_eq!(
            states,
            vec![
                state("19:a", false),
                state("19:old", true),
                state("19:oldest", false),
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
    async fn hidden_flag_is_read_from_the_thread_properties_as_text_or_bool() {
        let transport = canned(vec![(
            200,
            json!({
                "conversations": [
                    {"id": "19:a", "threadProperties": {"hidden": "true"}},
                    {"id": "19:b", "threadProperties": {"hidden": "false"}},
                    {"id": "19:c", "properties": {"hidden": true}},
                    {"id": "19:d", "properties": {}},
                    {"id": "19:e", "properties": {"hidden": "false"}, "threadProperties": {"hidden": "true"}},
                    {"id": "19:f", "properties": {"hidden": "true"}, "threadProperties": {"hidden": "false"}},
                ],
            }),
        )]);
        let states = Conversations::with_transport(&transport, "emea")
            .conversation_states()
            .await
            .unwrap();
        let hidden: Vec<Option<bool>> = states.iter().map(|state| state.hidden).collect();
        assert_eq!(
            hidden,
            [
                Some(true),
                Some(false),
                Some(true),
                None,
                Some(true),
                Some(true)
            ]
        );
    }

    #[tokio::test]
    async fn mute_states_fail_on_an_error_status() {
        let transport = canned(vec![(500, Value::Null)]);
        let outcome = Conversations::with_transport(&transport, "emea")
            .conversation_states()
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

    #[tokio::test]
    async fn states_carry_the_read_horizon() {
        let transport = canned(vec![(
            200,
            json!({
                "conversations": [{
                    "id": "19:a",
                    "properties": {"consumptionhorizon": "5;6;7"},
                    "lastMessage": {"originalarrivaltime": "1970-01-01T00:00:00.009Z", "from": "x/contacts/8:orgid:b"},
                }],
            }),
        )]);
        let states = Conversations::with_transport(&transport, "emea")
            .conversation_states()
            .await
            .unwrap();
        assert_eq!(states[0].read.consumed_until_ms, 5);
        assert_eq!(states[0].read.last_message_ms, 9);
        assert!(states[0].read.is_unread("8:orgid:me"));
    }

    #[tokio::test]
    async fn clear_history_puts_the_time_as_a_number() {
        let transport = canned(vec![(200, Value::Null)]);
        Conversations::with_transport(&transport, "emea")
            .clear_history("19:a_b@unq.gbl.spaces", 1_792_200_000_001)
            .await
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Put);
        assert_eq!(
            requests[0].url,
            format!("{BASE}/19%3Aa_b%40unq.gbl.spaces/properties?name=clearHistoryTime")
        );
        assert_eq!(
            requests[0].body,
            Some(json!({"clearHistoryTime": 1_792_200_000_001_i64}))
        );
    }

    #[tokio::test]
    async fn states_carry_the_clear_history_time_as_text_or_number() {
        let transport = canned(vec![(
            200,
            json!({
                "conversations": [
                    {"id": "19:a", "properties": {"clearHistoryTime": "1792200000001"}},
                    {"id": "19:b", "properties": {"clearHistoryTime": 1_792_200_000_002_i64}},
                    {"id": "19:c", "properties": {"clearHistoryTime": "0"}},
                    {"id": "19:d"},
                ],
            }),
        )]);
        let states = Conversations::with_transport(&transport, "emea")
            .conversation_states()
            .await
            .unwrap();
        let cleared: Vec<Option<i64>> = states
            .iter()
            .map(|state| state.history_cleared_ms)
            .collect();
        assert_eq!(
            cleared,
            [Some(1_792_200_000_001), Some(1_792_200_000_002), None, None]
        );
    }

    #[tokio::test]
    async fn set_read_horizon_puts_the_consumption_horizon_property() {
        let transport = canned(vec![(200, Value::Null)]);
        Conversations::with_transport(&transport, "emea")
            .set_read_horizon("19:a@thread.skype", "9;10;0")
            .await
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Put);
        assert_eq!(
            requests[0].url,
            format!("{BASE}/19%3Aa%40thread.skype/properties?name=consumptionhorizon")
        );
        assert_eq!(
            requests[0].body,
            Some(json!({"consumptionhorizon": "9;10;0"}))
        );
    }
}
