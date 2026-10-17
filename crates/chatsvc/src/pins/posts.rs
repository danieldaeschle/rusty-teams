use chrono::{DateTime, Utc};
use serde_json::Value;
use session::{Method, Request};

use super::Pins;
use super::chats::{api_error, ensure_success};
use super::pinbar::PinnedMessage;
use super::transport::CsaTransport;
use crate::error::{Error, Result};
use crate::messages::encode;

pub const PINNED_POSTS_PAGE_SIZE: usize = 20;
const MAX_PINNED_POST_PAGES: usize = 10;
const CONTINUATION_HEADER: &str = "x-ms-continuation";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedPostsPage {
    pub pins: Vec<PinnedMessage>,
    pub continuation_token: Option<String>,
}

fn pinned_time(message: &Value) -> Option<DateTime<Utc>> {
    let pinned = message.pointer("/properties/pinned")?;
    let parsed;
    let pinned = match pinned {
        Value::String(text) => {
            parsed = serde_json::from_str::<Value>(text).ok()?;
            &parsed
        }
        other => other,
    };
    let millis = match pinned.get("pinnedTime")? {
        Value::String(text) => text.parse::<i64>().ok()?,
        other => other.as_i64()?,
    };
    DateTime::from_timestamp_millis(millis)
}

fn pinned_post(message: &Value) -> Option<PinnedMessage> {
    let message_id = match message.get("id")? {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    Some(PinnedMessage {
        message_id,
        pinned_at: pinned_time(message),
        parent_id: None,
    })
}

pub fn parse_pinned_posts(body: &Value) -> Result<PinnedPostsPage> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("pinned posts messages missing".into()))?;
    Ok(PinnedPostsPage {
        pins: messages.iter().filter_map(pinned_post).collect(),
        continuation_token: body
            .get("continuationToken")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(str::to_owned),
    })
}

impl<T: CsaTransport> Pins<T> {
    pub fn channel_url(&self, team_thread_id: &str, channel_id: &str) -> String {
        format!(
            "{}/teams/{}/channels/{}",
            self.api_url,
            encode(team_thread_id),
            encode(channel_id)
        )
    }

    pub fn pinned_posts_request(
        &self,
        team_thread_id: &str,
        channel_id: &str,
        continuation_token: Option<&str>,
    ) -> Request {
        let mut request = Request::get(format!(
            "{}/pins?pageSize={PINNED_POSTS_PAGE_SIZE}",
            self.channel_url(team_thread_id, channel_id)
        ));
        if let Some(token) = continuation_token {
            request
                .headers
                .push((CONTINUATION_HEADER.to_owned(), token.to_owned()));
        }
        request
    }

    pub fn post_pin_request(
        &self,
        team_thread_id: &str,
        channel_id: &str,
        post_id: &str,
        pinned: bool,
    ) -> Request {
        let url = format!(
            "{}/posts/{post_id}/pin",
            self.channel_url(team_thread_id, channel_id)
        );
        if pinned {
            Request {
                method: Method::Post,
                ..Request::get(url)
            }
        } else {
            Request::delete(url)
        }
    }

    /// Pinned root posts in service order; only roots can be pinned while `enablePinMessageV2` is off.
    pub async fn channel_pinned_posts(
        &self,
        team_thread_id: &str,
        channel_id: &str,
    ) -> Result<Vec<PinnedMessage>> {
        let mut pins = Vec::new();
        let mut continuation_token: Option<String> = None;
        for _ in 0..MAX_PINNED_POST_PAGES {
            let request =
                self.pinned_posts_request(team_thread_id, channel_id, continuation_token.as_deref());
            let answer = self.transport.send(request).await?;
            ensure_success(&answer)?;
            let page = parse_pinned_posts(&answer.body)?;
            pins.extend(page.pins);
            match page.continuation_token {
                Some(token) if Some(&token) != continuation_token.as_ref() => {
                    continuation_token = Some(token)
                }
                _ => break,
            }
        }
        Ok(pins)
    }

    pub async fn set_post_pinned(
        &self,
        team_thread_id: &str,
        channel_id: &str,
        post_id: &str,
        pinned: bool,
    ) -> Result<()> {
        let request = self.post_pin_request(team_thread_id, channel_id, post_id, pinned);
        let answer = self.transport.send(request).await?;
        if answer.is_success() {
            Ok(())
        } else {
            Err(api_error(&answer))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use serde_json::json;
    use session::ApiResponse;

    use super::*;

    const TEAM: &str = "19:team@thread.tacv2";
    const CHANNEL: &str = "19:channel@thread.tacv2";
    const CHANNEL_URL: &str = "https://teams.cloud.microsoft/api/csa/emea/api/v1/teams/19%3Ateam%40thread.tacv2/channels/19%3Achannel%40thread.tacv2";

    struct Canned {
        answers: Mutex<VecDeque<(u16, Value)>>,
        requests: Mutex<Vec<Request>>,
    }

    impl CsaTransport for Canned {
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

    fn canned(answers: Vec<(u16, Value)>) -> Pins<Canned> {
        Pins::with_transport(
            Canned {
                answers: Mutex::new(answers.into()),
                requests: Mutex::new(Vec::new()),
            },
            "emea",
        )
    }

    #[test]
    fn parses_ids_pinned_time_and_the_continuation() {
        let body = json!({
            "messages": [
                {"id": "1791368000000", "parentMessageId": "1791368000000",
                 "properties": {"pinned": "{\"pinnedTime\":1791368500000}"}},
                {"id": 1791367000000_i64, "properties": {"pinned": {"pinnedTime": "1791367500000"}}},
                {"id": "1791366000000"},
                {"content": "no id"}
            ],
            "continuationToken": "next-page"
        });
        let page = parse_pinned_posts(&body).unwrap();
        let ids: Vec<&str> = page.pins.iter().map(|pin| pin.message_id.as_str()).collect();
        assert_eq!(ids, ["1791368000000", "1791367000000", "1791366000000"]);
        assert_eq!(
            page.pins[0].pinned_at.map(|time| time.timestamp_millis()),
            Some(1791368500000)
        );
        assert_eq!(
            page.pins[1].pinned_at.map(|time| time.timestamp_millis()),
            Some(1791367500000)
        );
        assert_eq!(page.pins[2].pinned_at, None);
        assert_eq!(page.continuation_token.as_deref(), Some("next-page"));
    }

    #[test]
    fn an_answer_without_messages_is_unexpected() {
        assert!(matches!(
            parse_pinned_posts(&json!({"continuationToken": "x"})),
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    #[test]
    fn pin_and_unpin_hit_the_post_pin_url_without_a_body() {
        let pins = canned(Vec::new());
        let pin = pins.post_pin_request(TEAM, CHANNEL, "1791368000000", true);
        assert_eq!(pin.method, Method::Post);
        assert_eq!(pin.url, format!("{CHANNEL_URL}/posts/1791368000000/pin"));
        assert_eq!(pin.body, None);
        let unpin = pins.post_pin_request(TEAM, CHANNEL, "1791368000000", false);
        assert_eq!(unpin.method, Method::Delete);
        assert_eq!(unpin.url, pin.url);
        assert_eq!(unpin.body, None);
    }

    #[tokio::test]
    async fn pinned_posts_follow_the_continuation_header() {
        let pins = canned(vec![
            (
                200,
                json!({"messages": [{"id": "3"}, {"id": "2"}], "continuationToken": "page-2"}),
            ),
            (200, json!({"messages": [{"id": "1"}]})),
        ]);
        let found = pins.channel_pinned_posts(TEAM, CHANNEL).await.unwrap();
        let ids: Vec<&str> = found.iter().map(|pin| pin.message_id.as_str()).collect();
        assert_eq!(ids, ["3", "2", "1"]);
        let requests = pins.transport.requests.lock().unwrap();
        assert_eq!(requests[0].url, format!("{CHANNEL_URL}/pins?pageSize=20"));
        assert!(requests[0].headers.is_empty());
        assert_eq!(
            requests[1].headers,
            [("x-ms-continuation".to_owned(), "page-2".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_repeated_continuation_stops_paging() {
        let pins = canned(vec![
            (200, json!({"messages": [{"id": "2"}], "continuationToken": "same"})),
            (200, json!({"messages": [{"id": "1"}], "continuationToken": "same"})),
        ]);
        let found = pins.channel_pinned_posts(TEAM, CHANNEL).await.unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(pins.transport.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn failures_are_errors() {
        let pins = canned(vec![(403, json!({})), (412, json!({}))]);
        assert!(pins.channel_pinned_posts(TEAM, CHANNEL).await.is_err());
        assert!(pins.set_post_pinned(TEAM, CHANNEL, "1", true).await.is_err());
    }

    #[tokio::test]
    async fn unpin_accepts_no_content() {
        let pins = canned(vec![(204, Value::Null)]);
        pins.set_post_pinned(TEAM, CHANNEL, "1", false).await.unwrap();
    }
}
