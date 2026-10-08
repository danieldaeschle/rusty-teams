use std::future::Future;

use chrono::{Duration, Local, Offset, Utc};
use gpui_kit::*;
use store::ChatRecord;

use crate::app_state::{AppState, Selection, chat_title};
use crate::notice::{NoticeAction, short_error, truncated};
use crate::sidebar_model::{SectionInput, build_sections, next_chat_id};

pub const TITLE_LIMIT: usize = 40;

impl AppState {
    pub fn keeps_unread(&self, chat_id: &str) -> bool {
        self.keep_unread.as_deref() == Some(chat_id)
    }

    pub fn mark_chat_unread(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(last_message_at) = chat.last_message_at else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let last_read_at = last_message_at - Duration::milliseconds(1);
        let _ = self.store.mark_chat_unread(chat_id, last_read_at);
        if self.selection == Some(Selection::Chat(chat_id.to_owned())) {
            self.keep_unread = Some(chat_id.to_owned());
        }
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        let revert_id = owned_id.clone();
        let restored_read_at = chat.last_read_at.unwrap_or(last_message_at);
        self.run_chat_action(
            "Mark as unread",
            async move { engine.mark_unread(&owned_id).await },
            move |state, cx| {
                let _ = state.store.mark_chat_read(&revert_id, restored_read_at);
                if state.keep_unread.as_deref() == Some(revert_id.as_str()) {
                    state.keep_unread = None;
                }
                state.reload_sidebar(cx);
            },
            |_, _| {},
            cx,
        );
    }

    pub fn set_chat_muted(&mut self, chat_id: &str, muted: bool, cx: &mut Context<Self>) {
        if self.chat_record(chat_id).is_none() {
            return;
        }
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let _ = self.store.set_chat_muted(chat_id, muted);
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        let revert_id = owned_id.clone();
        self.run_chat_action(
            if muted { "Mute" } else { "Unmute" },
            async move { engine.set_chat_muted(&owned_id, muted).await },
            move |state, cx| {
                let _ = state.store.set_chat_muted(&revert_id, !muted);
                state.reload_sidebar(cx);
            },
            |_, _| {},
            cx,
        );
    }

    pub fn hide_chat(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        self.remove_chat_locally(chat_id, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        let undo_id = owned_id.clone();
        let text = format!("\"{}\" hidden", truncated(&chat_title(&chat), TITLE_LIMIT));
        self.run_chat_action(
            "Hide",
            async move { engine.hide_chat(&owned_id).await },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            move |state, cx| {
                let undo =
                    NoticeAction::new("Undo", move |state, cx| state.unhide_chat(&undo_id, cx));
                state.raise_notice(text, Some(undo), cx);
            },
            cx,
        );
    }

    pub fn unhide_chat(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(engine) = self.chat_action_engine(cx).flatten() else {
            return;
        };
        let owned_id = chat_id.to_owned();
        self.run_chat_action(
            "Undo",
            async move { engine.unhide_chat(&owned_id).await },
            |_, _| {},
            |_, _| {},
            cx,
        );
    }

    pub fn leave_chat(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        self.remove_chat_locally(chat_id, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        self.run_chat_action(
            "Leave chat",
            async move { engine.leave_chat(&owned_id).await },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            |_, _| {},
            cx,
        );
    }

    fn chat_record(&self, chat_id: &str) -> Option<ChatRecord> {
        self.sidebar
            .chats
            .iter()
            .find(|chat| chat.id == chat_id)
            .cloned()
    }

    /// `None` stops the action. `Some(None)` is demo mode, which only changes the local store.
    fn chat_action_engine(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Option<std::sync::Arc<crate::backend::Engine>>> {
        if self.mode.read_only {
            self.raise_notice("Read-only mode: chat not changed".to_owned(), None, cx);
            return None;
        }
        match self.engine.clone() {
            Some(engine) => Some(Some(engine)),
            None if self.mode.demo => Some(None),
            None => None,
        }
    }

    fn remove_chat_locally(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let was_selected = self.selection == Some(Selection::Chat(chat_id.to_owned()));
        let next = was_selected
            .then(|| self.next_chat_after(chat_id))
            .flatten();
        let _ = self.store.remove_chats(&[chat_id.to_owned()]);
        self.reload_sidebar(cx);
        if was_selected {
            match next {
                Some(next_id) => self.select(Selection::Chat(next_id), cx),
                None => {
                    self.selection = None;
                    self.keep_unread = None;
                    cx.emit(crate::app_state::AppEvent::Selection);
                    cx.notify();
                }
            }
        }
    }

    fn restore_chat_locally(&mut self, chat: &ChatRecord, cx: &mut Context<Self>) {
        let _ = self.store.upsert_chats(std::slice::from_ref(chat));
        let _ = self.store.set_chat_muted(&chat.id, chat.muted);
        self.reload_sidebar(cx);
    }

    fn next_chat_after(&self, chat_id: &str) -> Option<String> {
        let input = SectionInput {
            chats: &self.sidebar.chats,
            directory: &self.directory,
            collapsed: &self.collapsed,
            now: Utc::now(),
            offset: Local::now().offset().fix(),
        };
        next_chat_id(&build_sections(&input), chat_id)
    }

    fn run_chat_action(
        &mut self,
        label: &'static str,
        call: impl Future<Output = teams_core::Result<()>> + Send + 'static,
        revert: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        on_success: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let receiver = crate::runtime::spawn(call);
        cx.spawn(async move |this, cx| {
            let failure = match receiver.await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(short_error(&error)),
                Err(_) => Some("cancelled".to_owned()),
            };
            this.update(cx, |state, cx| match failure {
                None => on_success(state, cx),
                Some(reason) => {
                    revert(state, cx);
                    state.raise_notice(format!("{label} failed: {reason}"), None, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}
