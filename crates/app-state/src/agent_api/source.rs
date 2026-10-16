use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chatsvc::{Calendar, Transcripts};
use chrono::{DateTime, Utc};
use mcp::{
    EncodedImage, Meeting, MessageExtras, SourceError, SourceFuture, TeamsSource, Transcript,
    TranscriptTurn,
};
use store::{MessageRecord, Store};
use teams_core::CoreEvent;

use super::extras::{image_kind, message_extras};
use super::image_scale::downscale;
use crate::backend::Engine;

const IMAGE_WAIT: Duration = Duration::from_secs(10);

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

    fn message_extras(&self, message: &MessageRecord, my_user_id: Option<&str>) -> MessageExtras {
        let mut extras = message_extras(message, my_user_id);
        let unnamed: Vec<String> = extras
            .forwarded
            .iter()
            .filter(|forward| forward.sender.is_none())
            .filter_map(|forward| forward.sender_id.clone())
            .collect();
        let names = crate::people::stored_names(&self.store, &unnamed);
        for forward in &mut extras.forwarded {
            if forward.sender.is_none() {
                forward.sender = forward
                    .sender_id
                    .as_ref()
                    .and_then(|sender_id| names.get(sender_id).cloned());
            }
        }
        extras
    }

    fn image_dimensions(&self, bytes: &[u8]) -> Option<(u32, u32)> {
        crate::inline_image::dimensions_of(bytes)
    }

    fn rendered_text(&self, markdown: &str) -> String {
        teams_core::Draft::from_markdown(markdown).text().to_owned()
    }

    fn plain_mention_spans(
        &self,
        markdown: &str,
        names: &[String],
        ignore_case: bool,
    ) -> Vec<Vec<std::ops::Range<usize>>> {
        let draft = teams_core::Draft::from_markdown(markdown);
        names
            .iter()
            .map(|name| teams_core::plain_mention_spans(&draft, name, ignore_case))
            .collect()
    }

    fn reaction_supported(&self, emoji: &str) -> bool {
        chatsvc::emotion_key(emoji).is_some()
    }

    fn image<'a>(
        &'a self,
        message: &'a MessageRecord,
        image_id: &'a str,
        max_edge: u32,
    ) -> SourceFuture<'a, EncodedImage> {
        Box::pin(async move {
            let engine = self.engine()?;
            let image = teams_core::images(message)
                .into_iter()
                .find(|image| image.id == image_id)
                .ok_or_else(|| SourceError::Failed(format!("unknown image_id {image_id}")))?;
            let mut events = engine.subscribe();
            if engine.image(image.key()).is_none() {
                engine.fetch_image(&image).await.map_err(|error| {
                    SourceError::Failed(format!("image could not be fetched: {error}"))
                })?;
            }
            let stored = match engine.image(image.key()) {
                Some(stored) => stored,
                None => {
                    wait_for_image(&mut events, image.key()).await;
                    if engine.image(image.key()).is_none() {
                        engine.fetch_image(&image).await.map_err(|error| {
                            SourceError::Failed(format!("image could not be fetched: {error}"))
                        })?;
                    }
                    engine.image(image.key()).ok_or_else(|| {
                        SourceError::Failed("the image is still loading, try again".to_owned())
                    })?
                }
            };
            let kind = image.kind();
            let scaled = tokio::task::spawn_blocking(move || downscale(&stored.bytes, max_edge))
                .await
                .map_err(|error| SourceError::Failed(error.to_string()))?
                .map_err(SourceError::Failed)?;
            Ok(EncodedImage {
                bytes: scaled.bytes,
                mime_type: scaled.mime_type.to_owned(),
                kind: image_kind(kind),
                alt: image.alt,
                width: scaled.width,
                height: scaled.height,
            })
        })
    }

    fn refresh_recent(&self, conversation_ids: Vec<String>) -> SourceFuture<'_, ()> {
        Box::pin(async move {
            let engine = self.engine()?;
            for conversation_id in conversation_ids {
                let _ = engine.fetch_newer(&conversation_id).await;
            }
            Ok(())
        })
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

async fn wait_for_image(events: &mut tokio::sync::broadcast::Receiver<CoreEvent>, key: &str) {
    let _ = tokio::time::timeout(IMAGE_WAIT, async {
        loop {
            match events.recv().await {
                Ok(CoreEvent::ImagesChanged { keys }) if keys.iter().any(|known| known == key) => {
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                _ => {}
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mcp::TeamsSource;
    use store::Store;

    use super::AppSource;

    fn source() -> AppSource {
        AppSource::new(Arc::new(Store::open_in_memory().unwrap()))
    }

    fn spans(markdown: &str, name: &str, ignore_case: bool) -> usize {
        source()
            .plain_mention_spans(markdown, &[name.to_owned()], ignore_case)
            .remove(0)
            .len()
    }

    #[test]
    fn mention_spans_follow_the_real_plain_text_rule() {
        assert_eq!(spans("**hi @Ada** and `@Bo`", "Ada", false), 1);
        assert_eq!(spans("**hi @Ada** and `@Bo`", "Bo", false), 0);
        assert_eq!(spans("@Ada hi", "Ada", false), 1);
        assert_eq!(spans("@ada hi", "Ada", true), 1);
        assert_eq!(spans("@ada hi", "Ada", false), 0);
        assert_eq!(spans("[x](https://e.com/@Ada) ops@Ada", "Ada", false), 0);
        let both = source().plain_mention_spans(
            "@Ada and @Bo",
            &["Ada".to_owned(), "Bo".to_owned()],
            false,
        );
        assert_eq!(both, [vec![0..4], vec![9..12]]);
    }

    #[test]
    fn the_other_trait_methods_do_real_work() {
        let source = source();
        assert_eq!(source.rendered_text("**bold** @Ada"), "bold @Ada");
        assert!(source.reaction_supported("\u{1F44D}"));
        assert!(!source.reaction_supported("not an emoji"));
        let mut png = Vec::new();
        image::RgbaImage::new(3, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(source.image_dimensions(&png), Some((3, 2)));
        assert_eq!(source.image_dimensions(b"nope"), None);
    }
}
