use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use chatsvc::drafts::{ScheduledDraft, ScheduledDrafts};
use chatsvc::messages::MessageTransport;
use chatsvc::{Error, Result};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use session::{ApiResponse, Method, Request};

struct Mock {
    answers: Mutex<VecDeque<(u16, Value)>>,
    requests: Mutex<Vec<Request>>,
}

impl Mock {
    fn new(answers: Vec<(u16, Value)>) -> Self {
        Mock {
            answers: Mutex::new(answers.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl MessageTransport for &Mock {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        let mut requests = self.requests.lock().unwrap();
        requests.push(request);
        let (status, mut body) = self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("mock exhausted");
        if body == json!(ECHO_LIST) {
            body = listed(&sent_client_message_id(&requests[0]));
        }
        Ok(ApiResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

fn drafts(mock: &Mock) -> ScheduledDrafts<&Mock> {
    ScheduledDrafts::with_transport(mock, "emea").with_retry_delay(Duration::ZERO)
}

const ECHO_LIST: &str = "echo the posted draft";
const BASE: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/drafts";

fn send_at() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 12, 6, 0, 0).unwrap()
}

fn listed(client_message_id: &str) -> Value {
    json!({"drafts": [{
        "id": "d1",
        "clientmessageid": client_message_id,
        "content": "<p>hi</p>",
        "innerThreadId": "19:a@thread.v2",
        "draftType": "ScheduledDraft",
        "draftDetails": {"sendAt": "1791784800000"},
        "properties": {},
    }]})
}

fn sent_client_message_id(request: &Request) -> String {
    request.body.as_ref().unwrap()["message"]["clientmessageid"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn draft() -> ScheduledDraft {
    ScheduledDraft {
        id: "d 1".to_owned(),
        client_message_id: "123".to_owned(),
        inner_thread_id: "19:a@thread.v2".to_owned(),
        send_at: send_at(),
        html: "<p>old</p>".to_owned(),
        delivery_state: None,
    }
}

#[tokio::test]
async fn create_finds_the_draft_by_client_message_id() {
    let mock = Mock::new(vec![
        (201, json!({"OriginalArrivalTime": 1})),
        (200, json!(ECHO_LIST)),
    ]);
    let created = drafts(&mock)
        .create("19:a@thread.v2", "<p>hi</p>", send_at(), "Me")
        .await
        .unwrap();
    assert_eq!(created.id, "d1");
    let requests = mock.requests();
    assert_eq!(requests[0].method, Method::Post);
    assert_eq!(requests[0].url, BASE);
    let body = requests[0].body.as_ref().unwrap();
    assert_eq!(body["draftType"], "ScheduledDraft");
    assert_eq!(body["draftDetails"]["sendAt"], "1791784800000");
    assert_eq!(body["innerThreadId"], "19:a@thread.v2");
    assert_eq!(body["message"]["content"], "<p>hi</p>");
    assert_eq!(body["message"]["messagetype"], "RichText/Html");
    assert_eq!(body["message"]["contenttype"], "text");
    assert_eq!(body["message"]["imdisplayname"], "Me");
    let client_message_id = sent_client_message_id(&requests[0]);
    assert!(client_message_id.len() <= 19 && client_message_id.chars().all(|c| c.is_ascii_digit()));
    assert_eq!(requests[1].method, Method::Get);
}

#[tokio::test]
async fn create_retries_the_lookup_with_backoff_until_the_draft_shows() {
    let mock = Mock::new(vec![
        (201, json!({})),
        (200, json!({"drafts": []})),
        (200, json!({"drafts": []})),
        (200, json!(ECHO_LIST)),
    ]);
    let created = drafts(&mock)
        .create("19:a@thread.v2", "<p>hi</p>", send_at(), "Me")
        .await
        .unwrap();
    assert_eq!(created.id, "d1");
    assert_eq!(mock.requests().len(), 4);
}

#[tokio::test]
async fn create_without_a_found_id_still_succeeds_with_an_empty_id() {
    let mock = Mock::new(vec![
        (201, json!({})),
        (200, json!({"drafts": []})),
        (200, json!({"drafts": []})),
        (200, json!({"drafts": []})),
        (200, json!({"drafts": []})),
    ]);
    let created = drafts(&mock)
        .create("19:a@thread.v2", "<p>hi</p>", send_at(), "Me")
        .await
        .unwrap();
    let requests = mock.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(created.id, "");
    assert_eq!(
        created.client_message_id,
        sent_client_message_id(&requests[0])
    );
    assert_eq!(created.inner_thread_id, "19:a@thread.v2");
    assert_eq!(created.send_at, send_at());
    assert_eq!(created.html, "<p>hi</p>");
}

#[tokio::test]
async fn list_keeps_scheduled_drafts_and_drops_tombstones() {
    let mock = Mock::new(vec![(
        200,
        json!({"_metadata": {}, "drafts": [
            {"id": "a", "clientmessageid": "1", "content": "<p>x</p>", "innerThreadId": "c",
             "draftType": "ScheduledDraft", "draftDetails": {"sendAt": "1791784800000"},
             "properties": {"deliveryState": "failed"}},
            {"id": "b", "clientmessageid": "2", "content": "<p>x</p>", "innerThreadId": "c",
             "draftType": "RegularDraft", "properties": {}},
            {"id": "c", "clientmessageid": "3", "content": "", "innerThreadId": "c",
             "draftType": "ScheduledDraft", "draftDetails": {"sendAt": "1791784800000"},
             "properties": {"deletetime": "1791700000000"}},
            {"id": "e", "clientmessageid": "4", "content": "<p>x</p>", "innerThreadId": "c",
             "draftType": "ScheduledDraft", "draftDetails": {"sendAt": "1791784800000"},
             "properties": {"deletetime": "1791700000000"}},
        ]}),
    )]);
    let now = send_at();
    let listed = drafts(&mock).list_at(now).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "a");
    assert_eq!(listed[0].send_at, send_at());
    assert_eq!(listed[0].delivery_state.as_deref(), Some("failed"));
    assert_eq!(
        mock.requests()[0].url,
        format!(
            "{BASE}?view=msnp24Equivalent&pageSize=200&startTime={}",
            (now - chrono::Duration::days(8)).timestamp_millis()
        )
    );
}

#[tokio::test]
async fn list_answers_empty_when_the_stream_does_not_exist() {
    let mock = Mock::new(vec![(404, Value::Null)]);
    assert!(drafts(&mock).list().await.unwrap().is_empty());
}

#[tokio::test]
async fn update_puts_the_draft_id_inside_the_message() {
    let mock = Mock::new(vec![(200, Value::Null)]);
    let updated = drafts(&mock)
        .update(&draft(), "<p>new</p>", "Me")
        .await
        .unwrap();
    assert_eq!(updated.html, "<p>new</p>");
    let requests = mock.requests();
    assert_eq!(requests[0].method, Method::Put);
    assert_eq!(requests[0].url, format!("{BASE}/d%201"));
    let body = requests[0].body.as_ref().unwrap();
    assert_eq!(body["message"]["id"], "d 1");
    assert_eq!(body["message"]["clientmessageid"], "123");
    assert_eq!(body["message"]["content"], "<p>new</p>");
    assert_eq!(body["draftDetails"]["sendAt"], "1791784800000");
}

#[tokio::test]
async fn cancel_deletes_the_encoded_draft_url() {
    let mock = Mock::new(vec![(200, Value::Null)]);
    drafts(&mock).cancel("d/1").await.unwrap();
    let requests = mock.requests();
    assert_eq!(requests[0].method, Method::Delete);
    assert_eq!(requests[0].url, format!("{BASE}/d%2F1"));
}

#[tokio::test]
async fn edit_retries_on_not_found_up_to_three_times() {
    let mock = Mock::new(vec![
        (404, Value::Null),
        (404, Value::Null),
        (404, Value::Null),
        (200, Value::Null),
    ]);
    drafts(&mock)
        .update(&draft(), "<p>new</p>", "Me")
        .await
        .unwrap();
    assert_eq!(mock.requests().len(), 4);

    let mock = Mock::new(vec![(404, Value::Null); 4]);
    let error = drafts(&mock)
        .update(&draft(), "<p>new</p>", "Me")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Session(_)));
    assert_eq!(mock.requests().len(), 4);
}
