use chrono::{DateTime, Utc};
use gpui_kit::*;
use teams_core::{JumpPlan, WindowDirection, WindowPage, jump_plan};

use super::ConversationView;
use crate::notice::short_error;
use crate::runtime;

pub(super) struct DetachedWindow {
    pub(super) anchor_id: String,
    pub(super) oldest: DateTime<Utc>,
    pub(super) newest: DateTime<Utc>,
    pub(super) older_cursor: Option<String>,
    pub(super) newer_cursor: Option<String>,
    pub(super) loading: bool,
}

impl ConversationView {
    pub(super) fn is_detached(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.detached.is_some())
    }

    pub(super) fn jump_plan_for(
        &self,
        chat_id: &str,
        target_created_at: Option<DateTime<Utc>>,
        cx: &App,
    ) -> JumpPlan {
        let oldest_loaded = self
            .app
            .read(cx)
            .store
            .sync_state(chat_id)
            .ok()
            .flatten()
            .and_then(|state| state.oldest_loaded);
        let window = self
            .current
            .as_ref()
            .and_then(|current| current.detached.as_ref())
            .map(|detached| (detached.oldest, detached.newest));
        jump_plan(window, target_created_at, oldest_loaded)
    }

    pub(super) fn open_detached(
        &mut self,
        chat_id: String,
        message_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = self.app.read(cx).engine.clone() else {
            return;
        };
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.window_request.as_deref() == Some(message_id.as_str()) {
            return;
        }
        current.window_request = Some(message_id.clone());
        cx.notify();
        let task_chat_id = chat_id.clone();
        let task_message_id = message_id.clone();
        let receiver =
            runtime::spawn(
                async move { engine.load_window(&task_chat_id, &task_message_id).await },
            );
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            this.update(cx, |this, cx| {
                this.finish_open_detached(&chat_id, message_id, result, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_open_detached(
        &mut self,
        chat_id: &str,
        message_id: String,
        result: Result<teams_core::Result<WindowPage>, tokio::sync::oneshot::error::RecvError>,
        cx: &mut Context<Self>,
    ) {
        if self.chat_id().as_deref() != Some(chat_id) {
            return;
        }
        let Some(current) = self
            .current
            .as_mut()
            .filter(|current| current.window_request.as_deref() == Some(message_id.as_str()))
        else {
            return;
        };
        current.window_request = None;
        cx.notify();
        let page = match result {
            Ok(Ok(page)) => page,
            Ok(Err(error)) => {
                self.notice = Some(format!(
                    "Could not load the message: {}",
                    short_error(&error)
                ));
                cx.notify();
                return;
            }
            Err(_) => return,
        };
        let (Some(oldest), Some(newest)) = (page.oldest, page.newest) else {
            self.notice = Some("Could not find the message".to_owned());
            cx.notify();
            return;
        };
        let Some(current) = self.current.as_mut() else {
            return;
        };
        current.first_unread = None;
        current.detached = Some(DetachedWindow {
            anchor_id: message_id.clone(),
            oldest,
            newest,
            older_cursor: page.older_cursor,
            newer_cursor: page.newer_cursor,
            loading: false,
        });
        self.reload_pending(cx);
        self.rebuild(true, cx);
        self.reveal_row(message_id, cx);
    }

    pub(super) fn leave_detached(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.detached.take().is_none() {
            return;
        }
        self.rebuild(true, cx);
    }

    pub(super) fn request_window_page(
        &mut self,
        direction: WindowDirection,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = self.app.read(cx).engine.clone() else {
            return;
        };
        let Some(chat_id) = self.chat_id() else {
            return;
        };
        let Some(detached) = self
            .current
            .as_mut()
            .and_then(|current| current.detached.as_mut())
        else {
            return;
        };
        let cursor = match direction {
            WindowDirection::Backward => detached.older_cursor.clone(),
            WindowDirection::Forward => detached.newer_cursor.clone(),
            WindowDirection::Bidirectional => None,
        };
        let Some(cursor) = cursor.filter(|_| !detached.loading) else {
            return;
        };
        detached.loading = true;
        let anchor_id = detached.anchor_id.clone();
        let task_chat_id = chat_id.clone();
        let receiver = runtime::spawn(async move {
            engine
                .load_window_page(&task_chat_id, &cursor, direction)
                .await
        });
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            this.update(cx, |this, cx| {
                this.finish_window_page(&chat_id, &anchor_id, direction, result, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_window_page(
        &mut self,
        chat_id: &str,
        anchor_id: &str,
        direction: WindowDirection,
        result: Result<teams_core::Result<WindowPage>, tokio::sync::oneshot::error::RecvError>,
        cx: &mut Context<Self>,
    ) {
        if self.chat_id().as_deref() != Some(chat_id) {
            return;
        }
        let Some(detached) = self
            .current
            .as_mut()
            .and_then(|current| current.detached.as_mut())
            .filter(|detached| detached.anchor_id == anchor_id)
        else {
            return;
        };
        detached.loading = false;
        let page = match result {
            Ok(Ok(page)) => page,
            failure => {
                match direction {
                    WindowDirection::Backward => detached.older_cursor = None,
                    _ => detached.newer_cursor = None,
                }
                if let Ok(Err(error)) = failure {
                    self.notice = Some(format!("Could not load messages: {}", short_error(&error)));
                }
                self.rebuild(false, cx);
                return;
            }
        };
        match direction {
            WindowDirection::Backward => {
                detached.older_cursor = page.older_cursor;
                detached.oldest = page
                    .oldest
                    .map_or(detached.oldest, |oldest| oldest.min(detached.oldest));
            }
            _ => {
                detached.newer_cursor = page.newer_cursor;
                detached.newest = page
                    .newest
                    .map_or(detached.newest, |newest| newest.max(detached.newest));
            }
        }
        let reached_latest = direction == WindowDirection::Forward
            && (detached.newer_cursor.is_none() || self.window_reaches_newest_cached(chat_id, cx));
        if reached_latest {
            self.leave_detached(cx);
        } else {
            self.rebuild(false, cx);
        }
    }

    fn window_reaches_newest_cached(&self, chat_id: &str, cx: &App) -> bool {
        let Some(newest) = self
            .current
            .as_ref()
            .and_then(|current| current.detached.as_ref())
            .map(|detached| detached.newest)
        else {
            return false;
        };
        self.app
            .read(cx)
            .store
            .messages(chat_id, None, 1)
            .ok()
            .and_then(|latest| latest.last().map(|record| record.created_at))
            .is_none_or(|latest| newest >= latest)
    }
}
