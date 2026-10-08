use graph::{DOWNLOAD_CHUNK_BYTES, Error as GraphError, SharedFile, download_ranges, percent_done};

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    /// Streams the file behind a OneDrive or SharePoint link chunk by chunk into `write`.
    pub async fn download_file(
        &self,
        open_url: &str,
        mut write: impl FnMut(&[u8]) -> std::io::Result<()> + Send,
        progress: impl Fn(u8) + Send + Sync,
    ) -> Result<SharedFile> {
        let shared = self.remote.resolve_share(open_url).await?;
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
