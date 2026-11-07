use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::input::InputContent;
use gpui_kit::*;
use store::{AttachmentImage, DraftRecord, Store};
use teams_core::Draft;

use super::{Composer, Outgoing};
use crate::local_previews::load_local_previews;
use crate::notice::truncated;
use crate::stored_outgoing::{decode, encode};

const DRAFT_SAVE_DELAY: Duration = Duration::from_millis(500);
const DRAFT_PREVIEW_CHARS: usize = 200;
const IMAGE_ONLY_PREVIEW: &str = "Image";

pub(super) type ImageFingerprint = (String, Vec<(String, usize, Option<u32>, Option<u32>)>);

fn fingerprint(conversation_id: &str, images: &[AttachmentImage]) -> ImageFingerprint {
    let images = images
        .iter()
        .map(|image| {
            (
                image.name.clone(),
                image.bytes.len(),
                image.width,
                image.height,
            )
        })
        .collect();
    (conversation_id.to_owned(), images)
}

fn draft_preview(outgoing: &Outgoing) -> String {
    let text = outgoing
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let text = Some(text)
        .filter(|text| !text.is_empty())
        .or_else(|| outgoing.subject.clone())
        .or_else(|| outgoing.banner.as_ref().map(|banner| banner.title.clone()));
    let Some(text) = text else {
        return IMAGE_ONLY_PREVIEW.to_owned();
    };
    truncated(&text, DRAFT_PREVIEW_CHARS)
}

enum DraftWrite {
    Text(DraftRecord),
    Full(DraftRecord),
    Delete(String),
}

impl DraftWrite {
    fn apply(&self, store: &Store) -> bool {
        match self {
            Self::Text(record) => store.save_draft_text(record).is_ok(),
            Self::Full(record) => store.save_draft(record).is_ok(),
            Self::Delete(conversation_id) => store.delete_draft(conversation_id).is_ok(),
        }
    }
}

/// A write is skipped once a newer one has landed, so a late background save never
/// revives a sent draft or overwrites a newer one.
#[derive(Default)]
pub(super) struct DraftWrites {
    issued: u64,
    landed: Arc<Mutex<u64>>,
}

impl DraftWrites {
    fn next(&mut self) -> u64 {
        self.issued += 1;
        self.issued
    }

    fn apply(
        landed: &Mutex<u64>,
        sequence: u64,
        write: &DraftWrite,
        store: &Store,
    ) -> Option<bool> {
        let mut landed = landed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *landed >= sequence {
            return None;
        }
        *landed = sequence;
        Some(write.apply(store))
    }

    fn latest(landed: &Mutex<u64>, sequence: u64) -> bool {
        *landed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            == sequence
    }
}

impl Composer {
    pub fn schedule_draft_save(&mut self, cx: &mut Context<Self>) {
        self.draft_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DRAFT_SAVE_DELAY).await;
            let Ok(Some((write, fingerprint, store, landed, sequence))) =
                this.update(cx, |composer, cx| {
                    let (write, fingerprint) = composer.pending_draft_write(cx)?;
                    Some((
                        write,
                        fingerprint,
                        composer.app.read(cx).store.clone(),
                        composer.draft_writes.landed.clone(),
                        composer.draft_writes.next(),
                    ))
                })
            else {
                return;
            };
            let check = landed.clone();
            let result = cx
                .background_spawn(async move {
                    let saved = DraftWrites::apply(&landed, sequence, &write, &store)?;
                    Some((saved, load_local_previews(&store)))
                })
                .await;
            let Some((saved, previews)) = result else {
                return;
            };
            this.update(cx, |composer, cx| {
                if composer.draft_writes.issued == sequence {
                    composer.saved_images = fingerprint.filter(|_| saved);
                }
                if DraftWrites::latest(&check, sequence) {
                    composer
                        .app
                        .update(cx, |state, cx| state.set_local_previews(previews, cx));
                }
            })
            .ok();
        }));
    }

    fn storable_conversation(&self) -> Option<String> {
        self.conversation_id
            .clone()
            .filter(|conversation_id| !conversation_id.is_empty())
            .filter(|_| self.editing.is_none() && !self.inline)
    }

    fn storable_draft(&self, cx: &App) -> Option<Outgoing> {
        let mut outgoing = self.compose(cx);
        outgoing.files.clear();
        (!outgoing.draft.is_blank()
            || !outgoing.images.is_empty()
            || outgoing.subject.is_some()
            || outgoing.banner.is_some())
        .then_some(outgoing)
    }

    fn pending_draft_write(&self, cx: &App) -> Option<(DraftWrite, Option<ImageFingerprint>)> {
        let conversation_id = self.storable_conversation()?;
        let stored = self
            .storable_draft(cx)
            .and_then(|outgoing| Some((draft_preview(&outgoing), encode(&outgoing)?)));
        let Some((preview, stored)) = stored else {
            return Some((DraftWrite::Delete(conversation_id), None));
        };
        let current = fingerprint(&conversation_id, &stored.images);
        let unchanged = self.saved_images.as_ref() == Some(&current);
        let record = DraftRecord {
            conversation_id,
            payload: stored.payload,
            preview,
            images: stored.images,
            updated_at: chrono::Utc::now(),
        };
        let write = if unchanged {
            DraftWrite::Text(record)
        } else {
            DraftWrite::Full(record)
        };
        Some((write, Some(current)))
    }

    fn write_draft_now(&mut self, write: DraftWrite, cx: &mut Context<Self>) -> bool {
        let store = self.app.read(cx).store.clone();
        let sequence = self.draft_writes.next();
        let saved = DraftWrites::apply(&self.draft_writes.landed, sequence, &write, &store)
            .unwrap_or(false);
        self.app
            .update(cx, |state, cx| state.refresh_local_previews(cx));
        saved
    }

    pub fn save_draft_now(&mut self, cx: &mut Context<Self>) {
        let Some((write, fingerprint)) = self.pending_draft_write(cx) else {
            return;
        };
        let saved = self.write_draft_now(write, cx);
        self.saved_images = fingerprint.filter(|_| saved);
    }

    pub(super) fn clear_stored_draft(&mut self, cx: &mut Context<Self>) {
        self.draft_save = None;
        self.saved_images = None;
        let Some(conversation_id) = self.storable_conversation() else {
            return;
        };
        self.write_draft_now(DraftWrite::Delete(conversation_id), cx);
    }

    pub(super) fn load_stored_draft(
        &mut self,
        conversation_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stored = Some(conversation_id)
            .filter(|conversation_id| !conversation_id.is_empty() && !self.inline)
            .and_then(|conversation_id| self.app.read(cx).store.draft(conversation_id).ok())
            .flatten()
            .and_then(|record| decode(&record.payload, &record.images));
        match stored {
            Some(outgoing) => self.restore(&outgoing, window, cx),
            None => {
                self.mention_inputs.clear();
                self.load_draft(Draft::default(), InputContent::new(""), window, cx);
            }
        }
        self.draft_save = None;
    }
}

#[cfg(test)]
mod tests {
    use store::{DraftRecord, Store};

    use super::{DraftWrite, DraftWrites};

    fn record(text: &str) -> DraftRecord {
        DraftRecord {
            conversation_id: "chat".to_owned(),
            payload: "{}".to_owned(),
            preview: text.to_owned(),
            images: Vec::new(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_late_background_save_does_not_revive_a_deleted_draft() {
        let store = Store::open_in_memory().unwrap();
        let mut writes = DraftWrites::default();
        let background = writes.next();
        let delete = writes.next();
        let deleted = DraftWrite::Delete("chat".to_owned());
        assert_eq!(
            DraftWrites::apply(&writes.landed, delete, &deleted, &store),
            Some(true)
        );
        let late = DraftWrite::Full(record("typing"));
        assert_eq!(
            DraftWrites::apply(&writes.landed, background, &late, &store),
            None
        );
        assert!(store.draft("chat").unwrap().is_none());
    }
}
