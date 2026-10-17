use gpui_kit::*;
use store::MessageRecord;

use super::ConversationView;
use crate::notice::short_error;
use crate::restorable_deletes::RESTORE_FAILED_NOTICE;
use crate::runtime;

impl ConversationView {
    pub(super) fn delete(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let Some(record) = self.own_record(message_id, cx) else {
            return;
        };
        let state = self.app.read(cx);
        let (mode, engine) = (state.mode, state.engine.clone());
        if mode.demo {
            self.remember_delete(&record, cx);
            self.store_deleted(record, true, cx);
            return;
        }
        let Some(engine) = engine.filter(|_| !mode.read_only) else {
            self.show_notice("Read-only mode: message not deleted", cx);
            return;
        };
        self.remember_delete(&record, cx);
        let (conversation_id, message_id) = (record.conversation_id, record.message_id);
        let (failed_conversation, failed_message) = (conversation_id.clone(), message_id.clone());
        let receiver = runtime::spawn(async move {
            engine
                .soft_delete_message(&conversation_id, &message_id)
                .await
        });
        cx.spawn(async move |this, cx| {
            let error = match receiver.await {
                Ok(Ok(())) => return,
                Ok(Err(error)) => short_error(&error),
                Err(_) => "cancelled".to_owned(),
            };
            this.update(cx, |this, cx| {
                this.app.update(cx, |state, _| {
                    state
                        .restorable_deletes
                        .forget(&failed_conversation, &failed_message)
                });
                this.show_notice(&format!("Delete failed: {error}"), cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn undo_delete(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let Some(record) = self.deleted_own_record(message_id, cx) else {
            return;
        };
        let state = self.app.read(cx);
        let (mode, engine) = (state.mode, state.engine.clone());
        if mode.read_only && !mode.demo {
            return;
        }
        let forgotten = self.app.update(cx, |state, _| {
            state
                .restorable_deletes
                .forget(&record.conversation_id, &record.message_id)
        });
        if !forgotten {
            return;
        }
        let engine = engine.filter(|_| !mode.demo);
        self.store_deleted(record.clone(), false, cx);
        let Some(engine) = engine else {
            return;
        };
        let (conversation_id, message_id) =
            (record.conversation_id.clone(), record.message_id.clone());
        let receiver = runtime::spawn(async move {
            engine
                .undo_soft_delete_message(&conversation_id, &message_id)
                .await
        });
        cx.spawn(async move |this, cx| {
            if matches!(receiver.await, Ok(Ok(()))) {
                return;
            }
            this.update(cx, |this, cx| {
                this.remember_delete(&record, cx);
                this.store_deleted(record, true, cx);
                this.show_notice(RESTORE_FAILED_NOTICE, cx);
            })
            .ok();
        })
        .detach();
    }

    fn remember_delete(&mut self, record: &MessageRecord, cx: &mut Context<Self>) {
        self.app.update(cx, |state, _| {
            state
                .restorable_deletes
                .remember(&record.conversation_id, &record.message_id)
        });
    }

    fn store_deleted(&mut self, mut record: MessageRecord, deleted: bool, cx: &mut Context<Self>) {
        record.deleted = deleted;
        let _ = self
            .app
            .read(cx)
            .store
            .upsert_messages(std::slice::from_ref(&record));
        self.rebuild(false, cx);
    }

    fn deleted_own_record(&self, message_id: &str, cx: &App) -> Option<MessageRecord> {
        let conversation_id = self.conversation_id()?;
        let my_user_id = self.row_context(cx).my_user_id?;
        self.app
            .read(cx)
            .store
            .messages_by_id(&conversation_id, &[message_id.to_owned()])
            .ok()?
            .remove(message_id)
            .filter(|record| record.deleted && record.sender_id.as_deref() == Some(&my_user_id))
    }
}
