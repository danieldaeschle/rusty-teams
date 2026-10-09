use std::time::Duration;

use gpui_kit::component::input::InputContent;
use gpui_kit::*;
use store::{AttachmentImage, DraftRecord};
use teams_core::Draft;

use super::{Composer, Outgoing};
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
    if text.is_empty() {
        return IMAGE_ONLY_PREVIEW.to_owned();
    }
    truncated(&text, DRAFT_PREVIEW_CHARS)
}

impl Composer {
    pub(super) fn schedule_draft_save(&mut self, cx: &mut Context<Self>) {
        self.draft_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DRAFT_SAVE_DELAY).await;
            this.update(cx, |composer, cx| composer.save_draft_now(cx))
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
        (!outgoing.draft.is_blank() || !outgoing.images.is_empty()).then_some(outgoing)
    }

    pub fn save_draft_now(&mut self, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.storable_conversation() else {
            return;
        };
        let store = self.app.read(cx).store.clone();
        let stored = self
            .storable_draft(cx)
            .and_then(|outgoing| Some((draft_preview(&outgoing), encode(&outgoing)?)));
        match stored {
            Some((preview, stored)) => {
                let current = fingerprint(&conversation_id, &stored.images);
                let unchanged = self.saved_images.as_ref() == Some(&current);
                let record = DraftRecord {
                    conversation_id,
                    payload: stored.payload,
                    preview,
                    images: stored.images,
                    updated_at: chrono::Utc::now(),
                };
                let saved = if unchanged {
                    store.save_draft_text(&record)
                } else {
                    store.save_draft(&record)
                };
                self.saved_images = saved.is_ok().then_some(current);
            }
            None => {
                self.saved_images = None;
                store.delete_draft(&conversation_id).ok();
            }
        }
        self.app
            .update(cx, |state, cx| state.refresh_local_previews(cx));
    }

    pub(super) fn clear_stored_draft(&mut self, cx: &mut Context<Self>) {
        self.draft_save = None;
        self.saved_images = None;
        let Some(conversation_id) = self.storable_conversation() else {
            return;
        };
        self.app.read(cx).store.delete_draft(&conversation_id).ok();
        self.app
            .update(cx, |state, cx| state.refresh_local_previews(cx));
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
