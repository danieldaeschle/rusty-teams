use std::collections::HashMap;

use gpui_kit::Context;
use store::Store;

use crate::app_state::{AppEvent, AppState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalPreview {
    Draft(String),
    NotSent,
}

pub fn load_local_previews(store: &Store) -> HashMap<String, LocalPreview> {
    let mut previews: HashMap<String, LocalPreview> = store
        .draft_previews()
        .unwrap_or_default()
        .into_iter()
        .map(|(conversation_id, text)| (conversation_id, LocalPreview::Draft(text)))
        .collect();
    for conversation_id in store.failed_outbox_conversations().unwrap_or_default() {
        previews.insert(conversation_id, LocalPreview::NotSent);
    }
    previews
}

impl AppState {
    pub fn refresh_local_previews(&mut self, cx: &mut Context<Self>) {
        let previews = load_local_previews(&self.store);
        if previews != self.local_previews {
            self.local_previews = previews;
            cx.emit(AppEvent::LocalPreviews);
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use store::{DraftRecord, OutboxRecord, OutboxState, OutboxTarget};

    use super::*;

    fn failed_row(conversation_id: &str) -> OutboxRecord {
        OutboxRecord {
            id: "row".to_owned(),
            conversation_id: conversation_id.to_owned(),
            target: OutboxTarget::Flat,
            thread_root_id: None,
            payload: "{}".to_owned(),
            images: Vec::new(),
            state: OutboxState::Failed,
            last_error: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn a_failed_send_beats_the_draft_in_the_same_chat() {
        let store = Store::open_in_memory().unwrap();
        for (conversation_id, preview) in [("a", "typing"), ("b", "also typing")] {
            store
                .save_draft(&DraftRecord {
                    conversation_id: conversation_id.to_owned(),
                    payload: "{}".to_owned(),
                    preview: preview.to_owned(),
                    images: Vec::new(),
                    updated_at: Utc::now(),
                })
                .unwrap();
        }
        store.put_outbox(&failed_row("a")).unwrap();
        let previews = load_local_previews(&store);
        assert_eq!(previews["a"], LocalPreview::NotSent);
        assert_eq!(previews["b"], LocalPreview::Draft("also typing".to_owned()));
    }
}
