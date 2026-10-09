use serde_json::{Value, json};
use session::{Method, Request};

use super::Pins;
use super::chats::{FOLDER_QUERY, NO_OP_CODES, api_error, ensure_success};
use super::state::error_code;
use super::transport::CsaTransport;
use crate::error::{Error, Result};
use crate::messages::encode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Favorites,
    UserCreated,
    Recent,
    Meeting,
    Muted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub kind: FolderKind,
    pub expanded: bool,
    pub conversation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    pub hierarchy_version: i64,
    pub folders: Vec<Folder>,
}

impl Folders {
    fn containing(&self, conversation_id: &str) -> Option<&Folder> {
        self.folders.iter().find(|folder| {
            folder.kind == FolderKind::UserCreated
                && folder
                    .conversation_ids
                    .iter()
                    .any(|id| id == conversation_id)
        })
    }
}

pub fn parse_folders(body: &Value) -> Result<Folders> {
    let hierarchy_version = body
        .get("folderHierarchyVersion")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::UnexpectedAnswer("folderHierarchyVersion missing".into()))?;
    let raw = body
        .get("conversationFolders")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("conversationFolders missing".into()))?;
    let mut folders: Vec<Folder> = raw.iter().filter_map(parse_folder).collect();
    if let Some(order) = body
        .get("conversationFolderOrder")
        .and_then(Value::as_array)
    {
        let rank = |folder: &Folder| {
            order
                .iter()
                .position(|id| id.as_str() == Some(folder.id.as_str()))
                .unwrap_or(usize::MAX)
        };
        folders.sort_by_key(rank);
    }
    Ok(Folders {
        hierarchy_version,
        folders,
    })
}

fn parse_folder(folder: &Value) -> Option<Folder> {
    if folder.get("isDeleted").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let kind = match folder.get("folderType").and_then(Value::as_str)? {
        "Favorites" => FolderKind::Favorites,
        "UserCreated" => FolderKind::UserCreated,
        "RecentChats" => FolderKind::Recent,
        "MeetingChats" => FolderKind::Meeting,
        "MutedChats" => FolderKind::Muted,
        _ => return None,
    };
    Some(Folder {
        id: folder.get("id")?.as_str()?.to_owned(),
        name: folder
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        kind,
        expanded: folder
            .get("isExpanded")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        conversation_ids: folder
            .get("conversationFolderItems")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("conversationId").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn expanded_body(hierarchy_version: i64, expanded: bool) -> Value {
    json!({
        "folderHierarchyVersion": hierarchy_version,
        "supportsAdditionalSystemGeneratedFolders": true,
        "supportsSliceItems": true,
        "isExpanded": expanded,
    })
}

fn add_item(folder_id: &str, conversation_id: &str) -> Value {
    json!({"action": "AddItem", "folderId": folder_id, "itemId": conversation_id})
}

fn remove_item(folder_id: &str, conversation_id: &str) -> Value {
    json!({"action": "RemoveItem", "folderId": folder_id, "itemId": conversation_id})
}

impl<T: CsaTransport> Pins<T> {
    pub async fn folders(&self) -> Result<Folders> {
        let url = format!("{}/conversationFolders?{FOLDER_QUERY}", self.base_url);
        let answer = self.transport.send(Request::get(url)).await?;
        ensure_success(&answer)?;
        parse_folders(&answer.body)
    }

    pub async fn move_to_folder(
        &self,
        conversation_id: &str,
        target_folder_id: &str,
    ) -> Result<Folders> {
        self.write_folders(|state| {
            let target = state
                .folders
                .iter()
                .find(|folder| folder.id == target_folder_id)
                .ok_or_else(|| Error::UnknownFolder(target_folder_id.to_owned()))?;
            if target
                .conversation_ids
                .iter()
                .any(|id| id == conversation_id)
            {
                return Ok(Vec::new());
            }
            let mut actions = Vec::new();
            if let Some(current) = state.containing(conversation_id) {
                actions.push(remove_item(&current.id, conversation_id));
            }
            actions.push(add_item(target_folder_id, conversation_id));
            Ok(actions)
        })
        .await
    }

    pub async fn remove_from_folder(
        &self,
        conversation_id: &str,
        folder_id: &str,
    ) -> Result<Folders> {
        self.write_folders(|state| {
            let folder = state
                .folders
                .iter()
                .find(|folder| folder.id == folder_id)
                .ok_or_else(|| Error::UnknownFolder(folder_id.to_owned()))?;
            if !folder
                .conversation_ids
                .iter()
                .any(|id| id == conversation_id)
            {
                return Ok(Vec::new());
            }
            Ok(vec![remove_item(folder_id, conversation_id)])
        })
        .await
    }

    pub async fn set_folder_expanded(&self, folder_id: &str, expanded: bool) -> Result<()> {
        let mut hierarchy_version = self.folders().await?.hierarchy_version;
        for attempt in 0..2 {
            let url = format!(
                "{}/conversationFolders/{}?{FOLDER_QUERY}",
                self.base_url,
                encode(folder_id)
            );
            let body = expanded_body(hierarchy_version, expanded);
            let answer = self
                .transport
                .send(Request::with_body(Method::Put, url, body))
                .await?;
            match answer.status {
                200..=299 => return Ok(()),
                412 if attempt == 0 => {
                    hierarchy_version = match parse_folders(&answer.body) {
                        Ok(current) => current.hierarchy_version,
                        Err(_) => self.folders().await?.hierarchy_version,
                    };
                }
                412 => return Err(Error::VersionConflict),
                _ => return Err(api_error(&answer)),
            }
        }
        Err(Error::VersionConflict)
    }

    async fn write_folders(
        &self,
        plan: impl Fn(&Folders) -> Result<Vec<Value>>,
    ) -> Result<Folders> {
        let mut state = self.folders().await?;
        for attempt in 0..2 {
            let actions = plan(&state)?;
            if actions.is_empty() {
                return Ok(state);
            }
            let url = format!("{}/conversationFolders?{FOLDER_QUERY}", self.base_url);
            let body = json!({
                "folderHierarchyVersion": state.hierarchy_version,
                "supportsAdditionalSystemGeneratedFolders": true,
                "supportsSliceItems": true,
                "actions": actions,
            });
            let answer = self
                .transport
                .send(Request::with_body(Method::Post, url, body))
                .await?;
            match answer.status {
                200..=299 => return parse_folders(&answer.body),
                412 if attempt == 0 => {
                    state = match parse_folders(&answer.body) {
                        Ok(current) => current,
                        Err(_) => self.folders().await?,
                    };
                }
                412 => return Err(Error::VersionConflict),
                400 if error_code(&answer.body).is_some_and(|code| NO_OP_CODES.contains(&code)) => {
                    return self.folders().await;
                }
                _ => return Err(api_error(&answer)),
            }
        }
        Err(Error::VersionConflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_folders_in_server_order() {
        let body = json!({
            "folderHierarchyVersion": 5,
            "conversationFolderOrder": ["b", "a", "r"],
            "conversationFolders": [
                {"id": "a", "name": "A", "folderType": "UserCreated", "conversationFolderItems": [{"conversationId": "c1"}, {"conversationId": "c2"}]},
                {"id": "r", "folderType": "RecentChats", "conversationFolderItems": []},
                {"id": "b", "name": "Pins", "folderType": "Favorites", "conversationFolderItems": []},
                {"id": "x", "folderType": "UserCreated", "isDeleted": true}
            ]
        });
        let parsed = parse_folders(&body).unwrap();
        let ids: Vec<&str> = parsed
            .folders
            .iter()
            .map(|folder| folder.id.as_str())
            .collect();
        assert_eq!(ids, ["b", "a", "r"]);
        assert_eq!(parsed.folders[1].conversation_ids, ["c1", "c2"]);
        assert_eq!(parsed.folders[0].kind, FolderKind::Favorites);
    }

    #[test]
    fn keeps_system_folders_with_id_order_and_expanded_state() {
        let body = json!({
            "folderHierarchyVersion": 5,
            "conversationFolderOrder": ["m", "f", "r", "u"],
            "conversationFolders": [
                {"id": "r", "folderType": "RecentChats", "isExpanded": true, "conversationFolderItems": []},
                {"id": "u", "folderType": "MutedChats", "isExpanded": false, "conversationFolderItems": []},
                {"id": "m", "folderType": "MeetingChats", "conversationFolderItems": []},
                {"id": "f", "name": "Pins", "folderType": "Favorites", "conversationFolderItems": []},
                {"id": "q", "folderType": "QuickViews", "conversationFolderItems": []}
            ]
        });
        let parsed = parse_folders(&body).unwrap();
        let summary: Vec<(&str, FolderKind, bool)> = parsed
            .folders
            .iter()
            .map(|folder| (folder.id.as_str(), folder.kind, folder.expanded))
            .collect();
        assert_eq!(
            summary,
            [
                ("m", FolderKind::Meeting, true),
                ("f", FolderKind::Favorites, true),
                ("r", FolderKind::Recent, true),
                ("u", FolderKind::Muted, false),
            ]
        );
    }

    #[test]
    fn expanded_body_carries_only_the_flag_and_the_lock() {
        assert_eq!(
            expanded_body(7, false),
            json!({
                "folderHierarchyVersion": 7,
                "supportsAdditionalSystemGeneratedFolders": true,
                "supportsSliceItems": true,
                "isExpanded": false,
            })
        );
    }
}
