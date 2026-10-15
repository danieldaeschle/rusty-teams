use chatsvc::RecordingPolicy;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    pub async fn recording_policy(&self) -> Result<RecordingPolicy> {
        self.remote.recording_policy().await
    }

    pub async fn own_tenant_id(&self, chat_id: &str) -> Result<String> {
        self.my_tenant_id(chat_id).await
    }
}
