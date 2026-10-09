use graph::{DOWNLOAD_CHUNK_BYTES, Error as GraphError, SharedFile, download_ranges, percent_done};

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::remote::Remote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryFile {
    pub drive_id: String,
    pub item_id: String,
    pub name: String,
    pub size: u64,
    pub cached_url: Option<String>,
}

impl<R: Remote> SyncEngine<R> {
    /// Streams the file behind a OneDrive or SharePoint link chunk by chunk into `write`.
    pub async fn download_file(
        &self,
        open_url: &str,
        mut write: impl FnMut(&[u8]) -> std::io::Result<()> + Send,
        progress: impl Fn(u8) + Send + Sync,
    ) -> Result<SharedFile> {
        let shared = self.remote.resolve_share(open_url).await?;
        self.stream_file(shared, &mut write, &progress).await
    }

    /// Like `download_file` for a library entry. A `cached_url` from the listing skips the lookup.
    pub async fn download_library_file(
        &self,
        file: &LibraryFile,
        mut write: impl FnMut(&[u8]) -> std::io::Result<()> + Send,
        progress: impl Fn(u8) + Send + Sync,
    ) -> Result<SharedFile> {
        let shared = match &file.cached_url {
            Some(download_url) => SharedFile {
                name: file.name.clone(),
                size: file.size,
                download_url: download_url.clone(),
            },
            None => {
                self.remote
                    .resolve_drive_item(&file.drive_id, &file.item_id)
                    .await?
            }
        };
        self.stream_file(shared, &mut write, &progress).await
    }

    async fn stream_file(
        &self,
        shared: SharedFile,
        write: &mut (impl FnMut(&[u8]) -> std::io::Result<()> + Send),
        progress: &(impl Fn(u8) + Send + Sync),
    ) -> Result<SharedFile> {
        progress(0);
        let ranges = download_ranges(shared.size, DOWNLOAD_CHUNK_BYTES);
        let last = ranges.len().saturating_sub(1);
        for (position, (start, end)) in ranges.into_iter().enumerate() {
            let is_last = position == last;
            let bytes = self
                .remote
                .download_range(&shared.download_url, start, (!is_last).then_some(end))
                .await?;
            let expected = end - start + 1;
            let received = bytes.len() as u64;
            if received < expected || (!is_last && received != expected) {
                return Err(GraphError::Download(format!(
                    "expected {expected} bytes, got {received}"
                ))
                .into());
            }
            write(&bytes)?;
            progress(percent_done(start + received, shared.size));
        }
        progress(100);
        Ok(shared)
    }
}
