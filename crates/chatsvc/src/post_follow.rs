use std::collections::HashMap;

use serde_json::{Value, json};
use session::{Method, Request};

use crate::error::Result;
use crate::members::user_mri;
use crate::messages::{MessageTransport, Messages, encode, ensure_success};

const FOLLOW_EMOTION: &str = "follow";
const EXPLICIT_FOLLOW: u8 = 1;
const IMPLICIT_FOLLOW: u8 = 2;
const EXPLICIT_UNFOLLOW: u8 = 3;

/// Teams' reply chain follow state (`rcMetadata.f`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostFollowState {
    ExplicitFollow,
    ImplicitFollow,
    ExplicitUnfollow,
}

impl PostFollowState {
    fn from_code(code: u64) -> Option<Self> {
        match u8::try_from(code).ok()? {
            EXPLICIT_FOLLOW => Some(PostFollowState::ExplicitFollow),
            IMPLICIT_FOLLOW => Some(PostFollowState::ImplicitFollow),
            EXPLICIT_UNFOLLOW => Some(PostFollowState::ExplicitUnfollow),
            _ => None,
        }
    }

    pub fn followed(self) -> bool {
        self != PostFollowState::ExplicitUnfollow
    }
}

pub fn parse_post_follow_states(body: &Value) -> HashMap<String, PostFollowState> {
    body.get("rcMetadataList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let root_id = crate::json_id::id_text(entry.get("rootMessageId")?)?;
            let state = PostFollowState::from_code(entry.pointer("/rcMetadata/f")?.as_u64()?)?;
            Some((root_id, state))
        })
        .collect()
}

impl<T: MessageTransport> Messages<T> {
    fn user_conversations_url(&self, user_id: &str) -> String {
        self.base_url
            .replace("/users/ME/", &format!("/users/{}/", user_mri(user_id)))
    }

    pub fn post_follow_request(
        &self,
        user_id: &str,
        channel_id: &str,
        root_id: &str,
        followed: bool,
    ) -> Request {
        let code = if followed {
            EXPLICIT_FOLLOW
        } else {
            EXPLICIT_UNFOLLOW
        };
        Request::with_body(
            Method::Put,
            format!(
                "{}/{}/rcmetadata/{}",
                self.user_conversations_url(user_id),
                encode(channel_id),
                encode(root_id)
            ),
            json!({"f": code}),
        )
    }

    pub fn post_follow_annotation_request(
        &self,
        channel_id: &str,
        root_id: &str,
        followed: bool,
    ) -> Request {
        Request::with_body(
            Method::Put,
            format!(
                "{}/{}/messages/{}/properties?name=emotions",
                self.base_url,
                encode(channel_id),
                encode(root_id)
            ),
            json!({"emotions": {"key": FOLLOW_EMOTION, "value": u8::from(followed)}}),
        )
    }

    pub fn post_follow_states_request(&self, channel_id: &str, root_ids: &[u64]) -> Request {
        Request::with_body(
            Method::Post,
            format!("{}/{}/getrcmetadata", self.base_url, encode(channel_id)),
            json!({"rootMessageIds": root_ids}),
        )
    }

    /// Teams writes the reply chain metadata first and the `follow` annotation only after it succeeded.
    pub async fn set_post_followed(
        &self,
        user_id: &str,
        channel_id: &str,
        root_id: &str,
        followed: bool,
    ) -> Result<Option<String>> {
        let answer = self
            .transport
            .send(self.post_follow_request(user_id, channel_id, root_id, followed))
            .await?;
        ensure_success(&answer)?;
        let annotation = self
            .transport
            .send(self.post_follow_annotation_request(channel_id, root_id, followed))
            .await
            .and_then(|answer| ensure_success(&answer));
        Ok(annotation.err().map(|error| error.to_string()))
    }

    pub async fn post_follow_states(
        &self,
        channel_id: &str,
        root_ids: &[String],
    ) -> Result<HashMap<String, PostFollowState>> {
        let numeric: Vec<u64> = root_ids.iter().filter_map(|id| id.parse().ok()).collect();
        if numeric.is_empty() {
            return Ok(HashMap::new());
        }
        let answer = self
            .transport
            .send(self.post_follow_states_request(channel_id, &numeric))
            .await?;
        ensure_success(&answer)?;
        Ok(parse_post_follow_states(&answer.body))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    struct Canned {
        statuses: Vec<u16>,
        body: Value,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            let mut requests = self.requests.lock().unwrap();
            let status = self.statuses[requests.len().min(self.statuses.len() - 1)];
            requests.push(request);
            Ok(ApiResponse {
                status,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn canned(status: u16, body: Value) -> Messages<Canned> {
        canned_sequence(vec![status], body)
    }

    fn canned_sequence(statuses: Vec<u16>, body: Value) -> Messages<Canned> {
        Messages::with_transport(
            Canned {
                statuses,
                body,
                requests: Mutex::new(Vec::new()),
            },
            "emea",
        )
    }

    const BASE: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users";

    #[tokio::test]
    async fn follow_writes_reply_chain_metadata_then_annotation() {
        let messages = canned(200, json!({}));
        messages
            .set_post_followed("a1", "19:c@thread.tacv2", "1791368000000", true)
            .await
            .unwrap();
        messages
            .set_post_followed("a1", "19:c@thread.tacv2", "1791368000000", false)
            .await
            .unwrap();
        let requests = messages.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Put);
        assert_eq!(
            requests[0].url,
            format!(
                "{BASE}/8:orgid:a1/conversations/19%3Ac%40thread.tacv2/rcmetadata/1791368000000"
            )
        );
        assert_eq!(requests[0].body, Some(json!({"f": 1})));
        assert_eq!(
            requests[1].url,
            format!(
                "{BASE}/ME/conversations/19%3Ac%40thread.tacv2/messages/1791368000000/properties?name=emotions"
            )
        );
        assert_eq!(
            requests[1].body,
            Some(json!({"emotions": {"key": "follow", "value": 1}}))
        );
        assert_eq!(requests[2].body, Some(json!({"f": 3})));
        assert_eq!(
            requests[3].body,
            Some(json!({"emotions": {"key": "follow", "value": 0}}))
        );
    }

    #[tokio::test]
    async fn failed_metadata_write_skips_the_annotation() {
        let messages = canned(403, json!({}));
        assert!(
            messages
                .set_post_followed("a1", "19:c", "1", true)
                .await
                .is_err()
        );
        assert_eq!(messages.transport.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn failed_annotation_still_counts_as_followed() {
        let messages = canned_sequence(vec![200, 500], json!({}));
        let annotation_failure = messages
            .set_post_followed("a1", "19:c", "1", true)
            .await
            .unwrap();
        assert!(annotation_failure.is_some());
        assert_eq!(messages.transport.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn reads_follow_states_of_root_posts() {
        let body = json!({"conversationId": "19:c", "rcMetadataList": [
            {"statusCode": 200, "rootMessageId": 11, "rcMetadata": {"lu": 1, "f": 1}},
            {"statusCode": 200, "rootMessageId": "12", "rcMetadata": {"lu": 1, "f": 3}},
            {"statusCode": 200, "rootMessageId": 13, "rcMetadata": {"lu": 1, "f": 2}},
            {"statusCode": 200, "rootMessageId": 14, "rcMetadata": {"lu": 1, "s": 1}},
            {"statusCode": 404, "rootMessageId": 15},
        ]});
        let messages = canned(207, body);
        let states = messages
            .post_follow_states("19:c", &["11".into(), "12".into(), "x".into()])
            .await
            .unwrap();
        assert_eq!(states.len(), 3);
        assert!(states["11"].followed());
        assert!(!states["12"].followed());
        assert_eq!(states["13"], PostFollowState::ImplicitFollow);
        let requests = messages.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Post);
        assert_eq!(
            requests[0].url,
            format!("{BASE}/ME/conversations/19%3Ac/getrcmetadata")
        );
        assert_eq!(requests[0].body, Some(json!({"rootMessageIds": [11, 12]})));
    }

    #[tokio::test]
    async fn no_numeric_roots_sends_nothing() {
        let messages = canned(200, json!({}));
        assert!(messages.post_follow_states("19:c", &[]).await.unwrap().is_empty());
        assert!(messages.transport.requests.lock().unwrap().is_empty());
    }
}
