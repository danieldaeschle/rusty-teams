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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLayoutRecord {
    pub channel_id: String,
    pub general: bool,
    pub hidden: bool,
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
