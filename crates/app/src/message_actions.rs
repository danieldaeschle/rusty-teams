use std::collections::HashSet;

use chrono::{DateTime, Local, Offset, Utc};
use gpui_kit::*;
use store::MessageRecord;
use teams_core::{PinnedMessage, SavedMessage};

use crate::app_state::{AppEvent, AppState, Selection};
use crate::format;
use crate::notice::short_error;
use crate::notify::selection_for;
use crate::rows::{message_text, reply_excerpt};
use crate::runtime;

#[derive(Default)]
pub struct SavedSet {
    items: Vec<SavedMessage>,
    keys: HashSet<(String, String)>,
}

impl SavedSet {
    pub fn items(&self) -> &[SavedMessage] {
        &self.items
    }

    pub fn contains(&self, conversation_id: &str, message_id: &str) -> bool {
        self.keys
            .contains(&(conversation_id.to_owned(), message_id.to_owned()))
    }

    pub fn replace(&mut self, items: Vec<SavedMessage>) {
        self.keys = items.iter().map(saved_key).collect();
        self.items = items;
    }

    pub fn insert(&mut self, item: SavedMessage) {
        if self.keys.insert(saved_key(&item)) {
            self.items.insert(0, item);
        }
    }

    pub fn remove(&mut self, conversation_id: &str, message_id: &str) -> Option<SavedMessage> {
        let key = (conversation_id.to_owned(), message_id.to_owned());
        if !self.keys.remove(&key) {
            return None;
        }
        let position = self.items.iter().position(|item| saved_key(item) == key)?;
        Some(self.items.remove(position))
    }
}

fn saved_key(item: &SavedMessage) -> (String, String) {
    (item.conversation_id.clone(), item.message_id.clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSource {
    pub conversation_id: String,
    pub message_id: String,
    pub author: String,
    pub time: String,
    pub text: String,
}

fn saved_entry(record: &MessageRecord, is_channel: bool) -> SavedMessage {
    SavedMessage {
        conversation_id: record.conversation_id.clone(),
        message_id: record.message_id.clone(),
        root_id: record
            .reply_to_id
            .clone()
            .filter(|_| is_channel)
            .unwrap_or_else(|| record.message_id.clone()),
        author_id: record.sender_id.clone(),
        author_name: record.sender_name.clone(),
        preview: reply_excerpt(record),
        saved_at: Utc::now(),
        topic: None,
    }
}

impl AppState {
    pub(crate) fn message_record(&self, conversation_id: &str, message_id: &str) -> Option<MessageRecord> {
        self.store
            .messages_by_id(conversation_id, &[message_id.to_owned()])
            .ok()?
            .remove(message_id)
            .filter(|record| !record.deleted)
    }

    pub fn is_saved(&self, conversation_id: &str, message_id: &str) -> bool {
        self.saved.contains(conversation_id, message_id)
    }

    pub fn pinned_messages(&self, chat_id: &str) -> &[PinnedMessage] {
        self.pins.get(chat_id).map_or(&[], Vec::as_slice)
    }

    pub fn is_pinned(&self, chat_id: &str, message_id: &str) -> bool {
        self.pinned_messages(chat_id)
            .iter()
            .any(|pin| pin.message_id == message_id)
    }

    pub fn refresh_saved(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let receiver = runtime::spawn(async move { engine.list_saved().await });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(items)) = receiver.await {
                this.update(cx, |state, cx| {
                    state.saved.replace(items);
                    cx.emit(AppEvent::Saved);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn toggle_saved(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .saved
            .items()
            .iter()
            .find(|item| item.conversation_id == conversation_id && item.message_id == message_id)
            .cloned();
        match existing {
            Some(entry) => self.set_saved(entry, false, cx),
            None => {
                let Some(record) = self.message_record(conversation_id, message_id) else {
                    return;
                };
                let is_channel = matches!(
                    selection_for(&self.sidebar, conversation_id),
                    Some(Selection::Channel(_))
                );
                self.set_saved(saved_entry(&record, is_channel), true, cx);
            }
        }
    }

    pub fn set_saved(&mut self, entry: SavedMessage, saved: bool, cx: &mut Context<Self>) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let restore = if saved {
            self.saved.insert(entry.clone());
            None
        } else {
            self.saved.remove(&entry.conversation_id, &entry.message_id)
        };
        cx.emit(AppEvent::Saved);
        cx.notify();
        let Some(engine) = engine_or_demo else {
            return;
        };
        let (conversation_id, message_id) =
            (entry.conversation_id.clone(), entry.message_id.clone());
        let root_id = (entry.root_id != entry.message_id).then(|| entry.root_id.clone());
        self.run_chat_action(
            if saved { "Save" } else { "Unsave" },
            async move {
                engine
                    .set_saved(&conversation_id, &message_id, root_id.as_deref(), saved)
                    .await
            },
            move |state, cx| {
                if saved {
                    state
                        .saved
                        .remove(&entry.conversation_id, &entry.message_id);
                } else {
                    state.saved.insert(restore.unwrap_or(entry));
                }
                cx.emit(AppEvent::Saved);
                cx.notify();
            },
            |_, _| {},
            cx,
        );
    }

    pub fn refresh_pins(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let chat_id = chat_id.to_owned();
        let requested_id = chat_id.clone();
        let receiver = runtime::spawn(async move { engine.chat_pins(&requested_id).await });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(pins)) = receiver.await {
                this.update(cx, |state, cx| state.store_pins(&chat_id, pins, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn store_pins(&mut self, chat_id: &str, pins: Vec<PinnedMessage>, cx: &mut Context<Self>) {
        self.pins.insert(chat_id.to_owned(), pins);
        cx.emit(AppEvent::Pins(chat_id.to_owned()));
        cx.notify();
    }

    pub fn toggle_pinned(&mut self, chat_id: &str, message_id: &str, cx: &mut Context<Self>) {
        let pinned = self.is_pinned(chat_id, message_id);
        self.set_pinned(chat_id, message_id, !pinned, cx);
    }

    pub fn set_pinned(
        &mut self,
        chat_id: &str,
        message_id: &str,
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let previous = self.pinned_messages(chat_id).to_vec();
        let parent_id = previous
            .iter()
            .find(|pin| pin.message_id == message_id)
            .and_then(|pin| pin.parent_id.clone());
        let mut updated: Vec<PinnedMessage> = previous
            .iter()
            .filter(|pin| pin.message_id != message_id)
            .cloned()
            .collect();
        if pinned {
            updated.insert(
                0,
                PinnedMessage {
                    message_id: message_id.to_owned(),
                    pinned_at: Some(Utc::now()),
                    parent_id: None,
                },
            );
        }
        self.store_pins(chat_id, updated, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let (owned_chat, owned_message, revert_chat) = (
            chat_id.to_owned(),
            message_id.to_owned(),
            chat_id.to_owned(),
        );
        self.run_chat_action(
            if pinned { "Pin" } else { "Unpin" },
            async move {
                if pinned {
                    engine.pin_message(&owned_chat, &owned_message).await
                } else {
                    engine
                        .unpin_message(&owned_chat, &owned_message, parent_id.as_deref())
                        .await
                }
            },
            move |state, cx| state.store_pins(&revert_chat, previous, cx),
            |_, _| {},
            cx,
        );
    }

    pub fn copy_message_link(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = self.engine.clone() else {
            if !self.mode.demo {
                return;
            }
            let link = teams_core::chat_message_link(conversation_id, message_id, None);
            cx.write_to_clipboard(ClipboardItem::new_string(link));
            self.raise_notice("Link copied".to_owned(), None, cx);
            return;
        };
        let (conversation_id, message_id) = (conversation_id.to_owned(), message_id.to_owned());
        let receiver =
            runtime::spawn(async move { engine.message_link(&conversation_id, &message_id).await });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(link)) => Ok(link),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(_) => Err("cancelled".to_owned()),
            };
            this.update(cx, |state, cx| match outcome {
                Ok(link) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(link));
                    state.raise_notice("Link copied".to_owned(), None, cx);
                }
                Err(reason) => state.raise_notice(format!("Copy link failed: {reason}"), None, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn request_forward(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) {
        if self.mode.read_only {
            self.raise_notice("Read-only mode: message not forwarded".to_owned(), None, cx);
            return;
        }
        let Some(record) = self.message_record(conversation_id, message_id) else {
            return;
        };
        let now = Local::now();
        self.forward_request = Some(ForwardSource {
            conversation_id: record.conversation_id.clone(),
            message_id: record.message_id.clone(),
            author: record
                .sender_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_owned()),
            time: forward_time(record.created_at, now),
            text: message_text(&record),
        });
        cx.emit(AppEvent::Forward);
        cx.notify();
    }

    pub fn take_forward_request(&mut self) -> Option<ForwardSource> {
        self.forward_request.take()
    }

    pub fn forward_locally(
        &mut self,
        source: &ForwardSource,
        target: &Selection,
        comment: &str,
        cx: &mut Context<Self>,
    ) {
        let Selection::Chat(chat_id) = target else {
            return;
        };
        let Some(chat) = self.sidebar.chats.iter().find(|chat| &chat.id == chat_id) else {
            return;
        };
        let (chat, record) =
            crate::demo::forwarded_message(chat, source, comment, chrono::Utc::now());
        let _ = self.store.upsert_chats(&[chat]);
        let _ = self.store.upsert_messages(&[record]);
        self.reload_sidebar(cx);
        cx.emit(AppEvent::Messages(chat_id.clone()));
    }

    pub fn mark_unread_from_message(
        &mut self,
        chat_id: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.message_record(chat_id, message_id) else {
            return;
        };
        self.mark_chat_unread_from(chat_id, Some(record.created_at), cx);
    }
}

fn forward_time(time: DateTime<Utc>, now: DateTime<Local>) -> String {
    format::list_time_label(time, now.date_naive(), now.offset().fix())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use teams_core::SavedMessage;

    use super::SavedSet;

    fn saved(message_id: &str) -> SavedMessage {
        SavedMessage {
            conversation_id: "chat".into(),
            message_id: message_id.into(),
            root_id: message_id.into(),
            author_id: None,
            author_name: None,
            preview: "text".into(),
            saved_at: Utc.with_ymd_and_hms(2026, 10, 9, 8, 0, 0).unwrap(),
            topic: None,
        }
    }

    #[test]
    fn saving_then_unsaving_a_message_updates_the_lookup() {
        let mut set = SavedSet::default();
        set.insert(saved("m1"));
        assert!(set.contains("chat", "m1"));
        assert!(!set.contains("other", "m1"));
        assert!(set.remove("chat", "m1").is_some());
        assert!(!set.contains("chat", "m1"));
        assert!(set.items().is_empty());
    }

    #[test]
    fn newest_saved_message_comes_first() {
        let mut set = SavedSet::default();
        set.insert(saved("m1"));
        set.insert(saved("m2"));
        set.insert(saved("m2"));
        let ids: Vec<&str> = set
            .items()
            .iter()
            .map(|item| item.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m2", "m1"]);
    }

    #[test]
    fn replacing_the_set_rebuilds_the_lookup() {
        let mut set = SavedSet::default();
        set.insert(saved("m1"));
        set.replace(vec![saved("m2")]);
        assert!(!set.contains("chat", "m1"));
        assert!(set.contains("chat", "m2"));
        assert_eq!(
            set.remove("chat", "m2").map(|item| item.message_id),
            Some("m2".into())
        );
        assert_eq!(set.remove("chat", "m2"), None);
    }
}
