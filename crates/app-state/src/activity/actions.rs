use std::sync::Arc;

use gpui_kit::*;

use crate::app_state::{AppEvent, AppState};
use crate::backend::Engine;

impl AppState {
    pub fn mark_activity_read(&mut self, message_ids: Vec<String>, cx: &mut Context<Self>) {
        let Some(engine) = self.activity_engine() else {
            return;
        };
        let receiver = crate::runtime::spawn(async move {
            for message_id in &message_ids {
                engine.mark_activity_read(message_id).await?;
            }
            Ok::<(), teams_core::Error>(())
        });
        cx.spawn(async move |this, cx| {
            if !matches!(receiver.await, Ok(Ok(()))) {
                this.update(cx, |_, cx| cx.emit(AppEvent::Activity)).ok();
            }
        })
        .detach();
    }

    pub fn mark_all_activity_read(&mut self, cx: &mut Context<Self>) {
        let Some(Some(engine)) = self.chat_action_engine(cx) else {
            return;
        };
        self.run_chat_action(
            "Mark all as read",
            async move { engine.mark_all_activity_read().await },
            |_, cx| cx.emit(AppEvent::Activity),
            |_, _| {},
            cx,
        );
    }

    fn activity_engine(&self) -> Option<Arc<Engine>> {
        self.engine.clone().filter(|_| !self.mode.read_only)
    }
}
