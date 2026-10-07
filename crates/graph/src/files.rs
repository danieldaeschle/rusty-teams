use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};
use session::{Method, Request, Scope, Session};

use crate::client::Graph;
use crate::error::{Error, Result};
use crate::outgoing::FileReference;
use crate::urls;

const FILES_SCOPE: &str = "Files.ReadWrite";
const CHUNK_UNIT_BYTES: u64 = 327_680;
pub const UPLOAD_CHUNK_BYTES: u64 = 4 * CHUNK_UNIT_BYTES;
const MAX_CHUNK_RETRIES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFolder {
    pub drive_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadDestination {
    ChatFiles,
    Folder(DriveFolder),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedFile {
    pub drive_id: String,
    pub item_id: String,
    pub name: String,
    pub web_url: String,
    pub web_dav_url: Option<String>,
    pub etag: String,
}

impl UploadedFile {
    pub fn reference(&self) -> Option<FileReference> {
        Some(FileReference {
            attachment_id: etag_guid(&self.etag)?,
            content_url: self
                .web_dav_url
                .clone()
                .unwrap_or_else(|| self.web_url.clone()),
            name: self.name.clone(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveItem {
    id: String,
    name: String,
    #[serde(default)]
    web_url: String,
    web_dav_url: Option<String>,
    #[serde(default)]
    e_tag: String,
    parent_reference: Option<ParentReference>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParentReference {
    drive_id: Option<String>,
}

impl DriveItem {
    fn drive_id(&self) -> Option<String> {
        self.parent_reference.as_ref()?.drive_id.clone()
    }
}

/// `"{GUID},3"` becomes `GUID`.
pub fn etag_guid(etag: &str) -> Option<String> {
    let start = etag.find('{')? + 1;
    let length = etag[start..].find('}')?;
    let guid = &etag[start..start + length];
    (!guid.is_empty()).then(|| guid.to_owned())
}

/// Inclusive range of the chunk that starts at `start`; a multiple of 320 KiB long except at the end.
pub fn chunk_range_at(start: u64, total: u64) -> (u64, u64) {
    (start, (start + UPLOAD_CHUNK_BYTES).min(total) - 1)
}

pub fn chunk_ranges(total: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < total {
        let range = chunk_range_at(start, total);
        start = range.1 + 1;
        ranges.push(range);
    }
    ranges
}

/// First byte the server still wants, from `nextExpectedRanges` (`["1310720-"]`).
pub fn next_expected_start(body: &Value) -> Option<u64> {
    let range = body
        .get("nextExpectedRanges")?
        .as_array()?
        .first()?
        .as_str()?;
    range.split('-').next()?.parse().ok()
}

struct SessionGuard {
    session: Session,
    url: String,
    armed: bool,
}

impl SessionGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let session = self.session.clone();
        let request = Request {
            method: Method::Delete,
            anonymous: true,
            ..Request::get(std::mem::take(&mut self.url))
        };
        runtime.spawn(async move {
            let _ = session.send(request, &Scope::graph(FILES_SCOPE)).await;
        });
    }
}

pub fn content_range(start: u64, end: u64, total: u64) -> String {
    format!("bytes {start}-{end}/{total}")
}

pub fn percent_done(sent: u64, total: u64) -> u8 {
    if total == 0 {
        return 100;
    }
    (sent.min(total) * 100 / total) as u8
}

impl Graph {
    pub async fn channel_files_folder(
        &self,
        team_id: &str,
        channel_id: &str,
    ) -> Result<DriveFolder> {
        let answer = self
            .session()
            .request(
                Method::Get,
                &urls::channel_files_folder(team_id, channel_id),
                &Scope::graph(FILES_SCOPE),
                None,
            )
            .await?;
        let item: DriveItem = serde_json::from_value(answer.body)?;
        let drive_id = item
            .drive_id()
            .ok_or_else(|| Error::Upload("the channel folder has no drive".into()))?;
        Ok(DriveFolder {
            drive_id,
            item_id: item.id,
        })
    }

    pub async fn upload_file(
        &self,
        destination: &UploadDestination,
        file_name: &str,
        bytes: &[u8],
        progress: &(dyn Fn(u8) + Send + Sync),
    ) -> Result<UploadedFile> {
        if bytes.is_empty() {
            return Err(Error::EmptyUpload);
        }
        let scope = Scope::graph(FILES_SCOPE);
        let session_url = match destination {
            UploadDestination::ChatFiles => urls::chat_files_upload_session(file_name),
            UploadDestination::Folder(folder) => {
                urls::folder_upload_session(&folder.drive_id, &folder.item_id, file_name)
            }
        };
        let created = self
            .session()
            .request(
                Method::Post,
                &session_url,
                &scope,
                Some(json!({"item": {"@microsoft.graph.conflictBehavior": "rename"}})),
            )
            .await?;
        let upload_url = created
            .body
            .get("uploadUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Upload("no upload URL in the answer".into()))?
            .to_owned();
        let mut guard = SessionGuard {
            session: self.session().clone(),
            url: upload_url.clone(),
            armed: true,
        };
        let total = bytes.len() as u64;
        progress(0);
        let mut offset = 0;
        let mut failures = 0;
        while offset < total {
            let (start, end) = chunk_range_at(offset, total);
            let request = Request::anonymous_bytes(
                Method::Put,
                upload_url.clone(),
                vec![("Content-Range".to_owned(), content_range(start, end, total))],
                STANDARD.encode(&bytes[start as usize..=end as usize]),
            );
            match self.session().send(request, &scope).await {
                Ok(answer) if matches!(answer.status, 200 | 201) => {
                    guard.disarm();
                    progress(100);
                    return finished_item(answer.body);
                }
                Ok(_) => {
                    failures = 0;
                    offset = end + 1;
                    progress(percent_done(offset, total).min(99));
                }
                Err(error) => {
                    failures += 1;
                    if failures > MAX_CHUNK_RETRIES {
                        return Err(error.into());
                    }
                    if let Some(next) = self.resume_offset(&upload_url, &scope).await {
                        offset = next.min(total);
                    }
                }
            }
        }
        Err(Error::Upload(
            "the last chunk did not finish the upload".into(),
        ))
    }

    async fn resume_offset(&self, upload_url: &str, scope: &Scope) -> Option<u64> {
        let request = Request {
            anonymous: true,
            ..Request::get(upload_url)
        };
        let answer = self.session().send(request, scope).await.ok()?;
        next_expected_start(&answer.body)
    }

    pub async fn delete_drive_item(&self, drive_id: &str, item_id: &str) -> Result<()> {
        self.session()
            .request(
                Method::Delete,
                &urls::drive_item(drive_id, item_id),
                &Scope::graph(FILES_SCOPE),
                None,
            )
            .await?;
        Ok(())
    }

    pub async fn share_item(&self, file: &UploadedFile, user_ids: &[String]) -> Result<()> {
        if user_ids.is_empty() {
            return Ok(());
        }
        let recipients: Vec<Value> = user_ids
            .iter()
            .map(|user_id| json!({"objectId": user_id}))
            .collect();
        self.session()
            .request(
                Method::Post,
                &urls::drive_item_invite(&file.drive_id, &file.item_id),
                &Scope::graph(FILES_SCOPE),
                Some(json!({
                    "recipients": recipients,
                    "roles": ["read"],
                    "requireSignIn": true,
                    "sendInvitation": false,
                })),
            )
            .await?;
        Ok(())
    }
}

fn finished_item(body: Value) -> Result<UploadedFile> {
    let item: DriveItem = serde_json::from_value(body)?;
    let drive_id = item
        .drive_id()
        .ok_or_else(|| Error::Upload("the uploaded file has no drive".into()))?;
    Ok(UploadedFile {
        drive_id,
        item_id: item.id,
        name: item.name,
        web_url: item.web_url,
        web_dav_url: item.web_dav_url,
        etag: item.e_tag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_guid_is_the_part_between_braces() {
        assert_eq!(
            etag_guid("\"{4F2A9C1E-0B7D-4C3A-9E55-1A2B3C4D5E6F},3\""),
            Some("4F2A9C1E-0B7D-4C3A-9E55-1A2B3C4D5E6F".to_owned())
        );
        assert_eq!(etag_guid("\"abc,3\""), None);
        assert_eq!(etag_guid("{}"), None);
        assert_eq!(etag_guid(""), None);
    }

    #[test]
    fn small_files_are_one_chunk() {
        assert_eq!(chunk_ranges(1), vec![(0, 0)]);
        assert_eq!(
            chunk_ranges(UPLOAD_CHUNK_BYTES),
            vec![(0, UPLOAD_CHUNK_BYTES - 1)]
        );
        assert!(chunk_ranges(0).is_empty());
    }

    #[test]
    fn chunks_are_multiples_of_320_kib_except_the_last() {
        let total = 2 * UPLOAD_CHUNK_BYTES + 12_345;
        let ranges = chunk_ranges(total);
        assert_eq!(ranges.len(), 3);
        for (start, end) in &ranges[..2] {
            assert_eq!((end + 1 - start) % CHUNK_UNIT_BYTES, 0);
            assert_eq!(end + 1 - start, UPLOAD_CHUNK_BYTES);
        }
        let (last_start, last_end) = ranges[2];
        assert_eq!(last_end + 1 - last_start, 12_345);
        assert_eq!(last_end + 1, total);
    }

    #[test]
    fn chunks_are_contiguous_and_cover_the_file() {
        let total = 3 * UPLOAD_CHUNK_BYTES - 7;
        let ranges = chunk_ranges(total);
        assert_eq!(ranges[0].0, 0);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].1 + 1, pair[1].0);
        }
        assert_eq!(ranges.last().unwrap().1 + 1, total);
    }

    #[test]
    fn resume_offset_comes_from_the_next_expected_range() {
        assert_eq!(
            next_expected_start(&json!({"nextExpectedRanges": ["1310720-"]})),
            Some(1_310_720)
        );
        assert_eq!(
            next_expected_start(&json!({"nextExpectedRanges": ["26-100", "200-"]})),
            Some(26)
        );
        assert_eq!(
            next_expected_start(&json!({"nextExpectedRanges": []})),
            None
        );
        assert_eq!(next_expected_start(&json!({})), None);
    }

    #[test]
    fn chunk_size_stays_well_under_the_evaluate_timeout() {
        assert_eq!(UPLOAD_CHUNK_BYTES, 1_310_720);
        assert_eq!(chunk_range_at(1_310_720, 2_000_000), (1_310_720, 1_999_999));
    }

    #[test]
    fn content_range_header_matches_graph() {
        assert_eq!(
            content_range(0, 5_242_879, 6_000_000),
            "bytes 0-5242879/6000000"
        );
    }

    #[test]
    fn progress_is_a_percentage() {
        assert_eq!(percent_done(0, 200), 0);
        assert_eq!(percent_done(50, 200), 25);
        assert_eq!(percent_done(200, 200), 100);
        assert_eq!(percent_done(5, 0), 100);
    }

    #[test]
    fn uploaded_file_reference_prefers_the_webdav_url() {
        let mut file = UploadedFile {
            drive_id: "d".into(),
            item_id: "i".into(),
            name: "a.pdf".into(),
            web_url: "https://x/web".into(),
            web_dav_url: Some("https://x/dav".into()),
            etag: "\"{AAAA},1\"".into(),
        };
        let reference = file.reference().unwrap();
        assert_eq!(reference.attachment_id, "AAAA");
        assert_eq!(reference.content_url, "https://x/dav");
        file.web_dav_url = None;
        assert_eq!(file.reference().unwrap().content_url, "https://x/web");
        file.etag = "nope".into();
        assert!(file.reference().is_none());
    }

    #[test]
    fn drive_item_answer_parses() {
        let item = finished_item(json!({
            "id": "01F", "name": "a.pdf", "webUrl": "https://x/web",
            "webDavUrl": "https://x/dav", "eTag": "\"{AAAA},2\"",
            "parentReference": {"driveId": "b!d"}
        }))
        .unwrap();
        assert_eq!(item.drive_id, "b!d");
        assert_eq!(item.item_id, "01F");
        assert_eq!(item.web_dav_url.as_deref(), Some("https://x/dav"));
    }
}
