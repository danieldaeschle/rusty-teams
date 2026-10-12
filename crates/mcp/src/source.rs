use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use store::Store;

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

pub trait TeamsSource: Send + Sync + 'static {
    fn store(&self) -> Result<Arc<Store>, SourceError>;

    fn me_id(&self) -> Option<String>;

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
