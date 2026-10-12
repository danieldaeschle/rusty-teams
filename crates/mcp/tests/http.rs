use std::collections::BTreeSet;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeZone, Utc};
use mcp::{
    Access, AgentHub, AgentPolicy, EncodedImage, FileInfo, ImageInfo, ImageKind, LinkInfo, Meeting,
    MentionInfo, MessageExtras, Outcome, QuoteInfo, ReactionInfo, ServerHandle, SourceError,
    SourceFuture, TeamsSource, Transcript, TranscriptTurn,
};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
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
const RICH_MESSAGE: &str = "m5";
const RICH_IMAGE: &str = "0-1a2b3c4d";
const PIXEL_PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

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

    fn message_extras(&self, message: &MessageRecord, _my_user_id: Option<&str>) -> MessageExtras {
        let text = store::plain_text(&message.body_html);
        if message.message_id != RICH_MESSAGE {
            return MessageExtras {
                text,
                ..MessageExtras::default()
            };
        }
        MessageExtras {
            text,
            images: vec![ImageInfo {
                image_id: RICH_IMAGE.to_owned(),
                kind: ImageKind::Gif,
                alt: Some("dancing cat".to_owned()),
            }],
            reactions: vec![ReactionInfo {
                emoji: "\u{1F44D}".to_owned(),
                count: 2,
                mine: true,
            }],
            mentions: vec![MentionInfo {
                name: "Bob Sample".to_owned(),
                user_id: Some("user-bob".to_owned()),
            }],
            files: vec![FileInfo {
                name: "Plan.docx".to_owned(),
                size: None,
                file_type: Some("word".to_owned()),
                link: "https://example.sharepoint.com/Plan.docx".to_owned(),
            }],
            quote: Some(QuoteInfo {
                message_id: "m1".to_owned(),
                sender: Some("Ada Example".to_owned()),
                text: "budget plan".to_owned(),
            }),
            cards: vec!["Build passed".to_owned()],
            links: vec![LinkInfo {
                url: "https://news.example.com/story".to_owned(),
                title: Some("A story".to_owned()),
            }],
        }
    }

    fn image<'a>(
        &'a self,
        _message: &'a MessageRecord,
        _image_id: &'a str,
        _max_edge: u32,
    ) -> SourceFuture<'a, EncodedImage> {
        Box::pin(async {
            Ok(EncodedImage {
                bytes: PIXEL_PNG.to_vec(),
                mime_type: "image/png".to_owned(),
                kind: ImageKind::Gif,
                alt: Some("dancing cat".to_owned()),
                width: 1,
                height: 1,
            })
        })
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
    store: Arc<Store>,
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
            Arc::new(FakeSource {
                store: store.clone(),
            }),
            0,
            TOKEN.to_owned(),
        )
        .await
        .unwrap();
        Harness { store, hub, server }
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
            message(ALLOWED_CHAT, RICH_MESSAGE, 7, "<p>see the cat</p>"),
            message(CHANNEL, "t1", 8, "<p>thread root</p>"),
            MessageRecord {
                reply_to_id: Some("t1".to_owned()),
                ..message(CHANNEL, "t2", 9, "<p>thread reply</p>")
            },
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

async fn call_raw(
    client: &RunningService<rmcp::RoleClient, ClientConfig>,
    tool: &'static str,
    arguments: Value,
) -> CallToolResult {
    let params = CallToolRequestParams::new(tool)
        .with_arguments(arguments.as_object().cloned().unwrap_or_default());
    client.call_tool(params).await.unwrap()
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn message_ids(body: &Value) -> Vec<&str> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["message_id"].as_str().unwrap())
        .collect()
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
async fn lists_all_ten_tools_and_marks_reads_read_only() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let tools = client.list_all_tools().await.unwrap();
    let mut names: Vec<String> = tools.iter().map(|tool| tool.name.to_string()).collect();
    names.sort();
    for tool in &tools {
        let read_only = tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint);
        assert_eq!(
            read_only == Some(true),
            tool.name != "propose_message",
            "{}",
            tool.name
        );
    }
    assert_eq!(
        names,
        [
            "get_image",
            "get_transcript",
            "list_channels",
            "list_chats",
            "list_meetings",
            "propose_message",
            "read_messages",
            "recent_messages",
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
    assert!(
        text.contains("is not a message of this conversation"),
        "{text}"
    );
    let (is_error, body) = call(
        &client,
        "propose_message",
        json!({"conversation_id": ALLOWED_CHAT, "text": "ok", "reply_to_message_id": "m1"}),
    )
    .await;
    assert!(!is_error, "{body}");
}

#[tokio::test]
async fn read_messages_adds_rich_fields_only_where_present() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (_, body) = call(
        &client,
        "read_messages",
        json!({"conversation_id": ALLOWED_CHAT}),
    )
    .await;
    let body = json_of(&body);
    let messages = body["messages"].as_array().unwrap();
    let plain = &messages[0];
    for field in [
        "images",
        "reactions",
        "mentions",
        "files",
        "quote",
        "cards",
        "links",
        "thread_root_id",
    ] {
        assert!(plain.get(field).is_none(), "{field}");
    }
    let rich = messages
        .iter()
        .find(|message| message["message_id"] == RICH_MESSAGE)
        .unwrap();
    assert_eq!(rich["text"], "see the cat");
    assert_eq!(
        rich["images"],
        json!([{"image_id": RICH_IMAGE, "kind": "gif", "alt": "dancing cat"}])
    );
    assert_eq!(
        rich["reactions"],
        json!([{"emoji": "\u{1F44D}", "count": 2, "mine": true}])
    );
    assert_eq!(
        rich["mentions"],
        json!([{"name": "Bob Sample", "user_id": "user-bob"}])
    );
    assert_eq!(rich["files"][0]["name"], "Plan.docx");
    assert_eq!(rich["files"][0]["type"], "word");
    assert_eq!(rich["quote"]["message_id"], "m1");
    assert_eq!(rich["cards"], json!(["Build passed"]));
    assert_eq!(rich["links"][0]["title"], "A story");
}

#[tokio::test]
async fn read_messages_filters_one_channel_thread() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (_, body) = call(
        &client,
        "read_messages",
        json!({"conversation_id": CHANNEL, "thread_root_id": "t1"}),
    )
    .await;
    let body = json_of(&body);
    assert_eq!(message_ids(&body), ["t1", "t2"]);
    assert_eq!(body["messages"][1]["thread_root_id"], "t1");
    assert_eq!(body["messages"][0]["thread_root_id"], "t1");

    let (_, body) = call(
        &client,
        "read_messages",
        json!({"conversation_id": CHANNEL, "thread_root_id": "t1", "limit": 1}),
    )
    .await;
    assert_eq!(message_ids(&json_of(&body)), ["t2"]);

    let (is_error, text) = call(
        &client,
        "read_messages",
        json!({"conversation_id": CHANNEL, "thread_root_id": "t2"}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("is a reply"), "{text}");

    let (is_error, text) = call(
        &client,
        "read_messages",
        json!({"conversation_id": ALLOWED_CHAT, "thread_root_id": "m1"}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("only works in channels"), "{text}");

    let (is_error, text) = call(
        &client,
        "read_messages",
        json!({"conversation_id": CHANNEL, "thread_root_id": "nope"}),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("unknown thread_root_id"), "{text}");
}

#[tokio::test]
async fn get_image_returns_an_image_block_and_a_description() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let result = call_raw(
        &client,
        "get_image",
        json!({"conversation_id": ALLOWED_CHAT, "message_id": RICH_MESSAGE, "image_id": RICH_IMAGE}),
    )
    .await;
    assert_ne!(result.is_error, Some(true));
    let image = result.content[0].as_image().unwrap();
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(STANDARD.decode(&image.data).unwrap(), PIXEL_PNG);
    let description = json_of(&result.content[1].as_text().unwrap().text);
    assert_eq!(description["kind"], "gif");
    assert_eq!(description["alt"], "dancing cat");
    assert_eq!(description["width"], 1);
}

#[tokio::test]
async fn get_image_refuses_foreign_hidden_and_unknown_targets() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let refused = |conversation_id: &str, message_id: &str, image_id: &str| json!({"conversation_id": conversation_id, "message_id": message_id, "image_id": image_id});
    let (is_error, text) = call(&client, "get_image", refused(HIDDEN_CHAT, "m3", RICH_IMAGE)).await;
    assert!(is_error);
    assert!(text.contains("not allowed"), "{text}");

    let (is_error, text) = call(
        &client,
        "get_image",
        refused(ALLOWED_CHAT, "m3", RICH_IMAGE),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("unknown message_id"), "{text}");

    let (is_error, text) = call(
        &client,
        "get_image",
        refused(ALLOWED_CHAT, RICH_MESSAGE, "9-deadbeef"),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("unknown image_id"), "{text}");

    let (is_error, text) = call(
        &client,
        "get_image",
        refused(ALLOWED_CHAT, "m1", RICH_IMAGE),
    )
    .await;
    assert!(is_error);
    assert!(text.contains("unknown image_id"), "{text}");
}

fn since_start_of_seed() -> Value {
    json!("2026-10-06T00:00:00Z")
}

#[tokio::test]
async fn recent_messages_run_newest_first_inside_the_allowed_scope() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let (is_error, body) = call(
        &client,
        "recent_messages",
        json!({"since": since_start_of_seed()}),
    )
    .await;
    assert!(!is_error, "{body}");
    let body = json_of(&body);
    assert_eq!(
        message_ids(&body),
        ["t2", "t1", RICH_MESSAGE, "m4", "m2", "m1"]
    );
    assert_eq!(body["messages"][0]["conversation"], "Platform / General");
    assert_eq!(body["messages"][0]["conversation_id"], CHANNEL);
    assert_eq!(body["messages"][0]["thread_root_id"], "t1");
    assert_eq!(body["messages"][2]["conversation"], "Planning");
    assert_eq!(body["messages"][2]["images"][0]["image_id"], RICH_IMAGE);
    assert!(body["next_cursor"].is_null());
    assert!(!body.to_string().contains("m3"));

    let (_, body) = call(
        &client,
        "recent_messages",
        json!({"since": "2026-10-06T09:05:00Z"}),
    )
    .await;
    assert_eq!(message_ids(&json_of(&body)), ["t2", "t1", RICH_MESSAGE]);

    let (_, body) = call(
        &client,
        "recent_messages",
        json!({"since": since_start_of_seed(), "kinds": ["channel"]}),
    )
    .await;
    assert_eq!(message_ids(&json_of(&body)), ["t2", "t1", "m4"]);

    let (_, body) = call(
        &client,
        "recent_messages",
        json!({"since": since_start_of_seed(), "kinds": ["chat", "meeting"]}),
    )
    .await;
    assert_eq!(message_ids(&json_of(&body)), [RICH_MESSAGE, "m2", "m1"]);
}

#[tokio::test]
async fn recent_messages_page_with_a_cursor() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let mut collected: Vec<String> = Vec::new();
    let mut cursor = Value::Null;
    for _ in 0..4 {
        let mut arguments = json!({"since": since_start_of_seed(), "limit": 4});
        if !cursor.is_null() {
            arguments["cursor"] = cursor.clone();
        }
        let (is_error, body) = call(&client, "recent_messages", arguments).await;
        assert!(!is_error, "{body}");
        let body = json_of(&body);
        collected.extend(message_ids(&body).into_iter().map(str::to_owned));
        cursor = body["next_cursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    assert_eq!(collected, ["t2", "t1", RICH_MESSAGE, "m4", "m2", "m1"]);

    let (_, first_page) = call(
        &client,
        "recent_messages",
        json!({"since": since_start_of_seed(), "limit": 2}),
    )
    .await;
    let cursor = json_of(&first_page)["next_cursor"].clone();
    let (_, later_page) = call(
        &client,
        "recent_messages",
        json!({"since": "2030-01-01T00:00:00Z", "cursor": cursor}),
    )
    .await;
    assert_eq!(
        message_ids(&json_of(&later_page)),
        [RICH_MESSAGE, "m4", "m2", "m1"]
    );

    let (is_error, text) = call(&client, "recent_messages", json!({"cursor": "garbage"})).await;
    assert!(is_error);
    assert!(text.contains("invalid cursor"), "{text}");
}

#[tokio::test]
async fn recent_messages_unread_only_covers_chats_and_channels() {
    let harness = Harness::start(true).await;
    let client = harness.client().await;
    let arguments = || json!({"since": since_start_of_seed(), "unread_only": true});
    let (_, body) = call(&client, "recent_messages", arguments()).await;
    assert!(message_ids(&json_of(&body)).is_empty());

    let read_until = Utc.with_ymd_and_hms(2026, 10, 6, 9, 2, 0).unwrap();
    harness
        .store
        .mark_chat_unread(ALLOWED_CHAT, read_until)
        .unwrap();
    let (_, body) = call(&client, "recent_messages", arguments()).await;
    assert_eq!(message_ids(&json_of(&body)), [RICH_MESSAGE]);

    harness
        .store
        .upsert_channels(&[ChannelRecord {
            id: CHANNEL.to_owned(),
            team_id: "team-1".to_owned(),
            name: "General".to_owned(),
            membership_type: None,
            last_message_at: Some(Utc.with_ymd_and_hms(2026, 10, 6, 9, 6, 0).unwrap()),
            unread: true,
        }])
        .unwrap();
    let (_, body) = call(&client, "recent_messages", arguments()).await;
    assert_eq!(
        message_ids(&json_of(&body)),
        ["t2", "t1", RICH_MESSAGE, "m4"]
    );
}
