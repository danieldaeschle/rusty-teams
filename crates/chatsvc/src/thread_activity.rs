use chrono::{DateTime, Utc};
use serde_json::Value;
use session::Request;

use crate::error::Result;
use crate::messages::{MessageTransport, Messages, encode, ensure_success};

const MEMBER_JOINED: &str = "ThreadActivity/MemberJoined";
const TOPIC_UPDATE: &str = "ThreadActivity/TopicUpdate";
const ORGID_PREFIX: &str = "8:orgid:";
const PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadActivityKind {
    MembersJoined { member_ids: Vec<String> },
    TopicUpdated { topic: String },
}

/// System events of a meeting chat that Graph leaves out of `chats/{id}/messages`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadActivity {
    pub message_id: String,
    pub created_at: DateTime<Utc>,
    pub initiator_id: Option<String>,
    pub kind: ThreadActivityKind,
}

fn user_id(mri: &str) -> String {
    mri.strip_prefix(ORGID_PREFIX).unwrap_or(mri).to_owned()
}

fn xml_value<'a>(content: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = content.find(&open)? + open.len();
    let end = start + content[start..].find(&format!("</{tag}>"))?;
    Some(&content[start..end])
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn parse_activity(message: &Value) -> Option<ThreadActivity> {
    let message_id = crate::json_id::id_text(message.get("id")?)?;
    let content = message.get("content").and_then(Value::as_str).unwrap_or_default();
    let (initiator, kind) = match message.get("messagetype").and_then(Value::as_str)? {
        MEMBER_JOINED => {
            let detail: Value = serde_json::from_str(content).ok()?;
            let member_ids: Vec<String> = detail["members"]
                .as_array()?
                .iter()
                .filter_map(|member| member["id"].as_str().map(user_id))
                .collect();
            if member_ids.is_empty() {
                return None;
            }
            let initiator = detail["initiator"].as_str().map(user_id);
            (initiator, ThreadActivityKind::MembersJoined { member_ids })
        }
        TOPIC_UPDATE => {
            let topic = unescape(xml_value(content, "value")?.trim());
            let initiator = xml_value(content, "initiator").map(user_id);
            (initiator, ThreadActivityKind::TopicUpdated { topic })
        }
        _ => return None,
    };
    let created_at = message
        .get("composetime")
        .and_then(Value::as_str)
        .and_then(|time| DateTime::parse_from_rfc3339(time).ok())?
        .with_timezone(&Utc);
    Some(ThreadActivity {
        message_id,
        created_at,
        initiator_id: initiator,
        kind,
    })
}

pub fn parse_thread_activities(body: &Value) -> Vec<ThreadActivity> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_activity)
        .collect()
}

impl<T: MessageTransport> Messages<T> {
    pub async fn thread_activities(&self, chat_id: &str) -> Result<Vec<ThreadActivity>> {
        let url = format!(
            "{}/{}/messages?view=msnp24Equivalent&pageSize={PAGE_SIZE}",
            self.base_url,
            encode(chat_id)
        );
        let answer = self.transport.send(Request::get(url)).await?;
        ensure_success(&answer)?;
        Ok(parse_thread_activities(&answer.body))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn meeting_invites_and_topic_updates_become_activities() {
        let body = json!({"messages": [
            {"id": "3", "messagetype": "RichText/Html", "content": "hi", "composetime": "2026-11-09T16:26:16.078Z"},
            {"id": "2", "messagetype": "ThreadActivity/TopicUpdate", "composetime": "2026-10-30T09:26:48.2890000Z",
             "content": "<topicupdate><eventtime>1</eventtime><initiator>8:orgid:boss</initiator><value>Tree &amp; service</value></topicupdate>"},
            {"id": "1", "messagetype": "ThreadActivity/MemberJoined", "composetime": "2026-10-30T09:26:48.1380000Z",
             "content": "{\"initiator\":\"8:orgid:boss\",\"members\":[{\"id\":\"8:orgid:ann\",\"friendlyname\":\"\"},{\"id\":\"8:orgid:bob\"}]}"},
        ]});
        let activities = parse_thread_activities(&body);
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].message_id, "2");
        assert_eq!(activities[0].initiator_id.as_deref(), Some("boss"));
        assert_eq!(activities[0].kind, ThreadActivityKind::TopicUpdated { topic: "Tree & service".into() });
        assert_eq!(
            activities[1].kind,
            ThreadActivityKind::MembersJoined { member_ids: vec!["ann".into(), "bob".into()] }
        );
        assert_eq!(activities[1].created_at.timestamp_millis(), 1_793_352_408_138);
    }
}
