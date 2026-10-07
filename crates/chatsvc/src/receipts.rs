use chrono::{DateTime, Utc};
use serde_json::Value;
use session::Request;

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, encode};
use crate::pins::DEFAULT_REGION;

const READ_PART: usize = 0;
const READ_AT_PART: usize = 1;
const MESSAGE_ID_PART: usize = 2;
const DISABLED_STATUSES: [u16; 2] = [403, 404];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberHorizon {
    pub member_id: String,
    pub read_until: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub message_id: Option<String>,
    pub raw_parts: Vec<String>,
}

pub fn parse_horizon(member_id: &str, horizon: &str) -> Option<MemberHorizon> {
    let raw_parts: Vec<String> = horizon
        .split(';')
        .map(|part| part.trim().to_owned())
        .collect();
    let read_until = millis_part(&raw_parts, READ_PART)?;
    Some(MemberHorizon {
        member_id: member_id.to_owned(),
        read_until,
        read_at: millis_part(&raw_parts, READ_AT_PART),
        message_id: raw_parts
            .get(MESSAGE_ID_PART)
            .filter(|part| !part.is_empty())
            .cloned(),
        raw_parts,
    })
}

fn millis_part(parts: &[String], index: usize) -> Option<DateTime<Utc>> {
    let millis: i64 = parts.get(index)?.parse().ok()?;
    DateTime::from_timestamp_millis(millis)
}

pub fn parse_horizons(body: &Value) -> Result<Vec<MemberHorizon>> {
    let entries = body
        .get("consumptionhorizons")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("no consumptionhorizons list".into()))?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let member_id = entry.get("id")?.as_str()?;
            parse_horizon(member_id, entry.get("consumptionhorizon")?.as_str()?)
        })
        .collect())
}

pub struct Receipts<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    base_url: String,
}

impl Receipts<SessionMessageTransport> {
    pub fn new(session: &session::Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &session::Session, region: &str) -> Self {
        Receipts::with_transport(SessionMessageTransport::new(session), region)
    }
}

impl<T: MessageTransport> Receipts<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        Receipts {
            transport,
            base_url: format!("https://teams.cloud.microsoft/api/chatsvc/{region}/v1/threads"),
        }
    }

    pub fn horizons_url(&self, conversation_id: &str) -> String {
        format!(
            "{}/{}/consumptionhorizons",
            self.base_url,
            encode(conversation_id)
        )
    }

    /// Empty when the service answers 403 or 404, which counts as receipts disabled.
    pub async fn consumption_horizons(&self, conversation_id: &str) -> Result<Vec<MemberHorizon>> {
        let answer = self
            .transport
            .send(Request::get(self.horizons_url(conversation_id)))
            .await?;
        if DISABLED_STATUSES.contains(&answer.status) {
            return Ok(Vec::new());
        }
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                "chatsvc",
                answer.body.clone(),
            )));
        }
        parse_horizons(&answer.body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;
    use session::ApiResponse;

    use super::*;

    struct Canned {
        status: u16,
        body: Value,
        urls: Mutex<Vec<String>>,
    }

    impl MessageTransport for Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.urls.lock().unwrap().push(request.url.clone());
            Ok(ApiResponse {
                status: self.status,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn canned(status: u16, body: Value) -> Receipts<Canned> {
        Receipts::with_transport(
            Canned {
                status,
                body,
                urls: Mutex::new(Vec::new()),
            },
            "emea",
        )
    }

    #[test]
    fn parses_three_part_horizon() {
        let horizon = parse_horizon(
            "8:orgid:a",
            "1791368146075;1791368146999;6638123456789012345",
        )
        .unwrap();
        assert_eq!(horizon.read_until.timestamp_millis(), 1791368146075);
        assert_eq!(
            horizon.read_at.map(|time| time.timestamp_millis()),
            Some(1791368146999)
        );
        assert_eq!(horizon.message_id.as_deref(), Some("6638123456789012345"));
        assert_eq!(horizon.raw_parts.len(), 3);
    }

    #[test]
    fn tolerates_short_and_padded_horizons() {
        let single = parse_horizon("m", " 1791368146075 ").unwrap();
        assert_eq!(single.read_at, None);
        assert_eq!(single.message_id, None);
        let two = parse_horizon("m", "1791368146075;;").unwrap();
        assert_eq!(two.read_at, None);
        assert_eq!(two.message_id, None);
    }

    #[test]
    fn rejects_unparseable_horizons() {
        assert!(parse_horizon("m", "").is_none());
        assert!(parse_horizon("m", "abc;1;2").is_none());
        assert!(parse_horizon("m", ";1;2").is_none());
        assert!(parse_horizon("m", "99999999999999999999;1;2").is_none());
    }

    #[test]
    fn parses_body_and_skips_broken_members() {
        let body = json!({
            "id": "19:x@thread.v2",
            "version": "1",
            "consumptionhorizons": [
                {"id": "8:orgid:a", "consumptionhorizon": "1;2;3", "messageVisibilityTime": 0},
                {"id": "8:orgid:b", "consumptionhorizon": ""},
                {"id": "8:orgid:c"},
                {"consumptionhorizon": "1;2;3"},
                {"id": "8:orgid:d", "consumptionhorizon": "5;6;7"}
            ]
        });
        let horizons = parse_horizons(&body).unwrap();
        let ids: Vec<&str> = horizons
            .iter()
            .map(|entry| entry.member_id.as_str())
            .collect();
        assert_eq!(ids, ["8:orgid:a", "8:orgid:d"]);
    }

    #[test]
    fn body_without_list_is_unexpected() {
        assert!(matches!(
            parse_horizons(&json!({"id": "x"})),
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    #[tokio::test]
    async fn fetches_with_encoded_thread_id() {
        let receipts = canned(
            200,
            json!({"consumptionhorizons": [{"id": "8:orgid:a", "consumptionhorizon": "1;2;3"}]}),
        );
        let horizons = receipts
            .consumption_horizons("19:abc@thread.v2")
            .await
            .unwrap();
        assert_eq!(horizons.len(), 1);
        assert_eq!(
            receipts.transport.urls.lock().unwrap()[0],
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/threads/19%3Aabc%40thread.v2/consumptionhorizons"
        );
    }

    #[tokio::test]
    async fn forbidden_and_missing_mean_disabled() {
        for status in [403, 404] {
            let receipts = canned(status, json!({}));
            assert!(receipts.consumption_horizons("c").await.unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn other_failures_are_errors() {
        let receipts = canned(500, json!({"message": "boom"}));
        assert!(receipts.consumption_horizons("c").await.is_err());
    }
}
