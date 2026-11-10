use chrono::{DateTime, Utc};
use serde_json::Value;
use session::{Method, Scope};

use crate::client::Graph;
use crate::error::{Error, Result};
use crate::files::{DriveFolder, FILES_SCOPE, UploadedFile, finished_item, share_id};
use crate::library::DriveEntry;
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

const RECENT_FOR_SITES: usize = 100;
const SHARED_SHOWN: usize = 100;
const ROOT_ITEM: &str = "root";
const QUICK_ACCESS_SITES: usize = 8;

/// A SharePoint site under "Quick access".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSite {
    pub id: String,
    pub name: String,
}

impl From<&DriveEntry> for CloudFile {
    fn from(entry: &DriveEntry) -> Self {
        CloudFile {
            name: entry.name.clone(),
            size: entry.size,
            web_url: entry.web_url.clone(),
            drive_id: Some(entry.drive_id.clone()).filter(|id| !id.is_empty()),
            item_id: Some(entry.id.clone()),
            last_opened: None,
        }
    }
}

fn user_name(value: &Value) -> Option<String> {
    value["user"]["displayName"].as_str().filter(|name| !name.is_empty()).map(str::to_owned)
}

/// A file someone shared, as a picker row: `modified_at` holds the share date, `modified_by` who shared it.
fn shared_entry(item: &Value) -> Option<DriveEntry> {
    let remote = item.get("remoteItem").filter(|remote| remote.is_object())?;
    let shared = &remote["shared"];
    Some(DriveEntry {
        drive_id: remote["parentReference"]["driveId"].as_str()?.to_owned(),
        id: remote["id"].as_str()?.to_owned(),
        name: item["name"].as_str()?.to_owned(),
        web_url: item["webUrl"].as_str().or_else(|| remote["webUrl"].as_str())?.to_owned(),
        size: remote["size"].as_u64().unwrap_or(0),
        modified_at: time(&shared["sharedDateTime"]),
        modified_by: user_name(&shared["sharedBy"]).or_else(|| user_name(&shared["owner"])),
        child_count: remote.get("folder").map(|folder| folder["childCount"].as_u64().unwrap_or(0)),
        download_url: None,
    })
}

pub fn parse_shared(body: &Value) -> Vec<DriveEntry> {
    let mut shared: Vec<DriveEntry> = body["value"].as_array().into_iter().flatten().filter_map(shared_entry).collect();
    shared.sort_by(|first, second| second.modified_at.cmp(&first.modified_at));
    shared.truncate(SHARED_SHOWN);
    shared
}

/// The owner of each recent file, by item id, for the "Owner" column.
pub fn parse_recent_owners(body: &Value) -> Vec<(String, String)> {
    body["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let remote = item.get("remoteItem").filter(|remote| remote.is_object()).unwrap_or(item);
            let owner = user_name(&remote["shared"]["owner"]).or_else(|| user_name(&remote["createdBy"]))?;
            Some((remote["id"].as_str()?.to_owned(), owner))
        })
        .collect()
}

/// The SharePoint sites of recently opened files, most recent first, without personal OneDrives.
pub fn recent_site_ids(body: &Value, limit: usize) -> Vec<String> {
    let mut sites: Vec<String> = Vec::new();
    for item in body["value"].as_array().into_iter().flatten() {
        let remote = item.get("remoteItem").filter(|remote| remote.is_object()).unwrap_or(item);
        let web_url = remote["webUrl"].as_str().or_else(|| item["webUrl"].as_str()).unwrap_or_default();
        let site = remote["parentReference"]["siteId"].as_str().or_else(|| remote["sharepointIds"]["siteId"].as_str());
        if let Some(site) = site
            && !web_url.contains("/personal/")
            && !sites.iter().any(|known| known == site)
        {
            sites.push(site.to_owned());
        }
    }
    sites.truncate(limit);
    sites
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
        Ok(self.recent_files_with_owners(top).await?.0)
    }

    pub async fn recent_files_with_owners(&self, top: usize) -> Result<(Vec<CloudFile>, Vec<(String, String)>)> {
        let body = self.get(&urls::recent_drive_items(top), &Scope::graph(FILES_SCOPE)).await?;
        Ok((parse_recent_files(&body), parse_recent_owners(&body)))
    }

    pub async fn shared_with_me(&self) -> Result<Vec<DriveEntry>> {
        Ok(parse_shared(&self.get(&urls::shared_with_me(), &Scope::graph(FILES_SCOPE)).await?))
    }

    pub async fn quick_access_sites(&self) -> Result<Vec<QuickSite>> {
        let scope = Scope::graph(FILES_SCOPE);
        let recent = self.get(&urls::recent_drive_items(RECENT_FOR_SITES), &scope).await?;
        let mut sites = Vec::new();
        for site_id in recent_site_ids(&recent, QUICK_ACCESS_SITES) {
            if let Ok(site) = self.get(&urls::site(&site_id), &scope).await
                && let Some(name) = site["displayName"].as_str()
            {
                sites.push(QuickSite { id: site_id, name: name.to_owned() });
            }
        }
        Ok(sites)
    }

    /// The root folder of the user's OneDrive, or of a site's document library.
    pub async fn drive_root(&self, site_id: Option<&str>) -> Result<DriveFolder> {
        let url = site_id.map_or_else(urls::my_drive, urls::site_drive);
        let drive = self.get(&url, &Scope::graph(FILES_SCOPE)).await?;
        let drive_id = drive["id"].as_str().ok_or_else(|| Error::Decode("a drive without an id".into()))?;
        Ok(DriveFolder { drive_id: drive_id.to_owned(), item_id: ROOT_ITEM.to_owned() })
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

    #[test]
    fn shared_files_carry_the_sharer_and_date_newest_first() {
        let body = json!({"value": [
            {"name": "old.pdf", "webUrl": "https://x/o", "remoteItem": {"id": "o", "file": {}, "parentReference": {"driveId": "d"},
             "shared": {"sharedDateTime": "2026-10-01T10:00:00Z", "sharedBy": {"user": {"displayName": "Anna"}}}}},
            {"name": "Team", "webUrl": "https://x/t", "remoteItem": {"id": "t", "folder": {"childCount": 2}, "parentReference": {"driveId": "d"},
             "shared": {"sharedDateTime": "2026-11-01T10:00:00Z", "owner": {"user": {"displayName": "Ben"}}}}}
        ]});
        let shared = parse_shared(&body);
        assert_eq!(shared[0].name, "Team");
        assert!(shared[0].is_folder());
        assert_eq!(shared[0].modified_by.as_deref(), Some("Ben"));
        assert_eq!(shared[1].modified_by.as_deref(), Some("Anna"));
        assert_eq!(CloudFile::from(&shared[1]).item_id.as_deref(), Some("o"));
    }

    #[test]
    fn recent_owners_and_sites() {
        let recent = json!({"value": [
            {"name": "a", "remoteItem": {"id": "1", "createdBy": {"user": {"displayName": "Cara"}},
             "webUrl": "https://t.sharepoint.com/sites/dev/a", "parentReference": {"siteId": "dev"}}},
            {"name": "b", "remoteItem": {"id": "2", "webUrl": "https://t-my.sharepoint.com/personal/x/b", "parentReference": {"siteId": "me"}}},
            {"name": "c", "remoteItem": {"id": "3", "webUrl": "https://t.sharepoint.com/sites/dev/c", "parentReference": {"siteId": "dev"}}},
            {"name": "d", "remoteItem": {"id": "4", "webUrl": "https://t.sharepoint.com/sites/ops/d", "sharepointIds": {"siteId": "ops"}}}
        ]});
        assert_eq!(parse_recent_owners(&recent), vec![("1".to_owned(), "Cara".to_owned())]);
        assert_eq!(recent_site_ids(&recent, 8), ["dev", "ops"]);
        assert_eq!(recent_site_ids(&recent, 1), ["dev"]);
    }
}
