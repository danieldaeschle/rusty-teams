use serde_json::{Value, json};
use session::{Method, Request, Session};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::DEFAULT_REGION;

const ORGID_PREFIX: &str = "8:orgid:";
const MEMBER_ROLE: &str = "Admin";
const DAY_MS: i64 = 86_400_000;
const TOPIC_PROPERTY: &str = "topic";
pub const MAX_TOPIC_LENGTH: usize = 250;
pub const INVALID_TOPIC_CHARACTER: char = ':';
pub const MAX_HISTORY_DAYS: u32 = 999;

/// How much chat history new members see, as the Teams add-people dialog offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryShare {
    #[default]
    None,
    Days(u32),
    All,
}

impl HistoryShare {
    /// The `shareHistoryTime` Teams sends: -1 for none, 0 for all, else the start in epoch ms.
    pub fn share_history_time(self, now_ms: i64) -> i64 {
        match self {
            HistoryShare::None => -1,
            HistoryShare::All => 0,
            HistoryShare::Days(days) => now_ms - i64::from(days) * DAY_MS,
        }
    }
}

pub fn user_mri(user_id: &str) -> String {
    if user_id.contains(':') {
        user_id.to_owned()
    } else {
        format!("{ORGID_PREFIX}{user_id}")
    }
}

pub fn threads_url(region: &str) -> String {
    format!("https://teams.cloud.microsoft/api/chatsvc/{region}/v1/threads")
}

pub fn add_members_body(user_ids: &[String], share: HistoryShare, now_ms: i64) -> Value {
    let share_history_time = share.share_history_time(now_ms);
    let members: Vec<Value> = user_ids
        .iter()
        .map(|user_id| {
            json!({
                "id": user_mri(user_id),
                "role": MEMBER_ROLE,
                "shareHistoryTime": share_history_time,
            })
        })
        .collect();
    json!({ "members": members })
}

pub fn add_members_request(
    region: &str,
    thread_id: &str,
    user_ids: &[String],
    share: HistoryShare,
    now_ms: i64,
) -> Request {
    Request::with_body(
        Method::Put,
        format!("{}/{}/members", threads_url(region), encode(thread_id)),
        add_members_body(user_ids, share, now_ms),
    )
}

pub fn remove_member_request(region: &str, thread_id: &str, user_id: &str) -> Request {
    Request::delete(format!(
        "{}/{}/members/{}",
        threads_url(region),
        encode(thread_id),
        user_mri(user_id)
    ))
}

pub fn create_group_request(region: &str, user_ids: &[String], topic: Option<&str>) -> Request {
    let members: Vec<Value> = user_ids
        .iter()
        .map(|user_id| json!({"id": user_mri(user_id), "role": MEMBER_ROLE}))
        .collect();
    let mut properties = json!({
        "threadType": "chat",
        "fixedRoster": false,
        "uniquerosterthread": false,
        "chatFilesIndexId": "2",
    });
    if let Some(topic) = topic.map(str::trim).filter(|topic| !topic.is_empty()) {
        properties["topic"] = json!(topic);
    }
    Request::with_body(
        Method::Post,
        threads_url(region),
        json!({"members": members, "properties": properties}),
    )
}

/// 1:1 chat (person or bot): unique roster so Teams can address it as `groupChat` on a call.
pub fn create_one_to_one_request(region: &str, member_mris: &[String]) -> Request {
    let members: Vec<Value> = member_mris
        .iter()
        .map(|mri| json!({"id": user_mri(mri), "role": MEMBER_ROLE}))
        .collect();
    Request::with_body(
        Method::Post,
        threads_url(region),
        json!({
            "members": members,
            "properties": {
                "threadType": "chat",
                "fixedRoster": true,
                "uniquerosterthread": true,
                "chatFilesIndexId": "2",
            },
        }),
    )
}

fn created_thread_id(body: &Value) -> Option<String> {
    if let Some(id) = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("19:"))
    {
        return Some(id.to_owned());
    }
    if let Some(id) = body
        .get("threadId")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("19:"))
    {
        return Some(id.to_owned());
    }
    let location = body.get("location")?.as_str()?;
    let last = location.split('?').next()?.rsplit('/').next()?;
    let decoded = percent_encoding::percent_decode_str(last)
        .decode_utf8()
        .ok()?;
    decoded.starts_with("19:").then(|| decoded.into_owned())
}

/// Teams renames a group chat by setting the thread property `topic`; an empty topic removes the name.
pub fn topic_request(region: &str, thread_id: &str, topic: &str) -> Request {
    Request::with_body(
        Method::Put,
        format!(
            "{}/{}/properties?name={TOPIC_PROPERTY}",
            threads_url(region),
            encode(thread_id)
        ),
        json!({ TOPIC_PROPERTY: topic }),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicProblem {
    TooLong,
    InvalidCharacter,
}

pub fn topic_problem(topic: &str) -> Option<TopicProblem> {
    if topic.chars().count() > MAX_TOPIC_LENGTH {
        Some(TopicProblem::TooLong)
    } else if topic.contains(INVALID_TOPIC_CHARACTER) {
        Some(TopicProblem::InvalidCharacter)
    } else {
        None
    }
}

pub struct ThreadMembers<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    region: String,
}

impl ThreadMembers<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        ThreadMembers::with_transport(SessionMessageTransport::new(session), region)
    }
}

impl<T: MessageTransport> ThreadMembers<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        ThreadMembers {
            transport,
            region: region.to_owned(),
        }
    }

    pub async fn add(
        &self,
        thread_id: &str,
        user_ids: &[String],
        share: HistoryShare,
        now_ms: i64,
    ) -> Result<()> {
        let request = add_members_request(&self.region, thread_id, user_ids, share, now_ms);
        ensure_success(&self.transport.send(request).await?)
    }

    pub async fn remove(&self, thread_id: &str, user_id: &str) -> Result<()> {
        let request = remove_member_request(&self.region, thread_id, user_id);
        ensure_success(&self.transport.send(request).await?)
    }

    /// Teams' `createThread` for a group chat; answers with the new thread id.
    pub async fn create_group(&self, user_ids: &[String], topic: Option<&str>) -> Result<String> {
        let request = create_group_request(&self.region, user_ids, topic);
        let answer = self.transport.send(request).await?;
        ensure_success(&answer)?;
        created_thread_id(&answer.body)
            .ok_or_else(|| Error::UnexpectedAnswer("thread creation has no location".into()))
    }

    /// Teams' `createThread` for a 1:1 unique-roster chat; answers with the new thread id.
    pub async fn create_one_to_one(&self, member_mris: &[String]) -> Result<String> {
        let request = create_one_to_one_request(&self.region, member_mris);
        let answer = self.transport.send(request).await?;
        ensure_success(&answer)?;
        created_thread_id(&answer.body).ok_or_else(|| {
            Error::UnexpectedAnswer(format!(
                "thread creation has no location (HTTP {} {})",
                answer.status, answer.body
            ))
        })
    }

    pub async fn set_topic(&self, thread_id: &str, topic: &str) -> Result<()> {
        let request = topic_request(&self.region, thread_id, topic);
        ensure_success(&self.transport.send(request).await?)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_group_thread_lists_everyone_as_admin_and_reads_its_id_from_the_location() {
        let request = create_group_request("emea", &["me".into(), "ann".into()], Some(" Plan "));
        assert_eq!(
            request.url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/threads"
        );
        let body = request.body.unwrap();
        assert_eq!(
            body["members"][1],
            json!({"id": "8:orgid:ann", "role": "Admin"})
        );
        assert_eq!(body["properties"]["topic"], "Plan");
        assert_eq!(
            created_thread_id(&json!({"location": "https://emea.ng.msg.teams.microsoft.com/v1/threads/19%3Aabc%40thread.v2"})).as_deref(),
            Some("19:abc@thread.v2")
        );
        assert_eq!(
            created_thread_id(&json!({"location": "https://x/v1/threads/oops"})),
            None
        );
        assert_eq!(
            created_thread_id(&json!({"id": "19:me_bot@unq.gbl.spaces"})).as_deref(),
            Some("19:me_bot@unq.gbl.spaces")
        );
        assert_eq!(
            created_thread_id(&json!({"threadId": "19:me_bot@unq.gbl.spaces"})).as_deref(),
            Some("19:me_bot@unq.gbl.spaces")
        );
    }

    #[test]
    fn a_one_to_one_thread_is_a_unique_roster_chat() {
        let request = create_one_to_one_request(
            "emea",
            &[
                "me".into(),
                "28:cf28171e-fcfd-47e4-a1d6-79460b0b3ca0".into(),
            ],
        );
        let body = request.body.unwrap();
        assert_eq!(body["properties"]["fixedRoster"], true);
        assert_eq!(body["properties"]["uniquerosterthread"], true);
        assert_eq!(
            body["members"][0],
            json!({"id": "8:orgid:me", "role": "Admin"})
        );
        assert_eq!(
            body["members"][1],
            json!({"id": "28:cf28171e-fcfd-47e4-a1d6-79460b0b3ca0", "role": "Admin"})
        );
    }

    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    #[test]
    fn topic_is_put_as_the_thread_property() {
        let request = topic_request("emea", THREAD, "Release crew");
        assert_eq!(request.method, Method::Put);
        assert_eq!(
            request.url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/threads/19%3Aabc%40thread.v2/properties?name=topic"
        );
        assert_eq!(request.body, Some(json!({"topic": "Release crew"})));
        assert_eq!(
            topic_request("emea", THREAD, "").body,
            Some(json!({"topic": ""}))
        );
    }

    #[test]
    fn topic_rules_follow_teams() {
        assert_eq!(topic_problem("Release crew"), None);
        assert_eq!(topic_problem(&"a".repeat(MAX_TOPIC_LENGTH)), None);
        assert_eq!(
            topic_problem(&"a".repeat(MAX_TOPIC_LENGTH + 1)),
            Some(TopicProblem::TooLong)
        );
        assert_eq!(
            topic_problem("Team: ops"),
            Some(TopicProblem::InvalidCharacter)
        );
    }

    const NOW: i64 = 1_792_200_000_000;
    const THREAD: &str = "19:abc@thread.v2";

    struct Recorder {
        status: u16,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for &Recorder {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: self.status,
                body: Value::Null,
                retry_after: None,
            })
        }
    }

    fn recorder(status: u16) -> Recorder {
        Recorder {
            status,
            requests: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn history_share_maps_to_the_teams_share_history_time() {
        assert_eq!(HistoryShare::None.share_history_time(NOW), -1);
        assert_eq!(HistoryShare::All.share_history_time(NOW), 0);
        assert_eq!(
            HistoryShare::Days(3).share_history_time(NOW),
            NOW - 3 * 86_400_000
        );
    }

    #[test]
    fn add_request_puts_every_member_with_role_and_history() {
        let request = add_members_request(
            "emea",
            THREAD,
            &["user-a".to_owned(), "8:orgid:user-b".to_owned()],
            HistoryShare::Days(1),
            NOW,
        );
        assert_eq!(request.method, Method::Put);
        assert_eq!(
            request.url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/threads/19%3Aabc%40thread.v2/members"
        );
        assert_eq!(
            request.body,
            Some(json!({"members": [
                {"id": "8:orgid:user-a", "role": "Admin", "shareHistoryTime": NOW - 86_400_000},
                {"id": "8:orgid:user-b", "role": "Admin", "shareHistoryTime": NOW - 86_400_000},
            ]}))
        );
    }

    #[test]
    fn remove_request_deletes_the_member_mri() {
        let request = remove_member_request("emea", THREAD, "user-a");
        assert_eq!(request.method, Method::Delete);
        assert_eq!(
            request.url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/threads/19%3Aabc%40thread.v2/members/8:orgid:user-a"
        );
        assert_eq!(request.body, None);
    }

    #[tokio::test]
    async fn add_sends_one_request_and_reports_failures() {
        let ok = recorder(200);
        ThreadMembers::with_transport(&ok, "emea")
            .add(THREAD, &["user-a".to_owned()], HistoryShare::All, NOW)
            .await
            .unwrap();
        assert_eq!(ok.requests.lock().unwrap().len(), 1);
        let refused = recorder(403);
        let result = ThreadMembers::with_transport(&refused, "emea")
            .remove(THREAD, "user-a")
            .await;
        assert!(result.is_err());
    }
}
