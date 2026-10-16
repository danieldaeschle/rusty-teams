use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use store::{MessageRecord, Store};

pub type SourceFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SourceError>> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("rusty-teams is not connected to Teams")]
    NotConnected,
    #[error("{0} is not supported by this build")]
    Unsupported(&'static str),
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Meeting {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub organizer: Option<String>,
    pub join_url: Option<String>,
    pub meeting_chat_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptTurn {
    pub speaker: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transcript {
    pub recording_name: Option<String>,
    pub created_at: Option<String>,
    pub turns: Vec<TranscriptTurn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    Inline,
    Gif,
    Sticker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageInfo {
    pub image_id: String,
    pub kind: ImageKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReactionInfo {
    pub emoji: String,
    pub count: usize,
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MentionInfo {
    pub name: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub size: Option<u64>,
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuoteInfo {
    pub message_id: String,
    pub sender: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForwardedInfo {
    pub message_id: String,
    pub conversation_id: Option<String>,
    pub sender: Option<String>,
    pub sender_id: Option<String>,
    pub time: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkInfo {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageExtras {
    pub text: String,
    pub images: Vec<ImageInfo>,
    pub reactions: Vec<ReactionInfo>,
    pub mentions: Vec<MentionInfo>,
    pub files: Vec<FileInfo>,
    pub quote: Option<QuoteInfo>,
    pub forwarded: Vec<ForwardedInfo>,
    pub cards: Vec<String>,
    pub links: Vec<LinkInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub kind: ImageKind,
    pub alt: Option<String>,
    pub width: u32,
    pub height: u32,
}

pub trait TeamsSource: Send + Sync + 'static {
    fn store(&self) -> Result<Arc<Store>, SourceError>;

    fn me_id(&self) -> Option<String>;

    fn message_extras(&self, message: &MessageRecord, my_user_id: Option<&str>) -> MessageExtras;

    fn image<'a>(
        &'a self,
        message: &'a MessageRecord,
        image_id: &'a str,
        max_edge: u32,
    ) -> SourceFuture<'a, EncodedImage>;

    fn reaction_supported(&self, emoji: &str) -> bool;

    fn image_dimensions(&self, bytes: &[u8]) -> Option<(u32, u32)>;

    fn rendered_text(&self, markdown: &str) -> String;

    /// Per name: the `@name` spans (in rendered text offsets) where a mention may sit.
    fn plain_mention_spans(
        &self,
        markdown: &str,
        names: &[String],
        ignore_case: bool,
    ) -> Vec<Vec<std::ops::Range<usize>>>;

    fn refresh_recent(&self, conversation_ids: Vec<String>) -> SourceFuture<'_, ()>;

    fn calendar(
        &self,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> SourceFuture<'_, Vec<Meeting>> {
        Box::pin(async { Err(SourceError::Unsupported("calendar")) })
    }

    fn transcript<'a>(&'a self, _meeting_chat_id: &'a str) -> SourceFuture<'a, Transcript> {
        Box::pin(async { Err(SourceError::Unsupported("transcript")) })
    }
}
