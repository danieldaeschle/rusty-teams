use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{Value, json};
use session::{ApiResponse, Method, Request, Session};

use crate::error::Result;
use crate::messages::{
    DEFAULT_RETRY_DELAY, MessageTransport, SessionMessageTransport, encode, ensure_success,
};
use crate::pins::DEFAULT_REGION;

const SCHEDULED_DRAFT: &str = "ScheduledDraft";
const NOT_FOUND_RETRIES: u32 = 3;
const BACKOFF_FACTOR: u32 = 3;
const CLIENT_MESSAGE_ID_LENGTH: usize = 19;
const LIST_PAGE_SIZE: u32 = 200;
const LIST_WINDOW_DAYS: i64 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledDraft {
    pub id: String,
    pub client_message_id: String,
    pub inner_thread_id: String,
    pub send_at: DateTime<Utc>,
    pub html: String,
    pub delivery_state: Option<String>,
}

pub struct ScheduledDrafts<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    base_url: String,
    retry_delay: Duration,
}

impl ScheduledDrafts<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        ScheduledDrafts::with_transport(SessionMessageTransport::new(session), region)
    }
}

impl<T: MessageTransport> ScheduledDrafts<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        ScheduledDrafts {
            transport,
            base_url: format!(
                "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/drafts"
            ),
            retry_delay: DEFAULT_RETRY_DELAY,
        }
    }

    pub fn with_retry_delay(mut self, retry_delay: Duration) -> Self {
        self.retry_delay = retry_delay;
        self
    }

    pub fn draft_url(&self, draft_id: &str) -> String {
        format!("{}/{}", self.base_url, encode(draft_id))
    }

    pub async fn create(
        &self,
        inner_thread_id: &str,
        html: &str,
        send_at: DateTime<Utc>,
        display_name: &str,
    ) -> Result<ScheduledDraft> {
        let client_message_id = new_client_message_id();
        let body = draft_body(
            inner_thread_id,
            html,
            send_at,
            display_name,
            &client_message_id,
            None,
        );
        let answer = self
            .send_retrying_not_found(Method::Post, &self.base_url, &body)
            .await?;
        ensure_success(&answer)?;
        // The POST answers without the draft id, so it is looked up by clientmessageid.
        let mut delay = self.retry_delay;
        for attempt in 0..=NOT_FOUND_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(delay).await;
                delay *= BACKOFF_FACTOR;
            }
            if let Some(draft) = self.find(&client_message_id).await? {
                return Ok(draft);
            }
        }
        Ok(ScheduledDraft {
            id: String::new(),
            client_message_id,
            inner_thread_id: inner_thread_id.to_owned(),
            send_at,
            html: html.to_owned(),
            delivery_state: None,
        })
    }

    pub async fn list(&self) -> Result<Vec<ScheduledDraft>> {
        self.list_at(Utc::now()).await
    }

    pub async fn list_at(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledDraft>> {
        let start_time = (now - ChronoDuration::days(LIST_WINDOW_DAYS)).timestamp_millis();
        let url = format!(
            "{}?view=msnp24Equivalent&pageSize={LIST_PAGE_SIZE}&startTime={start_time}",
            self.base_url
        );
        let answer = self.transport.send(Request::get(&url)).await?;
        if answer.status == 404 {
            return Ok(Vec::new());
        }
        ensure_success(&answer)?;
        Ok(parse_drafts(&answer.body))
    }

    pub async fn update(
        &self,
        draft: &ScheduledDraft,
        html: &str,
        display_name: &str,
    ) -> Result<ScheduledDraft> {
        let body = draft_body(
            &draft.inner_thread_id,
            html,
            draft.send_at,
            display_name,
            &draft.client_message_id,
            Some(&draft.id),
        );
        let answer = self
            .send_retrying_not_found(Method::Put, &self.draft_url(&draft.id), &body)
            .await?;
        ensure_success(&answer)?;
        Ok(ScheduledDraft {
            html: html.to_owned(),
            ..draft.clone()
        })
    }

    pub async fn cancel(&self, draft_id: &str) -> Result<()> {
        let answer = self
            .transport
            .send(Request::delete(self.draft_url(draft_id)))
            .await?;
        ensure_success(&answer)
    }

    async fn find(&self, client_message_id: &str) -> Result<Option<ScheduledDraft>> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .find(|draft| draft.client_message_id == client_message_id))
    }

    async fn send_retrying_not_found(
        &self,
        method: Method,
        url: &str,
        body: &Value,
    ) -> Result<ApiResponse> {
        let mut delay = self.retry_delay;
        let mut answer = self
            .transport
            .send(Request::with_body(method, url, body.clone()))
            .await?;
        for _ in 0..NOT_FOUND_RETRIES {
            if answer.status != 404 {
                break;
            }
            tokio::time::sleep(delay).await;
            delay *= BACKOFF_FACTOR;
            answer = self
                .transport
                .send(Request::with_body(method, url, body.clone()))
                .await?;
        }
        Ok(answer)
    }
}

fn draft_body(
    inner_thread_id: &str,
    html: &str,
    send_at: DateTime<Utc>,
    display_name: &str,
    client_message_id: &str,
    draft_id: Option<&str>,
) -> Value {
    let mut message = json!({
        "content": html,
        "messagetype": "RichText/Html",
        "contenttype": "text",
        "clientmessageid": client_message_id,
        "imdisplayname": display_name,
        "properties": {},
    });
    if let Some(draft_id) = draft_id {
        message["id"] = json!(draft_id);
    }
    json!({
        "draftDetails": {"sendAt": send_at.timestamp_millis().to_string()},
        "draftType": SCHEDULED_DRAFT,
        "innerThreadId": inner_thread_id,
        "message": message,
    })
}

fn new_client_message_id() -> String {
    let random = uuid::Uuid::new_v4().as_u128();
    let first = (random >> 96) as u32;
    let second = (random >> 64) as u32;
    let mut id = format!("{first}{second}");
    id.truncate(CLIENT_MESSAGE_ID_LENGTH);
    id
}

fn parse_drafts(body: &Value) -> Vec<ScheduledDraft> {
    body.get("drafts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_draft)
        .collect()
}

fn parse_draft(draft: &Value) -> Option<ScheduledDraft> {
    if draft["draftType"].as_str()? != SCHEDULED_DRAFT {
        return None;
    }
    let properties = &draft["properties"];
    let html = draft["content"].as_str().filter(|html| !html.is_empty())?;
    if !properties["deletetime"].is_null() {
        return None;
    }
    let send_at_millis: i64 = draft["draftDetails"]["sendAt"]
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| draft["draftDetails"]["sendAt"].as_i64())?;
    Some(ScheduledDraft {
        id: draft["id"].as_str()?.to_owned(),
        client_message_id: draft["clientmessageid"].as_str()?.to_owned(),
        inner_thread_id: draft["innerThreadId"].as_str()?.to_owned(),
        send_at: DateTime::from_timestamp_millis(send_at_millis)?,
        html: html.to_owned(),
        delivery_state: properties["deliveryState"].as_str().map(str::to_owned),
    })
}
