use std::sync::{Arc, OnceLock};

use chatsvc::{Calendar, Transcripts};
use chrono::{DateTime, Utc};
use mcp::{Meeting, SourceError, SourceFuture, TeamsSource, Transcript, TranscriptTurn};
use store::Store;

use crate::backend::Engine;

pub struct AppSource {
    store: Arc<Store>,
    engine: OnceLock<Arc<Engine>>,
}

impl AppSource {
    pub fn new(store: Arc<Store>) -> Self {
        AppSource {
            store,
            engine: OnceLock::new(),
        }
    }

    pub fn set_engine(&self, engine: Arc<Engine>) -> bool {
        self.engine.set(engine).is_ok()
    }

    fn engine(&self) -> Result<&Arc<Engine>, SourceError> {
        self.engine.get().ok_or(SourceError::NotConnected)
    }
}

impl TeamsSource for AppSource {
    fn store(&self) -> Result<Arc<Store>, SourceError> {
        Ok(self.store.clone())
    }

    fn me_id(&self) -> Option<String> {
        crate::data::me(&self.store).map(|person| person.user_id)
    }

    fn calendar(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> SourceFuture<'_, Vec<Meeting>> {
        Box::pin(async move {
            let engine = self.engine()?;
            let events = Calendar::new(engine.remote().session())
                .calendar_view(start, end)
                .await
                .map_err(|error| SourceError::Failed(error.to_string()))?;
            Ok(events
                .into_iter()
                .filter(|event| !event.cancelled)
                .map(|event| Meeting {
                    title: event.subject,
                    start: event.start,
                    end: event.end,
                    all_day: event.all_day,
                    organizer: event.organizer_name,
                    join_url: event.join_url,
                    meeting_chat_id: event.thread_id,
                })
                .collect())
        })
    }

    fn transcript<'a>(&'a self, meeting_chat_id: &'a str) -> SourceFuture<'a, Transcript> {
        Box::pin(async move {
            let engine = self.engine()?;
            let transcript = Transcripts::new(engine.remote().session())
                .for_meeting_chat(meeting_chat_id)
                .await
                .map_err(|error| SourceError::Failed(error.to_string()))?;
            Ok(Transcript {
                recording_name: transcript.recording_name,
                created_at: transcript.created_at,
                turns: transcript
                    .turns
                    .into_iter()
                    .map(|turn| TranscriptTurn {
                        speaker: turn.speaker,
                        start_seconds: turn.start_seconds,
                        end_seconds: turn.end_seconds,
                        text: turn.text,
                    })
                    .collect(),
            })
        })
    }
}
