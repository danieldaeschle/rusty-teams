use std::collections::BTreeMap;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use session::{Method, Request, Session};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::DEFAULT_REGION;

pub const ACTIVITY_FEED_ID: &str = "48:notifications";
const PAGE_SIZE: usize = 200;
const MAX_PAGES: usize = 3;
const READ_PROPERTY: &str = "isread";
const SEEN_PROPERTY: &str = "consumptionhorizon";
const BOOKMARK_PROPERTY: &str = "consumptionHorizonBookmark";
const MEETING_THREAD_PREFIX: &str = "19:meeting_";
const MEETING_THREAD_SUFFIX: &str = "@thread.v2";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActivityItem {
    pub message_id: String,
    pub activity_id: String,
    pub activity_type: String,
    pub activity_subtype: String,
    pub source_thread_id: String,
    pub source_message_id: Option<String>,
    pub source_reply_chain_id: Option<String>,
    pub source_user_id: Option<String>,
    pub source_user_name: String,
    pub source_app_name: Option<String>,
    pub preview: String,
    pub preview_template: String,
    pub thread_topic: Option<String>,
    pub thread_type: Option<String>,
    pub count: u32,
    pub timestamp_ms: i64,
    pub version_ms: i64,
    pub read_flag: bool,
    pub tap_url: Option<String>,
    pub meeting_thread_id: Option<String>,
    #[serde(default)]
    pub template_parameters: BTreeMap<String, String>,
    pub event_start_ms: Option<i64>,
    pub event_end_ms: Option<i64>,
}

/// `seen_ms`: the bell horizon (`consumptionhorizon`), `bookmark_ms`: Mark all as read (`consumptionHorizonBookmark`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActivityHorizons {
    pub seen_ms: i64,
    pub bookmark_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActivitySnapshot {
    pub items: Vec<ActivityItem>,
    pub horizons: ActivityHorizons,
}

impl ActivitySnapshot {
    /// Teams `isActivityRead` (module 804106): the item flag, or the Mark all as read bookmark at or after its version.
    pub fn is_read(&self, item: &ActivityItem) -> bool {
        item.read_flag || self.horizons.bookmark_ms >= item.version_ms
    }

    /// Teams `getUnreadMetadataInfo`: unread and newer than the bell horizon.
    pub fn counts_for_bell(&self, item: &ActivityItem) -> bool {
        !self.is_read(item) && item.timestamp_ms > self.horizons.seen_ms
    }

    pub fn mark_read(&mut self, message_id: &str) -> bool {
        let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.message_id == message_id && !item.read_flag)
        else {
            return false;
        };
        item.read_flag = true;
        true
    }

    pub fn mark_all_read(&mut self, now_ms: i64) {
        self.horizons.bookmark_ms = self.horizons.bookmark_ms.max(now_ms);
    }
}

pub struct ActivityFeed<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    conversation_url: String,
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
            conversation_url: format!(
                "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/conversations/{}",
                encode(ACTIVITY_FEED_ID)
            ),
        }
    }

    pub fn first_page_url(&self) -> String {
        format!(
            "{}/messages?view=msnp24Equivalent|supportsMessageProperties&pageSize={PAGE_SIZE}&startTime=1",
            self.conversation_url
        )
    }

    pub async fn snapshot(&self) -> Result<ActivitySnapshot> {
        let answer = self
            .transport
            .send(Request::get(&self.conversation_url))
            .await?;
        ensure_success(&answer)?;
        let horizons = parse_horizons(&answer.body);
        let mut items = Vec::new();
        let mut next_url = Some(self.first_page_url());
        for _ in 0..MAX_PAGES {
            let Some(url) = next_url.take() else {
                break;
            };
            let answer = self.transport.send(Request::get(&url)).await?;
            ensure_success(&answer)?;
            items.extend(parse_page(&answer.body)?);
            next_url = answer
                .body
                .pointer("/_metadata/backwardLink")
                .and_then(Value::as_str)
                .filter(|link| !link.is_empty() && *link != url)
                .map(str::to_owned);
        }
        items.sort_by_key(|item| std::cmp::Reverse(item.timestamp_ms));
        Ok(ActivitySnapshot { items, horizons })
    }

    /// Teams `markReadStatus`: the `isread` property on the activity message.
    pub async fn mark_read(&self, message_id: &str) -> Result<()> {
        let url = format!(
            "{}/messages/{}/properties?name={READ_PROPERTY}",
            self.conversation_url,
            encode(message_id)
        );
        let body = json!({ READ_PROPERTY: "true" });
        let answer = self
            .transport
            .send(Request::with_body(Method::Put, url, body))
            .await?;
        ensure_success(&answer)
    }

    /// Teams `setConsumptionHorizonBookmark`: everything up to now counts as read.
    pub async fn mark_all_read(&self, now_ms: i64) -> Result<()> {
        let url = format!(
            "{}/properties?name={BOOKMARK_PROPERTY}",
            self.conversation_url
        );
        let body = json!({ BOOKMARK_PROPERTY: format!("0;{now_ms};0") });
        let answer = self
            .transport
            .send(Request::with_body(Method::Put, url, body))
            .await?;
        ensure_success(&answer)
    }
}

pub fn parse_horizons(conversation: &Value) -> ActivityHorizons {
    let properties = &conversation["properties"];
    let seen = horizon_parts(properties[SEEN_PROPERTY].as_str());
    let bookmark = horizon_parts(properties[BOOKMARK_PROPERTY].as_str());
    ActivityHorizons {
        seen_ms: seen.map_or(0, |(arrival, _)| arrival),
        bookmark_ms: bookmark.map_or(0, |(arrival, timestamp)| {
            if arrival == 0 { timestamp } else { arrival }
        }),
    }
}

fn horizon_parts(text: Option<&str>) -> Option<(i64, i64)> {
    let mut parts = text?.split(';');
    let arrival = parts.next()?.trim().parse().ok()?;
    let timestamp = parts.next()?.trim().parse().ok()?;
    Some((arrival, timestamp))
}

pub fn parse_page(body: &Value) -> Result<Vec<ActivityItem>> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("activity messages missing".into()))?;
    Ok(messages.iter().filter_map(activity_item).collect())
}

fn activity_item(message: &Value) -> Option<ActivityItem> {
    let properties = message.get("properties")?;
    if properties.get("deletetime").is_some_and(|value| !value.is_null()) {
        return None;
    }
    let activity = object_property(properties, "activity")?;
    let text = |key: &str| activity.get(key).and_then(text_of);
    let context = activity.get("activityContext");
    let location = context
        .and_then(|context| object_property(context, "location"))
        .and_then(|location| location.get("Custom").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
    let message_id = message.get("id").and_then(text_of)?;
    let version_ms = message
        .get("version")
        .and_then(timestamp_ms)
        .unwrap_or_default();
    let timestamp_ms = activity
        .get("activityTimestamp")
        .and_then(timestamp_ms)
        .or_else(|| message.get("originalarrivaltime").and_then(timestamp_ms))
        .unwrap_or(version_ms);
    Some(ActivityItem {
        activity_id: text("activityId").unwrap_or_else(|| message_id.clone()),
        message_id,
        activity_type: text("activityType")?,
        activity_subtype: text("activitySubtype").unwrap_or_default(),
        source_thread_id: text("sourceThreadId").unwrap_or_default(),
        source_message_id: text("sourceMessageId"),
        source_reply_chain_id: text("sourceReplyChainId"),
        source_user_id: text("sourceUserId"),
        source_user_name: text("sourceUserImDisplayName").unwrap_or_default(),
        source_app_name: text("sourceAppDisplayName"),
        preview: text("messagePreview").unwrap_or_default(),
        preview_template: text("messagePreviewTemplateOption").unwrap_or_default(),
        thread_topic: text("sourceThreadTopic"),
        thread_type: text("sourceThreadType"),
        count: text("count")
            .and_then(|count| count.parse().ok())
            .unwrap_or(1),
        timestamp_ms,
        version_ms,
        read_flag: flag(properties.get(READ_PROPERTY)),
        tap_url: text("customTapAction"),
        meeting_thread_id: meeting_thread_in(&activity.to_string()),
        template_parameters: context
            .and_then(|context| object_property(context, "templateParameters"))
            .and_then(|parameters| parameters.as_object().cloned())
            .map(|parameters| {
                parameters
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), text_of(value)?)))
                    .collect()
            })
            .unwrap_or_default(),
        event_start_ms: tag_time(&location, "StartDateTime"),
        event_end_ms: tag_time(&location, "EndDateTime"),
    })
}

fn tag_time(text: &str, tag: &str) -> Option<i64> {
    let open = format!("<{tag}>");
    let start = text.find(&open)? + open.len();
    let end = start + text[start..].find(&format!("</{tag}>"))?;
    timestamp_ms(&Value::String(text[start..end].to_owned()))
}

fn meeting_thread_in(text: &str) -> Option<String> {
    let decoded = text
        .replace("%3a", ":")
        .replace("%3A", ":")
        .replace("%40", "@");
    let start = decoded.find(MEETING_THREAD_PREFIX)?;
    let rest = &decoded[start..];
    let end = rest.find(MEETING_THREAD_SUFFIX)? + MEETING_THREAD_SUFFIX.len();
    let thread_id = &rest[..end];
    thread_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "_:@.-=".contains(character))
        .then(|| thread_id.to_owned())
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
    crate::json_id::id_text(value)
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    const BASE: &str =
        "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Anotifications";

    struct Canned {
        answers: Mutex<VecDeque<(u16, Value)>>,
        requests: Mutex<Vec<Request>>,
    }

    impl Canned {
        fn new(answers: Vec<(u16, Value)>) -> Self {
            Canned {
                answers: Mutex::new(answers.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
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

    fn message(id: &str, version: i64, activity: Value, isread: &str) -> Value {
        json!({
            "id": id,
            "version": version.to_string(),
            "originalarrivaltime": "2026-10-17T08:00:00.000Z",
            "properties": {"activity": activity.to_string(), "isread": isread}
        })
    }

    #[test]
    fn maps_an_activity_message() {
        let body = json!({"messages": [
            message("1791368000001", 1791368000500, json!({
                "activityId": 4711, "activityType": "reactionInChat", "activitySubtype": "heart",
                "activityTimestamp": "1791368000000", "sourceThreadId": "19:a@thread.v2",
                "sourceMessageId": 1791367000000.0, "sourceReplyChainId": "1791367000000",
                "sourceUserId": "8:orgid:user-1", "sourceUserImDisplayName": "Ada Example",
                "messagePreview": "Ship it", "messagePreviewTemplateOption": "hasText",
                "sourceThreadTopic": "Release", "sourceThreadType": "chat", "count": "3"
            }), "false"),
            {"id": "2", "version": "1", "properties": {}},
            {"id": "3", "version": "1", "properties": {"deletetime": "5", "activity": {"activityType": "mention"}}}
        ]});
        let items = parse_page(&body).unwrap();
        assert_eq!(
            items,
            [ActivityItem {
                message_id: "1791368000001".into(),
                activity_id: "4711".into(),
                activity_type: "reactionInChat".into(),
                activity_subtype: "heart".into(),
                source_thread_id: "19:a@thread.v2".into(),
                source_message_id: Some("1791367000000".into()),
                source_reply_chain_id: Some("1791367000000".into()),
                source_user_id: Some("8:orgid:user-1".into()),
                source_user_name: "Ada Example".into(),
                source_app_name: None,
                preview: "Ship it".into(),
                preview_template: "hasText".into(),
                thread_topic: Some("Release".into()),
                thread_type: Some("chat".into()),
                count: 3,
                timestamp_ms: 1791368000000,
                version_ms: 1791368000500,
                read_flag: false,
                tap_url: None,
                meeting_thread_id: None,
                ..ActivityItem::default()
            }]
        );
    }

    #[test]
    fn an_app_activity_keeps_its_tap_link_and_the_meeting_it_names() {
        let body = json!({"messages": [
            message("1", 1, json!({
                "activityType": "msGraph", "activitySubtype": "recapIsReadyV2",
                "customTapAction": "https://teams.microsoft.com/l/entity/x?context=%7B%22threadId%22%3A%2219%3Ameeting_Zm9v%40thread.v2%22%7D",
            }), "false"),
            message("2", 1, json!({
                "activityType": "msGraph", "activitySubtype": "recapIsReadyV2",
                "activityContext": {"templateParameters": "{\"threadId\":\"19:meeting_YmFy@thread.v2\"}"},
            }), "false"),
        ]});
        let items = parse_page(&body).unwrap();
        assert!(items[0].tap_url.as_deref().unwrap().starts_with("https://teams.microsoft.com/l/entity/"));
        assert_eq!(items[0].meeting_thread_id.as_deref(), Some("19:meeting_Zm9v@thread.v2"));
        assert_eq!(items[1].tap_url, None);
        assert_eq!(items[1].meeting_thread_id.as_deref(), Some("19:meeting_YmFy@thread.v2"));
    }

    #[test]
    fn an_app_activity_keeps_its_template_parameters_and_time_range() {
        let body = json!({"messages": [message("1", 1, json!({
            "activityType": "msGraph", "activitySubtype": "recapIsReadyForFollowedMeetingsV2",
            "activityContext": {
                "location": "{\"Custom\":\"<DateTimeRange><StartDateTime>2026-11-05T08:00:00.0000000Z</StartDateTime><EndDateTime>2026-11-05T09:00:00.0000000Z</EndDateTime></DateTimeRange>\"}",
                "templateParameters": "{\"subject\":\"Following: Sprint Demo \"}",
            },
        }), "false")]});
        let item = &parse_page(&body).unwrap()[0];
        assert_eq!(
            item.template_parameters.get("subject").map(String::as_str),
            Some("Following: Sprint Demo ")
        );
        assert_eq!(item.event_start_ms, Some(1793865600000));
        assert_eq!(item.event_end_ms, Some(1793869200000));
    }

    #[test]
    fn reads_both_horizons_of_the_feed_conversation() {
        let conversation = json!({"properties": {
            "consumptionhorizon": "1791368000000;1791368005000;1791368000000",
            "consumptionHorizonBookmark": "0;1791000000000;0"
        }});
        assert_eq!(
            parse_horizons(&conversation),
            ActivityHorizons {
                seen_ms: 1791368000000,
                bookmark_ms: 1791000000000
            }
        );
        assert_eq!(parse_horizons(&json!({})), ActivityHorizons::default());
    }

    #[test]
    fn read_and_bell_follow_the_teams_rules() {
        let item = |version_ms: i64, read_flag: bool| ActivityItem {
            version_ms,
            timestamp_ms: version_ms,
            read_flag,
            ..ActivityItem::default()
        };
        let snapshot = ActivitySnapshot {
            items: Vec::new(),
            horizons: ActivityHorizons {
                seen_ms: 300,
                bookmark_ms: 200,
            },
        };
        assert!(snapshot.is_read(&item(150, false)));
        assert!(snapshot.is_read(&item(250, true)));
        assert!(!snapshot.is_read(&item(250, false)));
        assert!(!snapshot.counts_for_bell(&item(250, false)));
        assert!(snapshot.counts_for_bell(&item(350, false)));
        assert!(!snapshot.counts_for_bell(&item(350, true)));
    }

    #[tokio::test]
    async fn snapshot_reads_the_horizons_then_pages_backward() {
        let transport = Canned::new(vec![
            (200, json!({"properties": {"consumptionhorizon": "5;6;7"}})),
            (
                200,
                json!({"messages": [message("a", 10, json!({"activityType": "mention", "activityTimestamp": "10"}), "true")],
                       "_metadata": {"backwardLink": "https://next"}}),
            ),
            (
                200,
                json!({"messages": [message("b", 20, json!({"activityType": "call", "activityTimestamp": "20"}), "false")],
                       "_metadata": {"backwardLink": ""}}),
            ),
        ]);
        let feed = ActivityFeed::with_transport(&transport, "emea");
        let snapshot = feed.snapshot().await.unwrap();
        let ids: Vec<&str> = snapshot.items.iter().map(|item| item.message_id.as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
        assert_eq!(snapshot.horizons.seen_ms, 5);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].url, BASE);
        assert_eq!(
            requests[1].url,
            format!("{BASE}/messages?view=msnp24Equivalent|supportsMessageProperties&pageSize=200&startTime=1")
        );
        assert_eq!(requests[2].url, "https://next");
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn writes_like_teams() {
        let transport = Canned::new(vec![(200, json!({})), (200, json!({}))]);
        let feed = ActivityFeed::with_transport(&transport, "emea");
        feed.mark_read("1791368000001").await.unwrap();
        feed.mark_all_read(1791368009000).await.unwrap();
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Put);
        assert_eq!(
            requests[0].url,
            format!("{BASE}/messages/1791368000001/properties?name=isread")
        );
        assert_eq!(requests[0].body, Some(json!({"isread": "true"})));
        assert_eq!(
            requests[1].url,
            format!("{BASE}/properties?name=consumptionHorizonBookmark")
        );
        assert_eq!(
            requests[1].body,
            Some(json!({"consumptionHorizonBookmark": "0;1791368009000;0"}))
        );
    }

    #[test]
    fn local_marks_follow_the_writes() {
        let mut snapshot = ActivitySnapshot {
            items: vec![ActivityItem {
                message_id: "a".into(),
                version_ms: 50,
                ..ActivityItem::default()
            }],
            horizons: ActivityHorizons::default(),
        };
        assert!(!snapshot.is_read(&snapshot.items[0]));
        assert!(snapshot.mark_read("a"));
        assert!(!snapshot.mark_read("a"));
        assert!(snapshot.is_read(&snapshot.items[0]));
        snapshot.items[0].read_flag = false;
        snapshot.mark_all_read(60);
        assert!(snapshot.is_read(&snapshot.items[0]));
    }
}
