use graph::{DriveEntry, DriveFolder, UploadDestination, UploadedFile};

use crate::engine::{Conversation, SyncEngine};
use crate::error::{Error, Result};
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    pub async fn channel_library_root(&self, channel_id: &str) -> Result<DriveEntry> {
        let Conversation::Channel { team_id } = self.resolve(channel_id)? else {
            return Err(Error::Unsupported("a library for a chat"));
        };
        self.remote.channel_files_root(&team_id, channel_id).await
    }

    pub async fn channel_tab_web_url(
        &self,
        channel_id: &str,
        tab_id: &str,
    ) -> Result<Option<String>> {
        let Conversation::Channel { team_id } = self.resolve(channel_id)? else {
            return Err(Error::Unsupported("a tab link for a chat"));
        };
        self.remote
            .channel_tab_web_url(&team_id, channel_id, tab_id)
            .await
    }

    pub async fn library_children(&self, folder: &DriveFolder) -> Result<Vec<DriveEntry>> {
        self.remote.list_children(folder).await
    }

    pub async fn create_library_folder(
        &self,
        parent: &DriveFolder,
        name: &str,
    ) -> Result<DriveEntry> {
        self.remote.create_folder(parent, name).await
    }

    pub async fn upload_to_library(
        &self,
        folder: &DriveFolder,
        file_name: &str,
        bytes: &[u8],
        progress: impl Fn(u8) + Send + Sync,
    ) -> Result<UploadedFile> {
        self.remote
            .upload_file(
                &UploadDestination::Folder(folder.clone()),
                file_name,
                bytes,
                &progress,
            )
            .await
    }
}
