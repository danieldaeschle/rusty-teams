use gpui_kit::Context;
use store::Store;
use teams_core::{
    ChatAccountSettings, ChatNotificationMode, ChatSettingChange, ChatSettingsDocument,
    DesktopNotifications,
};

use crate::app_state::{AppEvent, AppState};
use crate::rows::Receipt;
use crate::runtime;

const META_MARKDOWN_MODE: &str = "compose.markdown_mode";

pub fn stored_markdown_mode(store: &Store) -> bool {
    store.meta(META_MARKDOWN_MODE).ok().flatten().as_deref() == Some("true")
}

/// Teams shows no read state at all while the own read receipts setting is off.
pub fn shown_receipt(settings: &ChatAccountSettings, receipt: Receipt) -> Receipt {
    if settings.read_receipts {
        receipt
    } else {
        Receipt::Hidden
    }
}

impl AppState {
    pub fn account_chat_settings(&self) -> ChatAccountSettings {
        self.chat_settings.settings()
    }

    pub fn refresh_chat_settings(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let read = runtime::spawn(async move { engine.refresh_chat_settings().await });
        cx.spawn(async move |this, cx| {
            let document = match read.await {
                Ok(Ok(document)) => document,
                Ok(Err(error)) => {
                    log::warn!("chat settings not read: {error}");
                    return;
                }
                Err(_) => return,
            };
            this.update(cx, |state, cx| state.apply_chat_settings(document, cx))
                .ok();
        })
        .detach();
    }

    pub fn save_chat_setting(&mut self, change: ChatSettingChange, cx: &mut Context<Self>) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let previous = self.chat_settings.clone();
        let mut updated = previous.clone();
        updated.apply(change);
        self.apply_chat_settings(updated, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        self.run_chat_action(
            "Setting",
            async move { engine.save_chat_setting(change).await.map(|_| ()) },
            move |state, cx| state.apply_chat_settings(previous, cx),
            |_, _| {},
            cx,
        );
    }

    fn apply_chat_settings(&mut self, document: ChatSettingsDocument, cx: &mut Context<Self>) {
        if self.chat_settings == document {
            return;
        }
        let _ = teams_core::store_chat_settings(&self.store, &document);
        self.chat_settings = document;
        cx.emit(AppEvent::ChatSettings);
        cx.notify();
    }

    pub fn set_markdown_mode(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.markdown_mode = enabled;
        let _ = self
            .store
            .set_meta(META_MARKDOWN_MODE, if enabled { "true" } else { "false" });
        cx.emit(AppEvent::ChatSettings);
        cx.notify();
    }

    pub fn chat_desktop_notifications(&self, chat_id: &str) -> DesktopNotifications {
        match &self.engine {
            Some(engine) => engine.chat_desktop_notifications(chat_id),
            None => self
                .demo_chat_desktop
                .get(chat_id)
                .copied()
                .unwrap_or_default(),
        }
    }

    pub fn set_chat_notifications(
        &mut self,
        chat_id: &str,
        mode: ChatNotificationMode,
        desktop: DesktopNotifications,
        cx: &mut Context<Self>,
    ) {
        let Some(previous_muted) = self.chat_record(chat_id).map(|chat| chat.muted) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let muted = mode == ChatNotificationMode::Mute;
        let _ = self.store.set_chat_muted(chat_id, muted);
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            self.demo_chat_desktop.insert(chat_id.to_owned(), desktop);
            return;
        };
        let owned_id = chat_id.to_owned();
        let revert_id = owned_id.clone();
        self.run_chat_action(
            "Chat notifications",
            async move {
                engine
                    .set_chat_notifications(&owned_id, mode, desktop)
                    .await
            },
            move |state, cx| {
                let _ = state.store.set_chat_muted(&revert_id, previous_muted);
                state.reload_sidebar(cx);
            },
            |_, _| {},
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_disappear_while_the_own_setting_is_off() {
        let mut settings = ChatAccountSettings::default();
        assert_eq!(shown_receipt(&settings, Receipt::Read), Receipt::Read);
        assert_eq!(shown_receipt(&settings, Receipt::Sent), Receipt::Sent);
        settings.read_receipts = false;
        assert_eq!(shown_receipt(&settings, Receipt::Read), Receipt::Hidden);
        assert_eq!(shown_receipt(&settings, Receipt::Sent), Receipt::Hidden);
    }

    #[test]
    fn markdown_mode_is_a_local_setting_that_starts_off() {
        let store = Store::open_in_memory().unwrap();
        assert!(!stored_markdown_mode(&store));
        store.set_meta(META_MARKDOWN_MODE, "true").unwrap();
        assert!(stored_markdown_mode(&store));
    }
}
