use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ProgressNotificationParam, RequestMetaObject};
use rmcp::service::Peer;
use rmcp::{RoleServer, ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use store::{HIGHLIGHT_END, HIGHLIGHT_START, MessageRecord, Store, plain_text};

use crate::hub::AgentHub;
use crate::policy::{AgentPolicy, ConversationKind};
use crate::source::TeamsSource;

const NOT_ALLOWED: &str =
    "conversation not found or not allowed (allow it in rusty-teams Settings > MCP)";
const PROPOSE_DISABLED: &str =
    "proposing messages is turned off (enable it in rusty-teams Settings > MCP)";
const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 200;
const DEFAULT_READ_LIMIT: usize = 30;
const MAX_READ_LIMIT: usize = 200;
const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 100;
const DEFAULT_WAIT_SECONDS: u64 = 25;
const MAX_WAIT_SECONDS: u64 = 50;
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);
const MAX_PROPOSAL_CHARACTERS: usize = 20_000;
const DEFAULT_MEETING_DAYS: i64 = 7;
const MAX_MEETING_DAYS: i64 = 62;
const CALENDAR_DISABLED: &str =
    "calendar access is turned off (enable it in rusty-teams Settings > MCP)";
const TRANSCRIPTS_DISABLED: &str =
    "meeting transcripts are turned off (enable them in rusty-teams Settings > MCP)";

#[derive(Clone)]
pub struct TeamsTools {
    hub: Arc<AgentHub>,
    source: Arc<dyn TeamsSource>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListChatsParams {
    /// Maximum number of chats to return, newest first. Default 50, max 200.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadMessagesParams {
    /// Raw conversation id as returned by list_chats or list_channels, for example "19:...@thread.v2".
    pub conversation_id: String,
    /// Number of messages to return. Default 30, max 200.
    pub limit: Option<usize>,
    /// Return only messages older than this message id, to page backwards.
    pub before_message_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchMessagesParams {
    /// Words to search for. Every word is a prefix match. Use "from:name" to filter by sender.
    pub query: String,
    /// Maximum number of hits, newest first. Default 20, max 100.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProposeMessageParams {
    /// Raw conversation id as returned by list_chats or list_channels.
    pub conversation_id: String,
    /// Message text. Markdown is allowed.
    pub text: String,
    /// Id of the message this one answers (threads in channels).
    pub reply_to_message_id: Option<String>,
    /// Your session id, so the user can tell proposals of different sessions apart.
    pub session_id: Option<String>,
    /// Your name, shown to the user next to the proposal.
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WaitForEventParams {
    /// Seconds to wait for an event. Default 25, max 50.
    pub timeout_seconds: Option<u64>,
    /// Cursor from the previous call's "next". Omit it to only see events from now on.
    pub since: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListMeetingsParams {
    /// Start of the window, ISO 8601 with offset. Default: the start of today (UTC).
    pub start: Option<DateTime<Utc>>,
    /// End of the window, ISO 8601 with offset. Default: 7 days after start, max 62 days.
    pub end: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTranscriptParams {
    /// Meeting chat id as returned by list_meetings (meeting_chat_id).
    pub meeting_chat_id: String,
}

#[derive(Serialize)]
struct MessageView {
    message_id: String,
    sender: Option<String>,
    sender_id: Option<String>,
    time: String,
    edited_at: Option<String>,
    text: String,
    subject: Option<String>,
    reply_to: Option<String>,
}

fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339()
}

fn message_view(message: MessageRecord) -> MessageView {
    MessageView {
        text: plain_text(&message.body_html),
        message_id: message.message_id,
        sender: message.sender_name,
        sender_id: message.sender_id,
        time: timestamp(message.created_at),
        edited_at: message.edited_at.map(timestamp),
        subject: message.subject,
        reply_to: message.reply_to_id,
    }
}

fn render(value: serde_json::Value) -> String {
    value.to_string()
}

fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tool_router]
impl TeamsTools {
    pub fn new(hub: Arc<AgentHub>, source: Arc<dyn TeamsSource>) -> Self {
        TeamsTools {
            hub,
            source,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List the Teams chats (1:1, group and meeting chats) the user allowed for agents, newest activity first. Returns id, title, kind, last_message_at, unread. Use the id with read_messages and propose_message."
    )]
    async fn list_chats(
        &self,
        Parameters(params): Parameters<ListChatsParams>,
    ) -> Result<String, String> {
        let policy = self.hub.policy();
        let store = self.store()?;
        let limit = params
            .limit
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .clamp(1, MAX_LIST_LIMIT);
        let chats: Vec<_> = allowed_chats(&store, &policy)?
            .into_iter()
            .take(limit)
            .map(|chat| {
                json!({
                    "id": chat.id,
                    "title": chat.display_title(),
                    "kind": chat.kind,
                    "last_message_at": chat.last_message_at.map(timestamp),
                    "unread": chat.unread,
                })
            })
            .collect();
        Ok(render(json!({ "chats": chats })))
    }

    #[tool(
        description = "List the Teams channels the user allowed for agents. Returns id, team, channel name, last_message_at, unread. Use the id with read_messages and propose_message."
    )]
    async fn list_channels(&self) -> Result<String, String> {
        let policy = self.hub.policy();
        let store = self.store()?;
        let mut channels = Vec::new();
        for team in store.sidebar().map_err(failure)?.teams {
            for channel in team.channels {
                if policy.can_read(ConversationKind::Channel, &channel.id) {
                    channels.push(json!({
                        "id": channel.id,
                        "team": team.team.name,
                        "channel": channel.name,
                        "last_message_at": channel.last_message_at.map(timestamp),
                        "unread": channel.unread,
                    }));
                }
            }
        }
        Ok(render(json!({ "channels": channels })))
    }

    #[tool(
        description = "Read messages of one allowed chat or channel, oldest first. Returns message_id, sender, sender_id, time (RFC 3339), text (plain text), subject, reply_to. Defaults to the 30 newest messages. Pass before_message_id to page backwards."
    )]
    async fn read_messages(
        &self,
        Parameters(params): Parameters<ReadMessagesParams>,
    ) -> Result<String, String> {
        let store = self.store()?;
        self.readable_kind(&store, &params.conversation_id)?;
        let limit = params
            .limit
            .unwrap_or(DEFAULT_READ_LIMIT)
            .clamp(1, MAX_READ_LIMIT);
        let before = match params.before_message_id {
            Some(message_id) => {
                let mut found = store
                    .messages_by_id(&params.conversation_id, std::slice::from_ref(&message_id))
                    .map_err(failure)?;
                let message = found
                    .remove(&message_id)
                    .ok_or_else(|| format!("unknown before_message_id {message_id}"))?;
                Some(message.created_at)
            }
            None => None,
        };
        let messages: Vec<_> = store
            .messages(&params.conversation_id, before, limit)
            .map_err(failure)?
            .into_iter()
            .filter(|message| !message.deleted)
            .map(message_view)
            .collect();
        Ok(render(json!({
            "conversation_id": params.conversation_id,
            "messages": messages,
        })))
    }

    #[tool(
        description = "Full-text search over the messages of all allowed chats and channels, newest first. Every word is a prefix match, \"from:name\" filters by sender. Returns conversation_id, conversation title, message_id, sender, time and a snippet."
    )]
    async fn search_messages(
        &self,
        Parameters(params): Parameters<SearchMessagesParams>,
    ) -> Result<String, String> {
        let policy = self.hub.policy();
        let store = self.store()?;
        let mut titles: HashMap<String, String> = allowed_chats(&store, &policy)?
            .into_iter()
            .map(|chat| {
                let title = chat.display_title();
                (chat.id, title)
            })
            .collect();
        for team in store.sidebar().map_err(failure)?.teams {
            for channel in team.channels {
                if policy.can_read(ConversationKind::Channel, &channel.id) {
                    titles.insert(channel.id, format!("{} / {}", team.team.name, channel.name));
                }
            }
        }
        let conversation_ids: Vec<String> = titles.keys().cloned().collect();
        let limit = params
            .limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .clamp(1, MAX_SEARCH_LIMIT);
        let hits: Vec<_> = store
            .search_messages_in(&params.query, &conversation_ids, limit)
            .map_err(failure)?
            .into_iter()
            .map(|hit| {
                json!({
                    "conversation_id": hit.conversation_id,
                    "conversation": titles.get(&hit.conversation_id),
                    "message_id": hit.message_id,
                    "sender": hit.sender_name,
                    "time": timestamp(hit.created_at),
                    "snippet": hit.snippet.replace([HIGHLIGHT_START, HIGHLIGHT_END], ""),
                })
            })
            .collect();
        Ok(render(json!({ "hits": hits })))
    }

    #[tool(
        description = "Propose a message for the user to review in rusty-teams. The message is NEVER sent automatically: the user sees it as a draft, can edit it, and sends or discards it. Only one proposal per conversation is pending, a new one replaces the old one. Returns proposal_id and state \"pending\". Use wait_for_event to learn what the user did with it."
    )]
    async fn propose_message(
        &self,
        Parameters(params): Parameters<ProposeMessageParams>,
    ) -> Result<String, String> {
        let store = self.store()?;
        self.readable_kind(&store, &params.conversation_id)?;
        if !self.hub.policy().propose {
            return Err(PROPOSE_DISABLED.to_owned());
        }
        if let Some(message_id) = &params.reply_to_message_id {
            let found = store
                .messages_by_id(&params.conversation_id, std::slice::from_ref(message_id))
                .map_err(failure)?;
            if !found.contains_key(message_id) {
                return Err(format!(
                    "reply_to_message_id {message_id} is not a message of this conversation"
                ));
            }
        }
        let text = params.text.trim();
        if text.is_empty() || text.chars().count() > MAX_PROPOSAL_CHARACTERS {
            return Err(format!(
                "text must be 1 to {MAX_PROPOSAL_CHARACTERS} characters long"
            ));
        }
        let proposal = self
            .hub
            .add_proposal(
                &params.conversation_id,
                text,
                params.reply_to_message_id,
                params.agent,
                params.session_id,
            )
            .map_err(failure)?;
        Ok(render(
            json!({ "proposal_id": proposal.id, "state": "pending" }),
        ))
    }

    #[tool(
        description = "Wait up to timeout_seconds (default 25, max 50) for events in allowed conversations: new_message, user_typing, user_sent, proposal_resolved (outcome sent = the user sent it unchanged and it is queued for delivery, edited, discarded, stale or superseded). Returns {events, next}. Call again with since set to next to continue without missing events. Without since, only events from now on are returned."
    )]
    async fn wait_for_event(
        &self,
        Parameters(params): Parameters<WaitForEventParams>,
        meta: RequestMetaObject,
        client: Peer<RoleServer>,
    ) -> Result<String, String> {
        let timeout = Duration::from_secs(
            params
                .timeout_seconds
                .unwrap_or(DEFAULT_WAIT_SECONDS)
                .clamp(1, MAX_WAIT_SECONDS),
        );
        let progress_token = meta.get_progress_token();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut cursor = params.since;
        let mut elapsed_intervals = 0.0;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let (events, next) = self
                .hub
                .wait_events(cursor, remaining.min(PROGRESS_INTERVAL))
                .await;
            if !events.is_empty() || remaining <= PROGRESS_INTERVAL {
                return Ok(render(json!({ "events": events, "next": next })));
            }
            cursor = Some(next);
            elapsed_intervals += 1.0;
            if let Some(token) = progress_token.clone() {
                let _ = client
                    .notify_progress(
                        ProgressNotificationParam::new(token, elapsed_intervals)
                            .with_message("waiting for events"),
                    )
                    .await;
            }
        }
    }

    #[tool(
        description = "List the user's calendar meetings in a time window, oldest first. Returns title, start, end (RFC 3339), all_day, organizer, join_url and meeting_chat_id (use it with get_transcript). Defaults to today and the next 7 days."
    )]
    async fn list_meetings(
        &self,
        Parameters(params): Parameters<ListMeetingsParams>,
    ) -> Result<String, String> {
        let policy = self.hub.policy();
        if !policy.enabled || !policy.calendar {
            return Err(CALENDAR_DISABLED.to_owned());
        }
        let start = params.start.unwrap_or_else(start_of_today);
        let end = params
            .end
            .unwrap_or(start + chrono::Duration::days(DEFAULT_MEETING_DAYS))
            .min(start + chrono::Duration::days(MAX_MEETING_DAYS));
        if end <= start {
            return Err("end must be after start".to_owned());
        }
        let meetings = self.source.calendar(start, end).await.map_err(failure)?;
        Ok(render(json!({ "meetings": meetings })))
    }

    #[tool(
        description = "Get the spoken text of a meeting by its meeting_chat_id: the finished transcript turns (speaker, start_seconds, end_seconds, text) once the recording is processed, and live_captions while the user is in the meeting right now."
    )]
    async fn get_transcript(
        &self,
        Parameters(params): Parameters<GetTranscriptParams>,
    ) -> Result<String, String> {
        let policy = self.hub.policy();
        if !policy.enabled || !policy.transcripts {
            return Err(TRANSCRIPTS_DISABLED.to_owned());
        }
        let store = self.store()?;
        self.readable_kind(&store, &params.meeting_chat_id)?;
        let live_captions = self.hub.live_captions(&params.meeting_chat_id);
        match self.source.transcript(&params.meeting_chat_id).await {
            Ok(transcript) => Ok(render(json!({
                "meeting_chat_id": params.meeting_chat_id,
                "recording_name": transcript.recording_name,
                "created_at": transcript.created_at,
                "turns": transcript.turns,
                "live_captions": live_captions,
            }))),
            Err(_) if !live_captions.is_empty() => Ok(render(json!({
                "meeting_chat_id": params.meeting_chat_id,
                "turns": [],
                "live_captions": live_captions,
            }))),
            Err(error) => Err(failure(error)),
        }
    }

    fn store(&self) -> Result<Arc<Store>, String> {
        self.source.store().map_err(failure)
    }

    fn readable_kind(
        &self,
        store: &Store,
        conversation_id: &str,
    ) -> Result<ConversationKind, String> {
        let policy = self.hub.policy();
        let kind = conversation_kind(store, conversation_id)?;
        match kind {
            Some(kind) if policy.can_read(kind, conversation_id) => Ok(kind),
            _ => Err(NOT_ALLOWED.to_owned()),
        }
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "rusty-teams",
    instructions = "Read-only access to the user's Teams chats and channels from their local rusty-teams app. \
Only conversations the user allowed in rusty-teams Settings > MCP are visible; anything else looks like it does not exist. \
Start with list_chats or list_channels, then read_messages or search_messages. \
To write, call propose_message: the user reviews the draft in the app and sends it, nothing is sent automatically. \
list_meetings and get_transcript only work when the user turned on calendar or transcripts access. \
Use wait_for_event to follow new messages and to learn what happened to a proposal."
)]
impl ServerHandler for TeamsTools {}

fn start_of_today() -> DateTime<Utc> {
    Utc::now()
        .date_naive()
        .and_time(chrono::NaiveTime::MIN)
        .and_utc()
}

fn conversation_kind(
    store: &Store,
    conversation_id: &str,
) -> Result<Option<ConversationKind>, String> {
    if let Some(chat) = store.chat(conversation_id).map_err(failure)? {
        return Ok(Some(chat_kind(&chat)));
    }
    Ok(store
        .channel(conversation_id)
        .map_err(failure)?
        .map(|_| ConversationKind::Channel))
}

fn allowed_chats(store: &Store, policy: &AgentPolicy) -> Result<Vec<store::ChatRecord>, String> {
    Ok(store
        .recent_chats(usize::MAX >> 1)
        .map_err(failure)?
        .into_iter()
        .filter(|chat| policy.can_read(chat_kind(chat), &chat.id))
        .collect())
}

fn chat_kind(chat: &store::ChatRecord) -> ConversationKind {
    if chat.kind.eq_ignore_ascii_case("meeting") {
        ConversationKind::Meeting
    } else {
        ConversationKind::Chat
    }
}
