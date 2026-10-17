use chrono::DateTime;
use serde_json::Value;
use session::{Request, Session};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, ensure_success};
use crate::pins::DEFAULT_REGION;

const PAGE_SIZE: usize = 200;
const MAX_PAGES: usize = 3;
const MENTION_TYPES: [&str; 2] = ["mention", "mentionInCommunity"];

/// Who an @mention addressed, as Teams splits them in `ChannelActivitiesPublisher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    Personal,
    Channel,
    Team,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionActivity {
    pub activity_id: String,
    pub kind: MentionKind,
    pub source_thread_id: String,
    pub source_message_id: Option<String>,
    pub timestamp_ms: i64,
    pub is_read: bool,
}

pub struct ActivityFeed<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    stream_url: String,
}

impl ActivityFeed<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        ActivityFeed::with_transport(SessionMessageTransport::new(session), DEFAULT_REGION)
    }
}

impl<T: MessageTransport> ActivityFeed<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        ActivityFeed {
            transport,
            stream_url: format!(
                "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/streams/notifications/messages"
            ),
        }
    }

    pub fn first_page_url(&self) -> String {
        format!(
            "{}?view=msnp24Equivalent|supportsMessageProperties&pageSize={PAGE_SIZE}&startTime=1",
            self.stream_url
        )
    }

    /// Newest activity pages, mentions only.
    pub async fn mentions(&self) -> Result<Vec<MentionActivity>> {
        let mut mentions = Vec::new();
        let mut next_url = Some(self.first_page_url());
        for _ in 0..MAX_PAGES {
            let Some(url) = next_url.take() else {
                break;
            };
            let answer = self.transport.send(Request::get(&url)).await?;
            ensure_success(&answer)?;
            mentions.extend(parse_mention_page(&answer.body)?);
            next_url = answer
                .body
                .pointer("/_metadata/backwardLink")
                .and_then(Value::as_str)
                .filter(|link| !link.is_empty() && *link != url)
                .map(str::to_owned);
        }
        Ok(mentions)
    }
}

pub fn parse_mention_page(body: &Value) -> Result<Vec<MentionActivity>> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("activity messages missing".into()))?;
    Ok(messages.iter().filter_map(mention_activity).collect())
}

fn object_property(properties: &Value, key: &str) -> Option<Value> {
    match properties.get(key)? {
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_object),
        other if other.is_object() => Some(other.clone()),
        _ => None,
    }
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn flag(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => text.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn timestamp_ms(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse::<i64>().ok().or_else(|| {
            DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|time| time.timestamp_millis())
        }),
        _ => None,
    }
}

fn mention_kind(subtype: &str) -> Option<MentionKind> {
    match subtype.to_ascii_lowercase().as_str() {
        "person" | "tag" => Some(MentionKind::Personal),
        "channel" => Some(MentionKind::Channel),
        "team" => Some(MentionKind::Team),
        _ => None,
    }
}

fn mention_activity(message: &Value) -> Option<MentionActivity> {
    let properties = message.get("properties")?;
    let activity = object_property(properties, "activity")?;
    let activity_type = activity.get("activityType")?.as_str()?;
    if !MENTION_TYPES.contains(&activity_type) {
        return None;
    }
    let kind = mention_kind(activity.get("activitySubtype")?.as_str()?)?;
    Some(MentionActivity {
        activity_id: activity
            .get("activityId")
            .and_then(text_of)
            .or_else(|| message.get("id").and_then(text_of))?,
        kind,
        source_thread_id: activity.get("sourceThreadId")?.as_str()?.to_owned(),
        source_message_id: activity.get("sourceMessageId").and_then(text_of),
        timestamp_ms: activity
            .get("activityTimestamp")
            .and_then(timestamp_ms)
            .unwrap_or(0),
        is_read: flag(properties.get("isread")),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use serde_json::json;
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

    fn activity_message(id: &str, activity: Value, isread: Option<&str>) -> Value {
        let mut properties = json!({"activity": activity});
        if let Some(isread) = isread {
            properties["isread"] = json!(isread);
        }
        json!({"id": id, "messagetype": "Text", "properties": properties})
    }

    #[test]
    fn keeps_mentions_and_maps_the_subtypes() {
        let body = json!({"messages": [
            activity_message("m1", json!({
                "activityId": 101, "activityType": "mention", "activitySubtype": "person",
                "sourceThreadId": "19:a@thread.tacv2", "sourceMessageId": 1791368000000_i64,
                "activityTimestamp": "2026-10-17T08:00:00.000Z"
            }), Some("true")),
            activity_message("m2", json!({
                "activityType": "mention", "activitySubtype": "tag",
                "sourceThreadId": "19:a@thread.tacv2", "activityTimestamp": 1791368100000_i64
            }), None),
            activity_message("m3", json!({
                "activityId": "103", "activityType": "mention", "activitySubtype": "channel",
                "sourceThreadId": "19:b@thread.tacv2"
            }), Some("false")),
            activity_message("m4", json!({
                "activityId": "104", "activityType": "mentionInCommunity", "activitySubtype": "team",
                "sourceThreadId": "19:b@thread.tacv2"
            }), None),
            activity_message("m5", json!({
                "activityId": "105", "activityType": "reply", "activitySubtype": "person",
                "sourceThreadId": "19:b@thread.tacv2"
            }), None),
            activity_message("m6", json!({
                "activityId": "106", "activityType": "mention", "activitySubtype": "everyone",
                "sourceThreadId": "19:b@thread.tacv2"
            }), None),
            {"id": "m7", "properties": {}}
        ]});
        let mentions = parse_mention_page(&body).unwrap();
        let kinds: Vec<(&str, MentionKind)> = mentions
            .iter()
            .map(|mention| (mention.activity_id.as_str(), mention.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("101", MentionKind::Personal),
                ("m2", MentionKind::Personal),
                ("103", MentionKind::Channel),
                ("104", MentionKind::Team),
            ]
        );
        assert!(mentions[0].is_read);
        assert!(!mentions[1].is_read);
        assert_eq!(
            mentions[0].source_message_id.as_deref(),
            Some("1791368000000")
        );
        assert_eq!(mentions[0].timestamp_ms, 1792224000000);
        assert_eq!(mentions[1].timestamp_ms, 1791368100000);
        assert_eq!(mentions[2].timestamp_ms, 0);
    }

    #[test]
    fn reads_the_activity_when_it_is_a_json_string() {
        let activity = json!({
            "activityId": "7", "activityType": "mention", "activitySubtype": "person",
            "sourceThreadId": "19:a@thread.tacv2"
        });
        let body = json!({"messages": [
            activity_message("m1", Value::String(activity.to_string()), Some("True"))
        ]});
        let mentions = parse_mention_page(&body).unwrap();
        assert_eq!(mentions.len(), 1);
        assert!(mentions[0].is_read);
    }

    #[test]
    fn an_answer_without_messages_is_unexpected() {
        assert!(parse_mention_page(&json!({})).is_err());
    }

    #[tokio::test]
    async fn pages_backward_through_the_stream() {
        let transport = Canned {
            answers: Mutex::new(
                vec![
                    (
                        200,
                        json!({"messages": [], "_metadata": {"backwardLink": "https://next"}}),
                    ),
                    (200, json!({"messages": [], "_metadata": {"backwardLink": ""}})),
                ]
                .into(),
            ),
            requests: Mutex::new(Vec::new()),
        };
        let feed = ActivityFeed::with_transport(&transport, "emea");
        assert!(feed.mentions().await.unwrap().is_empty());
        let requests = transport.requests.lock().unwrap();
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/streams/notifications/messages?view=msnp24Equivalent|supportsMessageProperties&pageSize=200&startTime=1"
        );
        assert_eq!(requests[1].url, "https://next");
        assert_eq!(requests.len(), 2);
    }
}
