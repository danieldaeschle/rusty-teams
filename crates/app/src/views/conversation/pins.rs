use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::*;
use store::MessageRecord;

use super::ConversationView;
use crate::app_state::Selection;
use crate::rows::reply_excerpt;
use crate::runtime;
use crate::views::message_actions::Action;
use crate::views::pin_banner::{PinBannerActions, PinBannerData, cycle_pin, render_pin_banner};

pub(super) struct PinPreview {
    author: Option<String>,
    text: String,
}

impl PinPreview {
    fn of(record: &MessageRecord) -> Self {
        PinPreview {
            author: record.sender_name.clone(),
            text: reply_excerpt(record),
        }
    }
}

impl ConversationView {
    pub(super) fn chat_id(&self) -> Option<String> {
        match self.current.as_ref()?.selection {
            Selection::Chat(ref chat_id) => Some(chat_id.clone()),
            Selection::Channel(_) => None,
        }
    }

    pub(super) fn refresh_pin_previews(&mut self, cx: &mut Context<Self>) {
        self.pin_previews.clear();
        self.pin_fetches.clear();
        self.pin_index = 0;
        let Some(chat_id) = self.chat_id() else {
            return;
        };
        self.update_pin_previews(&chat_id, cx);
        cx.notify();
    }

    pub(super) fn on_pins_changed(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        if self.chat_id().as_deref() != Some(chat_id) {
            return;
        }
        self.pin_index = 0;
        self.update_pin_previews(chat_id, cx);
        cx.notify();
    }

    fn update_pin_previews(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let ids: Vec<String> = state
            .pinned_messages(chat_id)
            .iter()
            .map(|pin| pin.message_id.clone())
            .collect();
        let records = state
            .store
            .messages_by_id(chat_id, &ids)
            .unwrap_or_default();
        let mut previews: HashMap<String, PinPreview> = records
            .into_iter()
            .map(|(message_id, record)| (message_id, PinPreview::of(&record)))
            .collect();
        for id in &ids {
            if !previews.contains_key(id)
                && let Some(preview) = self.pin_previews.remove(id)
            {
                previews.insert(id.clone(), preview);
            }
        }
        self.pin_previews = previews;
        let count = ids.len();
        if self.pin_index >= count {
            self.pin_index = 0;
        }
        for id in ids {
            if !self.pin_previews.contains_key(&id) {
                self.fetch_pin_preview(chat_id, id, cx);
            }
        }
    }

    fn fetch_pin_preview(&mut self, chat_id: &str, message_id: String, cx: &mut Context<Self>) {
        let Some(engine) = self.app.read(cx).engine.clone() else {
            return;
        };
        if !self.pin_fetches.insert(message_id.clone()) {
            return;
        }
        let chat_id = chat_id.to_owned();
        let task_chat_id = chat_id.clone();
        let task_message_id = message_id.clone();
        let receiver = runtime::spawn(async move {
            engine
                .fetch_chat_message(&task_chat_id, &task_message_id)
                .await
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(record))) = receiver.await else {
                return;
            };
            this.update(cx, |this, cx| {
                if this.chat_id().as_deref() == Some(chat_id.as_str()) {
                    this.pin_previews.insert(message_id, PinPreview::of(&record));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn pin_action(
        &self,
        view: WeakEntity<Self>,
        run: fn(&mut Self, &mut Window, &mut Context<Self>),
    ) -> Action {
        Rc::new(move |window, cx| {
            view.update(cx, |this, cx| run(this, window, cx)).ok();
        })
    }

    pub(super) fn render_pin_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chat_id = self.chat_id()?;
        let pins = self.app.read(cx).pinned_messages(&chat_id);
        let pin = pins.get(self.pin_index)?;
        let preview = self.pin_previews.get(&pin.message_id);
        let data = PinBannerData {
            author: preview.and_then(|preview| preview.author.clone()),
            preview: preview.map(|preview| preview.text.clone()),
            position: self.pin_index,
            total: pins.len(),
        };
        let view = cx.weak_entity();
        let actions = PinBannerActions {
            open: self.pin_action(view.clone(), |this, _, cx| this.open_shown_pin(cx)),
            previous: self.pin_action(view.clone(), |this, _, cx| this.step_pin(-1, cx)),
            next: self.pin_action(view.clone(), |this, _, cx| this.step_pin(1, cx)),
            unpin: self.pin_action(view, |this, _, cx| this.unpin_shown_pin(cx)),
        };
        Some(render_pin_banner(data, actions))
    }

    fn shown_pin_id(&self, cx: &App) -> Option<String> {
        let chat_id = self.chat_id()?;
        self.app
            .read(cx)
            .pinned_messages(&chat_id)
            .get(self.pin_index)
            .map(|pin| pin.message_id.clone())
    }

    fn open_shown_pin(&mut self, cx: &mut Context<Self>) {
        let (Some(chat_id), Some(message_id)) = (self.chat_id(), self.shown_pin_id(cx)) else {
            return;
        };
        self.jump_to_message(&chat_id, &message_id, cx);
    }

    fn step_pin(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(chat_id) = self.chat_id() else {
            return;
        };
        let total = self.app.read(cx).pinned_messages(&chat_id).len();
        self.pin_index = cycle_pin(self.pin_index, total, delta);
        cx.notify();
    }

    fn unpin_shown_pin(&mut self, cx: &mut Context<Self>) {
        let (Some(chat_id), Some(message_id)) = (self.chat_id(), self.shown_pin_id(cx)) else {
            return;
        };
        self.app.update(cx, |state, cx| {
            state.set_pinned(&chat_id, &message_id, false, cx)
        });
    }

    pub(super) fn forward_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if let Some(conversation_id) = self.conversation_id() {
            self.app.update(cx, |state, cx| {
                state.request_forward(&conversation_id, message_id, cx)
            });
        }
    }

    pub(super) fn copy_link(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if let Some(conversation_id) = self.conversation_id() {
            self.app.update(cx, |state, cx| {
                state.copy_message_link(&conversation_id, message_id, cx)
            });
        }
    }

    pub(super) fn toggle_saved(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if let Some(conversation_id) = self.conversation_id() {
            self.app.update(cx, |state, cx| {
                state.toggle_saved(&conversation_id, message_id, cx)
            });
        }
    }

    pub(super) fn toggle_pinned(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if let Some(chat_id) = self.chat_id() {
            self.app.update(cx, |state, cx| {
                state.toggle_pinned(&chat_id, message_id, cx)
            });
        }
    }

    pub(super) fn mark_unread_from(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if let Some(chat_id) = self.chat_id() {
            self.app.update(cx, |state, cx| {
                state.mark_unread_from_message(&chat_id, message_id, cx)
            });
        }
    }
}
