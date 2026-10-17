use serde_json::{Value, json};
use session::{Method, Request, Session};

use crate::error::Result;
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::DEFAULT_REGION;

const ORGID_PREFIX: &str = "8:orgid:";
const MEMBER_ROLE: &str = "Admin";
const DAY_MS: i64 = 86_400_000;
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

pub struct ThreadMembers<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    region: String,
}

impl ThreadMembers<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        ThreadMembers::with_transport(SessionMessageTransport::new(session), DEFAULT_REGION)
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
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

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
