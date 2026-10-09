use std::collections::HashMap;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MemberRecord {
    pub user_id: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatRecord {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub member_summary: String,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub unread: bool,
    pub muted: bool,
    pub members: Vec<MemberRecord>,
    pub last_message_preview: Option<String>,
    pub last_message_sender_id: Option<String>,
    pub last_message_sender_name: Option<String>,
    pub last_message_deleted: bool,
    pub last_event_system: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AvatarRecord {
    pub user_id: String,
    pub bytes: Option<Vec<u8>>,
    pub content_type: String,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub expanded: bool,
    pub conversation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamRecord {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelRecord {
    pub id: String,
    pub team_id: String,
    pub name: String,
    pub membership_type: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub unread: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MessageRecord {
    pub conversation_id: String,
    pub message_id: String,
    pub reply_to_id: Option<String>,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
    pub deleted: bool,
    pub body_html: String,
    pub attachments_json: String,
    pub reactions_json: String,
    pub mentions_json: String,
    pub sender_application_id: Option<String>,
    pub links_json: String,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncState {
    pub newest_seen: Option<DateTime<Utc>>,
    pub oldest_loaded: Option<DateTime<Utc>>,
    pub has_more: bool,
    pub older_cursor: Option<String>,
    pub delta_link: Option<String>,
}

impl Default for SyncState {
    fn default() -> Self {
        SyncState {
            newest_seen: None,
            oldest_loaded: None,
            has_more: true,
            older_cursor: None,
            delta_link: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarTeam {
    pub team: TeamRecord,
    pub channels: Vec<ChannelRecord>,
    pub hidden: bool,
    pub hidden_channel_ids: Vec<String>,
    pub notifications: HashMap<String, ChannelNotifications>,
}

impl SidebarTeam {
    pub fn channel_notifications(&self, channel_id: &str) -> ChannelNotifications {
        self.notifications
            .get(channel_id)
            .copied()
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelNotificationLevel {
    BannerAndFeed,
    #[default]
    Feed,
    Off,
}

impl ChannelNotificationLevel {
    pub fn key(self) -> &'static str {
        match self {
            ChannelNotificationLevel::BannerAndFeed => "banner",
            ChannelNotificationLevel::Feed => "feed",
            ChannelNotificationLevel::Off => "off",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "banner" => ChannelNotificationLevel::BannerAndFeed,
            "off" => ChannelNotificationLevel::Off,
            _ => ChannelNotificationLevel::Feed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelNotifications {
    pub level: ChannelNotificationLevel,
    pub include_replies: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelTabRecord {
    pub tab_id: String,
    pub name: String,
    pub definition_id: String,
    pub open_url: Option<String>,
}

const WEBSITE_DEFINITION_ID: &str = "com.microsoft.teamspace.tab.web";

impl ChannelTabRecord {
    pub fn is_website(&self) -> bool {
        self.definition_id == WEBSITE_DEFINITION_ID && self.open_url.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLayoutRecord {
    pub channel_id: String,
    pub general: bool,
    pub hidden: bool,
    pub tabs: Vec<ChannelTabRecord>,
    pub notifications: ChannelNotifications,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamLayoutRecord {
    pub team_id: String,
    pub hidden: bool,
    pub channels: Vec<ChannelLayoutRecord>,
}

/// Everything the sidebar renders: chats by last message (newest first), teams in the user's Teams order (alphabetical without one), General first, then channels by name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sidebar {
    pub chats: Vec<ChatRecord>,
    pub teams: Vec<SidebarTeam>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChatPreview<'a> {
    pub text: Option<&'a str>,
    pub sender_id: Option<&'a str>,
    pub sender_name: Option<&'a str>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageRecord {
    pub key: String,
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub conversation_id: String,
    pub message_id: String,
    pub sender_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationHit {
    pub conversation_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityRecord {
    pub id: i64,
    pub conversation_id: String,
    pub kind: String,
    pub message_id: String,
    pub actors_json: String,
    pub preview: String,
    pub glyphs: String,
    pub count: u32,
    pub updated_at: DateTime<Utc>,
    pub read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentImage {
    pub name: String,
    pub format: String,
    pub bytes: Vec<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxTarget {
    Flat,
    Post,
    Thread,
}

impl OutboxTarget {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            OutboxTarget::Flat => "flat",
            OutboxTarget::Post => "post",
            OutboxTarget::Thread => "thread",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "flat" => Some(OutboxTarget::Flat),
            "post" => Some(OutboxTarget::Post),
            "thread" => Some(OutboxTarget::Thread),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxState {
    Sending,
    Failed,
}

impl OutboxState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            OutboxState::Sending => "sending",
            OutboxState::Failed => "failed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "sending" => Some(OutboxState::Sending),
            "failed" => Some(OutboxState::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRecord {
    pub id: String,
    pub conversation_id: String,
    pub target: OutboxTarget,
    pub thread_root_id: Option<String>,
    pub payload: String,
    pub images: Vec<AttachmentImage>,
    pub state: OutboxState,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRecord {
    pub conversation_id: String,
    pub payload: String,
    pub preview: String,
    pub images: Vec<AttachmentImage>,
    pub updated_at: DateTime<Utc>,
}
