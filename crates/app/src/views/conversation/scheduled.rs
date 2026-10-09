use std::rc::Rc;

use chrono::{DateTime, Utc};
use gpui_kit::*;
use store::OutboxTarget;
use teams_core::ScheduledDraft;
use uuid::Uuid;

use super::{ConversationView, HoverSlot, ViewMode};
use crate::backend::Engine;
use crate::notice::{short_error, truncated};
use crate::outbox::{deliver, new_outbox_id, outbox_record};
use crate::rows::MessageRow;
use crate::runtime;
use crate::scheduled::{conversation_of, thread_root_of};
use crate::scheduled_rows::{
    draft_id_of, draft_key, scheduled_feed_rows, scheduled_outgoing, scheduled_rows,
};
use crate::views::composer::{EditPreview, Outgoing};
use crate::views::message_actions::{Action, HoverChange};
use crate::views::message_row::{DeliveryActions, RowAction};
use crate::views::scheduled_toolbar::ScheduledMenu;
use std::sync::Arc;

const EXCERPT_CHARS: usize = 140;

#[derive(Clone)]
pub(super) enum ScheduledAction {
    Update(String),
    SendNow,
    Delete,
}

pub(super) struct ScheduledFailure {
    draft_id: String,
    action: ScheduledAction,
}

pub(super) struct ScheduledRowActions {
    pub delivery: DeliveryActions,
    pub menu: Option<ScheduledMenu>,
    pub hovered: HoverChange,
}

fn demo_draft(inner_thread_id: &str, html: &str, send_at: DateTime<Utc>) -> ScheduledDraft {
    ScheduledDraft {
        id: Uuid::new_v4().to_string(),
        client_message_id: Uuid::new_v4().to_string(),
        inner_thread_id: inner_thread_id.to_owned(),
        send_at,
        html: html.to_owned(),
        delivery_state: None,
    }
}

impl ConversationView {
    fn scheduled_thread_id(&self) -> Option<String> {
        let current = self.current.as_ref()?;
        let conversation_id = current.selection.conversation_id();
        Some(match &current.mode {
            ViewMode::Thread(root_id) => format!("{conversation_id};messageid={root_id}"),
            _ => conversation_id.to_owned(),
        })
    }

    pub(super) fn scheduled_rows(&self, my_user_id: Option<&str>, cx: &App) -> Vec<MessageRow> {
        let (Some(inner_thread_id), Some(conversation_id)) =
            (self.scheduled_thread_id(), self.conversation_id())
        else {
            return Vec::new();
        };
        let change_failed = self
            .scheduled_failure
            .as_ref()
            .map(|failure| failure.draft_id.as_str());
        let drafts = &self.app.read(cx).scheduled;
        if self.in_feed() {
            return scheduled_feed_rows(
                drafts,
                &conversation_id,
                my_user_id,
                Utc::now(),
                change_failed,
            );
        }
        scheduled_rows(
            drafts,
            &inner_thread_id,
            &conversation_id,
            my_user_id,
            Utc::now(),
            change_failed,
        )
    }

    fn scheduled_draft(&self, draft_id: &str, cx: &App) -> Option<ScheduledDraft> {
        self.app
            .read(cx)
            .scheduled
            .iter()
            .find(|draft| draft_key(draft) == draft_id)
            .cloned()
    }

    fn scheduled_engine(&mut self, cx: &mut Context<Self>) -> Option<Arc<Engine>> {
        let state = self.app.read(cx);
        let engine = state.engine.clone().filter(|_| !state.mode.read_only);
        if engine.is_none() {
            self.notice = Some("Read-only mode: nothing was changed".to_owned());
            cx.notify();
        }
        engine
    }

    pub(super) fn schedule(
        &mut self,
        outgoing: Outgoing,
        send_at: DateTime<Utc>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(inner_thread_id), Some(conversation_id)) =
            (self.scheduled_thread_id(), self.conversation_id())
        else {
            return;
        };
        if self.in_feed() && self.has_subject(cx) {
            self.notice = Some("Can't schedule a post with a subject yet".to_owned());
            self.composer
                .update(cx, |composer, cx| composer.fail_schedule(cx));
            cx.notify();
            return;
        }
        let html = outgoing.html();
        if self.app.read(cx).mode.demo {
            let draft = demo_draft(&inner_thread_id, &html, send_at);
            self.app
                .update(cx, |state, cx| state.scheduled_upserted(draft, cx));
            self.finish_scheduling(&conversation_id, window, cx);
            return;
        }
        let Some(engine) = self.scheduled_engine(cx) else {
            self.composer
                .update(cx, |composer, cx| composer.fail_schedule(cx));
            return;
        };
        let thread_root_id = match self.current.as_ref().map(|current| &current.mode) {
            Some(ViewMode::Thread(root_id)) => Some(root_id.clone()),
            _ => None,
        };
        let target = conversation_id.clone();
        let receiver = runtime::spawn(async move {
            engine
                .schedule_message(&target, thread_root_id.as_deref(), &html, send_at)
                .await
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(draft)) => {
                    this.app
                        .update(cx, |state, cx| state.scheduled_upserted(draft, cx));
                    this.finish_scheduling(&conversation_id, window, cx);
                }
                Ok(Err(error)) => this.fail_scheduling(&short_error(&error), cx),
                Err(error) => this.fail_scheduling(&short_error(&error), cx),
            })
            .ok();
        })
        .detach();
    }

    fn finish_scheduling(
        &mut self,
        conversation_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notice = None;
        self.composer.update(cx, |composer, cx| {
            composer.finish_schedule(conversation_id, window, cx)
        });
        if self.in_feed() {
            self.close_new_post(cx);
        }
        self.rebuild(false, cx);
        if !self.in_feed() {
            self.scroller
                .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
        }
    }

    fn fail_scheduling(&mut self, error: &str, cx: &mut Context<Self>) {
        self.notice = Some(format!("Couldn't schedule: {error}"));
        self.composer
            .update(cx, |composer, cx| composer.fail_schedule(cx));
        cx.notify();
    }

    pub(super) fn begin_scheduled_edit(
        &mut self,
        draft_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self
            .scheduled_draft(draft_id, cx)
            .filter(|draft| !draft.id.is_empty())
        else {
            return;
        };
        let mut outgoing = scheduled_outgoing(&draft.html);
        let excerpt = outgoing
            .text()
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| truncated(line, EXCERPT_CHARS))
            .unwrap_or_default();
        outgoing.edit = Some(EditPreview {
            message_id: draft.id,
            excerpt,
            scheduled: Some(draft.send_at),
        });
        self.show_main_composer(cx);
        self.composer
            .update(cx, |composer, cx| composer.begin_edit(outgoing, window, cx));
    }

    pub(super) fn send_scheduled_edit(
        &mut self,
        outgoing: Outgoing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = outgoing.edit.as_ref() else {
            return;
        };
        self.change_scheduled(
            &edit.message_id.clone(),
            ScheduledAction::Update(outgoing.html()),
            window,
            cx,
        );
    }

    pub(super) fn send_scheduled_now(
        &mut self,
        draft_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_scheduled(draft_id, ScheduledAction::SendNow, window, cx);
    }

    pub(super) fn delete_scheduled(
        &mut self,
        draft_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_scheduled(draft_id, ScheduledAction::Delete, window, cx);
    }

    pub(super) fn retry_scheduled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(failure) = self.scheduled_failure.take() {
            self.change_scheduled(&failure.draft_id, failure.action, window, cx);
        }
    }

    fn change_scheduled(
        &mut self,
        draft_id: &str,
        action: ScheduledAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self
            .scheduled_draft(draft_id, cx)
            .filter(|draft| !draft.id.is_empty())
        else {
            return;
        };
        if !self.scheduled_in_flight.insert(draft.id.clone()) {
            return;
        }
        self.scheduled_failure = None;
        if self.app.read(cx).mode.demo {
            self.scheduled_in_flight.remove(&draft.id);
            self.apply_scheduled_change(&draft, &action, cx);
            return;
        }
        let Some(engine) = self.scheduled_engine(cx) else {
            self.scheduled_in_flight.remove(&draft.id);
            return;
        };
        let (call_draft, call_action) = (draft.clone(), action.clone());
        let receiver = runtime::spawn(async move {
            match call_action {
                ScheduledAction::Update(html) => {
                    engine.update_scheduled(&call_draft, &html).await.map(Some)
                }
                ScheduledAction::SendNow | ScheduledAction::Delete => {
                    engine.cancel_scheduled(&call_draft.id).await.map(|_| None)
                }
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            this.update_in(cx, |this, _, cx| {
                this.scheduled_in_flight.remove(&draft.id);
                match result {
                    Ok(Ok(Some(updated))) => {
                        this.app
                            .update(cx, |state, cx| state.scheduled_upserted(updated, cx));
                        this.rebuild(false, cx);
                    }
                    Ok(Ok(None)) => this.apply_scheduled_change(&draft, &action, cx),
                    _ => {
                        this.scheduled_failure = Some(ScheduledFailure {
                            draft_id: draft.id.clone(),
                            action,
                        });
                        this.rebuild(false, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_scheduled_change(
        &mut self,
        draft: &ScheduledDraft,
        action: &ScheduledAction,
        cx: &mut Context<Self>,
    ) {
        match action {
            ScheduledAction::Update(html) => {
                let updated = ScheduledDraft {
                    html: html.clone(),
                    ..draft.clone()
                };
                self.app
                    .update(cx, |state, cx| state.scheduled_upserted(updated, cx));
            }
            ScheduledAction::Delete => self
                .app
                .update(cx, |state, cx| state.scheduled_removed(&draft.id, cx)),
            ScheduledAction::SendNow => {
                self.app
                    .update(cx, |state, cx| state.scheduled_removed(&draft.id, cx));
                self.deliver_now(draft, cx);
            }
        }
        self.rebuild(false, cx);
    }

    fn deliver_now(&mut self, draft: &ScheduledDraft, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let (Some(engine), store) = (state.engine.clone(), state.store.clone()) else {
            return;
        };
        if state.mode.demo {
            return;
        }
        let conversation_id = conversation_of(&draft.inner_thread_id).to_owned();
        let thread_root_id = thread_root_of(&draft.inner_thread_id).map(str::to_owned);
        let target = match (&thread_root_id, store.channel(&conversation_id)) {
            (Some(_), _) => OutboxTarget::Thread,
            (None, Ok(Some(_))) => OutboxTarget::Post,
            (None, _) => OutboxTarget::Flat,
        };
        let outgoing = scheduled_outgoing(&draft.html);
        let outbox_id = new_outbox_id();
        if let Some(record) = outbox_record(
            &outbox_id,
            &conversation_id,
            target,
            thread_root_id.as_deref(),
            &outgoing,
            Utc::now(),
        ) {
            store.put_outbox(&record).ok();
        }
        let app = self.app.clone();
        app.update(cx, |state, cx| {
            state.outbox_changed(conversation_id.clone(), cx)
        });
        let delivery_target = conversation_id.clone();
        let receiver = runtime::spawn(async move {
            deliver(
                &engine,
                &delivery_target,
                target,
                thread_root_id.as_deref(),
                &outgoing,
            )
            .await
        });
        cx.spawn(async move |_, cx| {
            let failure = match receiver.await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(short_error(&error)),
                Err(error) => Some(short_error(&error)),
            };
            match failure {
                None => store.delete_outbox(&outbox_id).ok(),
                Some(error) => store.mark_outbox_failed(&outbox_id, &error).ok(),
            };
            app.update(cx, |state, cx| state.outbox_changed(conversation_id, cx));
        })
        .detach();
    }

    pub(super) fn scheduled_row_actions(
        &self,
        row: &MessageRow,
        view: &WeakEntity<Self>,
        window: &Window,
        cx: &App,
    ) -> Option<ScheduledRowActions> {
        let draft_id = draft_id_of(&row.key)?.to_owned();
        let handle = window.window_handle();
        let link = |run: fn(&mut Self, &str, &mut Window, &mut Context<Self>)| -> RowAction {
            let (view, draft_id) = (view.clone(), draft_id.clone());
            Some(Box::new(move |cx: &mut App| {
                let (view, draft_id) = (view.clone(), draft_id.clone());
                handle
                    .update(cx, move |_, window, cx| {
                        view.update(cx, |this, cx| run(this, &draft_id, window, cx))
                            .ok();
                    })
                    .ok();
            }))
        };
        let retry: RowAction = {
            let view = view.clone();
            Some(Box::new(move |cx: &mut App| {
                let view = view.clone();
                handle
                    .update(cx, move |_, window, cx| {
                        view.update(cx, |this, cx| this.retry_scheduled(window, cx))
                            .ok();
                    })
                    .ok();
            }))
        };
        let action = |run: fn(&mut Self, &str, &mut Window, &mut Context<Self>)| -> Action {
            let (view, draft_id) = (view.clone(), draft_id.clone());
            Rc::new(move |window, cx| {
                view.update(cx, |this, cx| run(this, &draft_id, window, cx))
                    .ok();
            })
        };
        let hover = |slot: HoverSlot| -> HoverChange {
            let (view, key) = (view.clone(), row.key.clone());
            Rc::new(move |hovered: bool, cx: &mut App| {
                view.update(cx, |this, cx| this.set_hover(slot, &key, hovered, cx))
                    .ok();
            })
        };
        let visible = view
            .upgrade()
            .is_some_and(|entity| entity.read(cx).toolbar_visible(&row.key));
        Some(ScheduledRowActions {
            delivery: DeliveryActions {
                retry,
                delete: link(Self::delete_scheduled),
                send_now: link(Self::send_scheduled_now),
            },
            menu: visible.then(|| ScheduledMenu {
                key: row.key.clone(),
                edit: action(Self::begin_scheduled_edit),
                send_now: action(Self::send_scheduled_now),
                delete: action(Self::delete_scheduled),
                hover: hover(HoverSlot::Toolbar),
            }),
            hovered: hover(HoverSlot::Row),
        })
    }
}
