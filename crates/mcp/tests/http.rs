use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use mcp::{
    Access, AgentHub, AgentPolicy, Meeting, Outcome, ServerHandle, SourceError, SourceFuture,
    TeamsSource, Transcript, TranscriptTurn,
};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::service::RunningService;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Value, json};
use store::{ChannelRecord, ChatRecord, MessageRecord, Store, TeamRecord};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TOKEN: &str = "test-token";
const ALLOWED_CHAT: &str = "19:alpha@thread.v2";
const HIDDEN_CHAT: &str = "19:beta@thread.v2";
const CHANNEL: &str = "19:channel@thread.tacv2";
const MEETING_CHAT: &str = "19:meeting_abc@thread.v2";
const LIVE_MEETING_CHAT: &str = "19:meeting_live@thread.v2";
const HIDDEN_MEETING_CHAT: &str = "19:meeting_hidden@thread.v2";

struct FakeSource {
    store: Arc<Store>,
}

impl TeamsSource for FakeSource {
    fn store(&self) -> Result<Arc<Store>, SourceError> {
        Ok(self.store.clone())
    }

    fn me_id(&self) -> Option<String> {
        Some("user-me".to_owned())
    }

    fn calendar(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> SourceFuture<'_, Vec<Meeting>> {
        let meeting = Meeting {
            title: "Sprint review".to_owned(),
            start: Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 7, 10, 0, 0).unwrap(),
            all_day: false,
            organizer: Some("Ada Example".to_owned()),
            join_url: Some("https://teams.example/join".to_owned()),
            meeting_chat_id: Some(MEETING_CHAT.to_owned()),
        };
        Box::pin(async move {
            Ok(vec![meeting]
                .into_iter()
                .filter(|meeting| meeting.start >= start && meeting.start < end)
                .collect())
        })
    }

    fn transcript<'a>(&'a self, meeting_chat_id: &'a str) -> SourceFuture<'a, Transcript> {
        Box::pin(async move {
            if meeting_chat_id != MEETING_CHAT {
                return Err(SourceError::Failed(
                    "no finished recording in this meeting chat".to_owned(),
                ));
            }
            Ok(Transcript {
                recording_name: Some("Sprint review".to_owned()),
                created_at: None,
                turns: vec![TranscriptTurn {
                    speaker: "Ada Example".to_owned(),
                    start_seconds: 1.0,
                    end_seconds: 4.5,
                    text: "Welcome everyone".to_owned(),
                }],
            })
        })
    }
}

struct Harness {
    hub: Arc<AgentHub>,
    server: ServerHandle,
}

impl Harness {
    async fn start(propose: bool) -> Harness {
        Harness::start_with(propose, false, false).await
    }

    async fn start_with(propose: bool, calendar: bool, transcripts: bool) -> Harness {
        let store = Arc::new(Store::open_in_memory().unwrap());
        seed(&store);
        let policy = AgentPolicy {
            enabled: true,
            chats: Access::Selected(BTreeSet::from([
                ALLOWED_CHAT.to_owned(),
                MEETING_CHAT.to_owned(),
                LIVE_MEETING_CHAT.to_owned(),
            ])),
            channels: Access::Selected(BTreeSet::from([CHANNEL.to_owned()])),
            propose,
            calendar,
            transcripts,
            ..AgentPolicy::default()
        };
        let (ui, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let hub = AgentHub::new(store.clone(), policy, ui);
        let server = mcp::start(
            hub.clone(),
            Arc::new(FakeSource { store }),
            0,
            TOKEN.to_owned(),
        )
        .await
        .unwrap();
        Harness { hub, server }
    }

    fn url(&self) -> String {
        format!("http://{}/mcp", self.server.addr)
    }

    async fn client(&self) -> RunningService<rmcp::RoleClient, ClientConfig> {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(self.url()).auth_header(TOKEN),
        );
        ClientConfig::default().serve(transport).await.unwrap()
    }
}

fn seed(store: &Store) {
    let time = |minute: u32| Utc.with_ymd_and_hms(2026, 10, 6, 9, minute, 0).unwrap();
    let chat_of_kind = |id: &str, kind: &str, title: &str| ChatRecord {
        id: id.to_owned(),
        kind: kind.to_owned(),
        title: title.to_owned(),
        last_message_at: Some(time(5)),
        ..ChatRecord::default()
    };
    let chat = |id: &str, title: &str| chat_of_kind(id, "group", title);
    store
        .upsert_chats(&[
            chat(ALLOWED_CHAT, "Planning"),
            chat(HIDDEN_CHAT, "Secrets"),
            chat_of_kind(MEETING_CHAT, "meeting", "Sprint review"),
            chat_of_kind(LIVE_MEETING_CHAT, "meeting", "Live sync"),
            chat_of_kind(HIDDEN_MEETING_CHAT, "meeting", "Board meeting"),
        ])
        .unwrap();
    store
        .upsert_teams(&[TeamRecord {
            id: "team-1".to_owned(),
            name: "Platform".to_owned(),
        }])
        .unwrap();
    store
        .upsert_channels(&[ChannelRecord {
            id: CHANNEL.to_owned(),
            team_id: "team-1".to_owned(),
            name: "General".to_owned(),
            membership_type: None,
            last_message_at: Some(time(6)),
            unread: false,
        }])
        .unwrap();
    let message =
        |conversation_id: &str, message_id: &str, minute: u32, html: &str| MessageRecord {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            sender_id: Some("user-ada".to_owned()),
            sender_name: Some("Ada Example".to_owned()),
            created_at: time(minute),
            body_html: html.to_owned(),
            attachments_json: "[]".to_owned(),
            reactions_json: "[]".to_owned(),
            mentions_json: "[]".to_owned(),
            links_json: "[]".to_owned(),
            ..MessageRecord::default()
        };
    store
        .upsert_messages(&[
            message(ALLOWED_CHAT, "m1", 1, "<p>budget <b>plan</b></p>"),
            message(ALLOWED_CHAT, "m2", 2, "<p>lunch at noon</p>"),
            message(HIDDEN_CHAT, "m3", 3, "<p>budget secrets</p>"),
            message(CHANNEL, "m4", 4, "<p>budget channel post</p>"),
        ])
        .unwrap();
}

async fn call(
    client: &RunningService<rmcp::RoleClient, ClientConfig>,
    tool: &'static str,
    arguments: Value,
) -> (bool, String) {
    let params = CallToolRequestParams::new(tool)
        .with_arguments(arguments.as_object().cloned().unwrap_or_default());
    let result = client.call_tool(params).await.unwrap();
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|text| text.text.clone())
        .unwrap_or_default();
    (result.is_error == Some(true), text)
}

async fn raw_status(addr: std::net::SocketAddr, host: &str, authorization: Option<&str>) -> u16 {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let authorization = authorization
        .map(|value| format!("Authorization: {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\n{authorization}Content-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[tokio::test]
async fn lists_all_eight_tools() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "get_transcript",
            "list_channels",
            "list_chats",
            "list_meetings",
            "propose_message",
            "read_messages",
            "search_messages",
            "wait_for_event"
        ]
    );
    let info = client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "rusty-teams");
}

#[tokio::test]
async fn rejects_missing_token_bad_host_and_serves_healthz() {
    let harness = Harness::start(true).await;
    let addr = harness.server.addr;
    let own_host = addr.to_string();
    assert_eq!(raw_status(addr, &own_host, None).await, 401);
    assert_eq!(raw_status(addr, &own_host, Some("Bearer wrong")).await, 401);
    assert_eq!(
        raw_status(addr, "evil.example", Some("Bearer test-token")).await,
        403
    );
    let authorized = raw_status(addr, &own_host, Some("Bearer test-token")).await;
    assert!(authorized != 401 && authorized != 403, "{authorized}");

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            format!("GET /healthz HTTP/1.1\r\nHost: {own_host}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("ok"), "{response}");
}

#[tokio::test]
async fn lists_only_allowed_conversations() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (_, chats) = call(&client, "list_chats", json!({})).await;
    let chats: Value = serde_json::from_str(&chats).unwrap();
    let mut ids: Vec<&str> = chats["chats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chat| chat["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    let mut expected = vec![ALLOWED_CHAT, MEETING_CHAT, LIVE_MEETING_CHAT];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    let (_, channels) = call(&client, "list_channels", json!({})).await;
    let channels: Value = serde_json::from_str(&channels).unwrap();
    assert_eq!(channels["channels"][0]["team"], "Platform");
    assert_eq!(channels["channels"][0]["channel"], "General");
}

#[tokio::test]
async fn read_messages_respects_scope_and_paging() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (is_error, text) = call(
        &client,
        "read_messages",
        json!({"conversation_id": HIDDEN_CHAT}),
    )
    .await;
    assert!(is_error);
    assert!(
        text.contains("conversation not found or not allowed"),
        "{text}"
    );
    let (is_error, unknown) = call(
        &client,
        "read_messages",
        json!({"conversation_id": "19:nope@thread.v2"}),
    )
    .await;
    assert!(is_error);
    assert_eq!(unknown, text);

    let (is_error, body) = call(
        &client,
        "read_messages",
        json!({"conversation_id": ALLOWED_CHAT}),
    )
    .await;
    assert!(!is_error, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["messages"][0]["text"], "budget plan");
    assert_eq!(body["messages"][1]["message_id"], "m2");

    let (_, older) = call(
        &client,
        "read_messages",
        json!({"conversation_id": ALLOWED_CHAT, "before_message_id": "m2"}),
    )
    .await;
    let older: Value = serde_json::from_str(&older).unwrap();
    assert_eq!(older["messages"].as_array().unwrap().len(), 1);
    assert_eq!(older["messages"][0]["message_id"], "m1");
}

#[tokio::test]
async fn search_stays_inside_the_allowed_scope() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (is_error, body) = call(&client, "search_messages", json!({"query": "budget"})).await;
    assert!(!is_error, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    let mut message_ids: Vec<&str> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit["message_id"].as_str().unwrap())
        .collect();
    message_ids.sort();
    assert_eq!(message_ids, ["m1", "m4"]);
    assert!(!body.to_string().contains('\u{1}'));
}

#[tokio::test]
async fn proposal_resolution_reaches_wait_for_event() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (_, start) = call(&client, "wait_for_event", json!({"timeout_seconds": 1})).await;
    let start: Value = serde_json::from_str(&start).unwrap();
    let cursor = start["next"].as_u64().unwrap();

    let (is_error, body) = call(
        &client,
        "propose_message",
        json!({"conversation_id": ALLOWED_CHAT, "text": "Sounds good", "agent": "test"}),
    )
    .await;
    assert!(!is_error, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["state"], "pending");
    let proposal_id = body["proposal_id"].as_str().unwrap();
    assert!(harness.hub.resolve(proposal_id, Outcome::Sent));

    let (_, events) = call(
        &client,
        "wait_for_event",
        json!({"timeout_seconds": 5, "since": cursor}),
    )
    .await;
    let events: Value = serde_json::from_str(&events).unwrap();
    assert_eq!(events["events"][0]["type"], "proposal_resolved");
    assert_eq!(events["events"][0]["outcome"], "sent");
    assert_eq!(events["events"][0]["proposal_id"], proposal_id);
}

#[tokio::test]
async fn propose_is_refused_when_turned_off_or_out_of_scope() {
    let harness = Harness::start(false).await;
    let client = harness.client().await;
    let (is_error, text) = call(
        &client,
        "propose_message",
        json!({"conversation_id": ALLOWED_CHAT, "text": "hi"}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("turned off"), "{text}");

    let (is_error, text) = call(
        &client,
        "propose_message",
        json!({"conversation_id": HIDDEN_CHAT, "text": "hi"}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("not found or not allowed"), "{text}");
    assert!(harness.hub.pending_proposal(ALLOWED_CHAT).is_none());
}

#[tokio::test]
async fn a_second_server_on_the_same_port_reports_it_clearly() {
    let harness = Harness::start(true).await;
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (ui, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let hub = AgentHub::new(store.clone(), AgentPolicy::default(), ui);
    let result = mcp::start(
        hub,
        Arc::new(FakeSource { store }),
        harness.server.addr.port(),
        TOKEN.to_owned(),
    )
    .await;
    let error = result.err().expect("port is taken");
    assert!(error.to_string().contains("already in use"), "{error}");
}

#[tokio::test]
async fn meetings_and_transcripts_need_their_switches() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (is_error, text) = call(&client, "list_meetings", json!({})).await;
    assert!(is_error);
    assert!(text.contains("calendar access is turned off"), "{text}");
    let (is_error, text) = call(
        &client,
        "get_transcript",
        json!({"meeting_chat_id": MEETING_CHAT}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("transcripts are turned off"), "{text}");
}

#[tokio::test]
async fn list_meetings_returns_the_window() {
    let harness = Harness::start_with(true, true, false).await;
    let client = harness.client().await;
    let (is_error, body) = call(
        &client,
        "list_meetings",
        json!({"start": "2026-10-07T00:00:00Z", "end": "2026-10-08T00:00:00Z"}),
    )
    .await;
    assert!(!is_error, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["meetings"][0]["title"], "Sprint review");
    assert_eq!(body["meetings"][0]["meeting_chat_id"], MEETING_CHAT);

    let (_, empty) = call(
        &client,
        "list_meetings",
        json!({"start": "2026-10-09T00:00:00Z", "end": "2026-10-10T00:00:00Z"}),
    )
    .await;
    let empty: Value = serde_json::from_str(&empty).unwrap();
    assert!(empty["meetings"].as_array().unwrap().is_empty());

    let (is_error, text) = call(
        &client,
        "list_meetings",
        json!({"start": "2026-10-09T00:00:00Z", "end": "2026-10-08T00:00:00Z"}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("end must be after start"), "{text}");
}

#[tokio::test]
async fn get_transcript_returns_turns_and_live_captions() {
    let harness = Harness::start_with(true, false, true).await;
    let client = harness.client().await;
    let (is_error, body) = call(
        &client,
        "get_transcript",
        json!({"meeting_chat_id": MEETING_CHAT}),
    )
    .await;
    assert!(!is_error, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["turns"][0]["text"], "Welcome everyone");
    assert!(body["live_captions"].as_array().unwrap().is_empty());

    let (is_error, text) = call(
        &client,
        "get_transcript",
        json!({"meeting_chat_id": HIDDEN_MEETING_CHAT}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("not found or not allowed"), "{text}");

    let live_chat = LIVE_MEETING_CHAT;
    let (is_error, text) = call(
        &client,
        "get_transcript",
        json!({"meeting_chat_id": live_chat}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("no finished recording"), "{text}");

    harness.hub.push_caption(live_chat, "1", "Bo", "Hi all");
    let (is_error, body) = call(
        &client,
        "get_transcript",
        json!({"meeting_chat_id": live_chat}),
    )
    .await;
    assert!(!is_error, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    assert!(body["turns"].as_array().unwrap().is_empty());
    assert_eq!(body["live_captions"][0]["speaker"], "Bo");
    assert_eq!(body["live_captions"][0]["text"], "Hi all");
}

#[tokio::test]
async fn propose_rejects_a_reply_target_from_another_conversation() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (is_error, text) = call(
        &client,
        "propose_message",
        json!({"conversation_id": ALLOWED_CHAT, "text": "ok", "reply_to_message_id": "m3"}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("is not a message of this conversation"), "{text}");
    let (is_error, body) = call(
        &client,
        "propose_message",
        json!({"conversation_id": ALLOWED_CHAT, "text": "ok", "reply_to_message_id": "m1"}),
    )
    .await;
    assert!(!is_error, "{body}");
}
