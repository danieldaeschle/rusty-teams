use chrono::{DateTime, Utc};
use serde_json::Value;
use session::{Method, Scope};

use crate::client::Graph;
use crate::error::Result;
use crate::files::{FILES_SCOPE, UploadedFile, finished_item, share_id};
use crate::urls;

/// A file in OneDrive or SharePoint that can be attached by reference, without uploading it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFile {
    pub name: String,
    pub size: u64,
    pub web_url: String,
    pub drive_id: Option<String>,
    pub item_id: Option<String>,
    pub last_opened: Option<DateTime<Utc>>,
}

fn time(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

fn recent_file(item: &Value) -> Option<CloudFile> {
    let remote = item.get("remoteItem").filter(|remote| remote.is_object()).unwrap_or(item);
    item.get("file")?;
    let opened = |source: &Value| time(&source["fileSystemInfo"]["lastAccessedDateTime"]);
    Some(CloudFile {
        name: item["name"].as_str()?.to_owned(),
        size: item["size"].as_u64().or_else(|| remote["size"].as_u64()).unwrap_or(0),
        web_url: item["webUrl"].as_str().or_else(|| remote["webUrl"].as_str())?.to_owned(),
        drive_id: remote["parentReference"]["driveId"].as_str().map(str::to_owned),
        item_id: remote["id"].as_str().map(str::to_owned),
        last_opened: opened(remote).or_else(|| opened(item)),
    })
}

pub fn parse_recent_files(body: &Value) -> Vec<CloudFile> {
    body["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(recent_file)
        .collect()
}

impl Graph {
    pub async fn recent_files(&self, top: usize) -> Result<Vec<CloudFile>> {
        let answer = self
            .session()
            .request(Method::Get, &urls::recent_drive_items(top), &Scope::graph(FILES_SCOPE), None)
            .await?;
        Ok(parse_recent_files(&answer.body))
    }

    /// The reference a message needs to carry `file`: its drive item, found by id or else by its link.
    pub async fn cloud_file_reference(&self, file: &CloudFile) -> Result<UploadedFile> {
        let url = match (&file.drive_id, &file.item_id) {
            (Some(drive_id), Some(item_id)) => urls::drive_item(drive_id, item_id),
            _ => urls::shared_drive_item(&share_id(&file.web_url)),
        };
        let answer = self
            .session()
            .request(Method::Get, &url, &Scope::graph(FILES_SCOPE), None)
            .await?;
        finished_item(answer.body)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn recent_files_take_ids_and_open_time_from_the_remote_item() {
        let body = json!({"value": [
            {"name": "plan.pdf", "size": 12, "webUrl": "https://x/plan.pdf", "file": {},
             "fileSystemInfo": {"lastAccessedDateTime": "2026-11-09T10:00:00Z"},
             "remoteItem": {"id": "item-1", "parentReference": {"driveId": "drive-1"},
                            "fileSystemInfo": {"lastAccessedDateTime": "2026-11-09T12:00:00Z"}}},
            {"name": "Folder", "webUrl": "https://x/folder", "folder": {}},
            {"name": "notes.docx", "webUrl": "https://x/notes.docx", "file": {}, "id": "item-2",
             "parentReference": {"driveId": "drive-2"}}
        ]});
        let files = parse_recent_files(&body);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].item_id.as_deref(), Some("item-1"));
        assert_eq!(files[0].drive_id.as_deref(), Some("drive-1"));
        assert_eq!(files[0].last_opened.unwrap().to_rfc3339(), "2026-11-09T12:00:00+00:00");
        assert_eq!((files[1].item_id.as_deref(), files[1].size), (Some("item-2"), 0));
        assert!(files[1].last_opened.is_none());
    }
}
