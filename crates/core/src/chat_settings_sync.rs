use chatsvc::{
    ChatNotificationMode, ChatSettingChange, ChatSettingsDocument, DesktopNotifications,
};
use store::Store;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::events::CoreEvent;
use crate::remote::Remote;

const META_CHAT_SETTINGS: &str = "account.chat_settings";

pub fn stored_chat_settings(store: &Store) -> ChatSettingsDocument {
    store
        .meta(META_CHAT_SETTINGS)
        .ok()
        .flatten()
        .map(|text| ChatSettingsDocument::from_text(&text))
        .unwrap_or_default()
}

pub fn store_chat_settings(store: &Store, document: &ChatSettingsDocument) -> Result<()> {
    Ok(store.set_meta(META_CHAT_SETTINGS, &document.to_text())?)
}

impl<R: Remote> SyncEngine<R> {
    pub async fn refresh_chat_settings(&self) -> Result<ChatSettingsDocument> {
        let document = self.remote.chat_settings().await?;
        store_chat_settings(&self.store, &document)?;
        Ok(document)
    }

    pub async fn save_chat_setting(&self, change: ChatSettingChange) -> Result<ChatSettingsDocument> {
        let document = self.remote.save_chat_setting(change).await?;
        store_chat_settings(&self.store, &document)?;
        Ok(document)
    }

    pub fn chat_desktop_notifications(&self, chat_id: &str) -> DesktopNotifications {
        self.chat_states
            .lock()
            .unwrap()
            .get(chat_id)
            .map(|state| state.desktop_notifications())
            .unwrap_or_default()
    }

    pub async fn set_chat_notifications(
        &self,
        chat_id: &str,
        mode: ChatNotificationMode,
        desktop: DesktopNotifications,
    ) -> Result<()> {
        let current = self
            .chat_states
            .lock()
            .unwrap()
            .get(chat_id)
            .and_then(|state| state.notification_settings.clone());
        let write = self
            .remote
            .set_chat_notifications(chat_id, current.as_deref(), mode, desktop)
            .await?;
        {
            let mut states = self.chat_states.lock().unwrap();
            let state = states.entry(chat_id.to_owned()).or_default();
            state.id = chat_id.to_owned();
            state.muted = !write.alerts;
            state.notification_settings = Some(write.settings);
        }
        self.store.set_chat_muted(chat_id, !write.alerts)?;
        let _ = self.events.send(CoreEvent::SidebarChanged);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chatsvc::MessageDensity;

    use super::*;

    #[test]
    fn the_document_survives_a_restart_in_the_store() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(stored_chat_settings(&store), ChatSettingsDocument::default());
        let mut document = ChatSettingsDocument::default();
        document.apply(ChatSettingChange::MessageDensity(MessageDensity::Compact));
        store_chat_settings(&store, &document).unwrap();
        assert_eq!(stored_chat_settings(&store), document);
    }
}
