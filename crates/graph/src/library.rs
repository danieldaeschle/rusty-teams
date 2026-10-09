use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use session::{Method, Scope};

use crate::client::Graph;
use crate::error::{Error, Result};
use crate::files::{DriveFolder, FILES_SCOPE};
use crate::urls;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "RawEntry")]
pub struct DriveEntry {
    pub drive_id: String,
    pub id: String,
    pub name: String,
    pub web_url: String,
    pub size: u64,
    pub modified_at: Option<DateTime<Utc>>,
    pub modified_by: Option<String>,
    pub child_count: Option<u64>,
    pub download_url: Option<String>,
}

impl DriveEntry {
    pub fn is_folder(&self) -> bool {
        self.child_count.is_some()
    }

    pub fn folder(&self) -> DriveFolder {
        DriveFolder {
            drive_id: self.drive_id.clone(),
            item_id: self.id.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    id: String,
    name: String,
    #[serde(default)]
    web_url: String,
    #[serde(default)]
    size: u64,
    last_modified_date_time: Option<DateTime<Utc>>,
    last_modified_by: Option<RawActor>,
    folder: Option<RawFolder>,
    #[serde(default, rename = "@microsoft.graph.downloadUrl")]
    download_url: Option<String>,
    parent_reference: Option<RawParent>,
}

#[derive(Deserialize)]
struct RawActor {
    user: Option<RawIdentity>,
    application: Option<RawIdentity>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIdentity {
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFolder {
    #[serde(default)]
    child_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawParent {
    drive_id: Option<String>,
}

impl From<RawEntry> for DriveEntry {
    fn from(raw: RawEntry) -> Self {
        let modified_by = raw.last_modified_by.and_then(|actor| {
            actor
                .user
                .or(actor.application)
                .and_then(|identity| identity.display_name)
        });
        DriveEntry {
            drive_id: raw
                .parent_reference
                .and_then(|parent| parent.drive_id)
                .unwrap_or_default(),
            id: raw.id,
            name: raw.name,
            web_url: raw.web_url,
            size: raw.size,
            modified_at: raw.last_modified_date_time,
            modified_by,
            child_count: raw.folder.map(|folder| folder.child_count),
            download_url: raw.download_url.filter(|url| !url.is_empty()),
        }
    }
}

impl Graph {
    pub async fn channel_files_root(&self, team_id: &str, channel_id: &str) -> Result<DriveEntry> {
        let body = self
            .get(
                &urls::channel_files_folder(team_id, channel_id),
                &Scope::graph(FILES_SCOPE),
            )
            .await?;
        let entry: DriveEntry = serde_json::from_value(body)?;
        if entry.drive_id.is_empty() {
            return Err(Error::Upload("the channel folder has no drive".into()));
        }
        Ok(entry)
    }

    pub async fn list_children(&self, folder: &DriveFolder) -> Result<Vec<DriveEntry>> {
        self.get_all(
            &urls::folder_children(&folder.drive_id, &folder.item_id),
            Scope::graph(FILES_SCOPE),
        )
        .await
    }

    pub async fn create_folder(&self, parent: &DriveFolder, name: &str) -> Result<DriveEntry> {
        let answer = self
            .session()
            .request(
                Method::Post,
                &urls::folder_create(&parent.drive_id, &parent.item_id),
                &Scope::graph(FILES_SCOPE),
                Some(json!({
                    "name": name,
                    "folder": {},
                    "@microsoft.graph.conflictBehavior": "rename",
                })),
            )
            .await?;
        let mut entry: DriveEntry = serde_json::from_value(answer.body)?;
        if entry.drive_id.is_empty() {
            entry.drive_id.clone_from(&parent.drive_id);
        }
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use session::Scope;

    use super::*;
    use crate::page::Page;

    fn page(body: serde_json::Value) -> Page<DriveEntry> {
        Page::parse(body, Scope::graph(FILES_SCOPE)).unwrap()
    }

    #[test]
    fn children_parse_files_and_folders() {
        let parsed = page(json!({"value": [
            {
                "id": "f1", "name": "Plans", "webUrl": "https://x/Plans", "size": 0,
                "folder": {"childCount": 3},
                "lastModifiedDateTime": "2026-10-01T08:30:00Z",
                "lastModifiedBy": {"user": {"displayName": "Mara Lindqvist"}},
                "parentReference": {"driveId": "b!d"}
            },
            {
                "id": "i1", "name": "Plan.pdf", "webUrl": "https://x/Plan.pdf", "size": 2048,
                "file": {"mimeType": "application/pdf"},
                "@microsoft.graph.downloadUrl": "https://dl.example/plan",
                "lastModifiedBy": {"application": {"displayName": "Flow"}},
                "parentReference": {"driveId": "b!d"}
            }
        ]}));
        let [folder, file] = parsed.items.as_slice() else {
            panic!("two entries");
        };
        assert!(folder.is_folder());
        assert_eq!(folder.child_count, Some(3));
        assert_eq!(folder.modified_by.as_deref(), Some("Mara Lindqvist"));
        assert_eq!(folder.folder().drive_id, "b!d");
        assert!(!file.is_folder());
        assert_eq!(file.size, 2048);
        assert_eq!(file.modified_by.as_deref(), Some("Flow"));
        assert_eq!(
            file.download_url.as_deref(),
            Some("https://dl.example/plan")
        );
        assert_eq!(file.modified_at, None);
    }

    #[test]
    fn a_next_link_continues_the_listing() {
        let first = page(json!({
            "value": [{"id": "1", "name": "a"}],
            "@odata.nextLink": "https://graph.microsoft.com/v1.0/drives/d/items/i/children?$skiptoken=x"
        }));
        let last = page(json!({"value": [{"id": "2", "name": "b"}]}));
        assert!(first.next_link.is_some());
        assert!(last.next_link.is_none());
    }

    #[test]
    fn listing_keeps_the_download_url_by_not_selecting_fields() {
        let url = urls::folder_children("b!d", "01ABC");
        assert!(url.ends_with("/children?$top=200"));
        assert!(!url.contains("$select"));
    }
}
