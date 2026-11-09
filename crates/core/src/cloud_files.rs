use chatsvc::substrate_search::{QueryRequest, QueryScope, SearchFile, SearchSession};
use graph::{CloudFile, FileReference};

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::remote::Remote;
use crate::stored::FileKind;

const RECENT_FETCH: usize = 40;
pub const RECENT_SHOWN: usize = 20;
const SEARCH_SIZE: usize = 20;

pub(crate) fn recent_documents(files: Vec<CloudFile>) -> Vec<CloudFile> {
    files
        .into_iter()
        .filter(|file| FileKind::from_name(&file.name).is_document())
        .take(RECENT_SHOWN)
        .collect()
}

fn found_file(file: SearchFile) -> Option<CloudFile> {
    Some(CloudFile {
        name: file.name,
        size: file.size.unwrap_or(0),
        web_url: file.url?,
        drive_id: None,
        item_id: None,
        last_opened: file.modified_at,
    })
}

impl<R: Remote> SyncEngine<R> {
    /// The documents Teams lists under "Recent cloud files", newest first.
    pub async fn recent_cloud_files(&self) -> Result<Vec<CloudFile>> {
        Ok(recent_documents(self.remote.recent_files(RECENT_FETCH).await?))
    }

    pub async fn search_cloud_files(&self, search_session: &SearchSession, text: &str) -> Result<Vec<CloudFile>> {
        let request = QueryRequest {
            size: SEARCH_SIZE,
            ..QueryRequest::new(text, QueryScope::Files)
        };
        let results = self.server_query(search_session, &request).await?;
        Ok(results.files.items.into_iter().filter_map(found_file).collect())
    }

    /// Shares `file` with the chat like an upload; a file the user cannot share still goes out as a link.
    pub async fn attach_cloud_file(&self, conversation_id: &str, file: &CloudFile) -> Result<FileReference> {
        let uploaded = self.remote.cloud_file_reference(file).await?;
        let reference = uploaded
            .reference()
            .ok_or_else(|| Error::Unsupported("a cloud file without an eTag"))?;
        self.share_attachment(conversation_id, &uploaded).await.ok();
        Ok(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> CloudFile {
        CloudFile {
            name: name.to_owned(),
            size: 0,
            web_url: format!("https://x/{name}"),
            drive_id: None,
            item_id: None,
            last_opened: None,
        }
    }

    #[test]
    fn recent_cloud_files_keep_documents_like_teams() {
        let names: Vec<String> = recent_documents(vec![file("a.pdf"), file("clip.mp4"), file("b.pptx"), file("c.jpg"), file("d.xlsx")])
            .into_iter()
            .map(|file| file.name)
            .collect();
        assert_eq!(names, ["a.pdf", "b.pptx", "d.xlsx"]);
    }
}
