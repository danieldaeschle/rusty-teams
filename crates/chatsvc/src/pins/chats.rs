use serde_json::{Value, json};
use session::{ApiResponse, Method, Request};

use super::Pins;
use super::state::{PinnedChats, error_code, parse_pinned_chats};
use super::transport::CsaTransport;
use crate::error::{Error, Result};

pub(super) const FOLDER_QUERY: &str =
    "supportsAdditionalSystemGeneratedFolders=true&supportsSliceItems=true";
pub(super) const NO_OP_CODES: [&str; 2] = ["ConversationAlreadyLinkedWithFolder", "ItemNotFound"];

#[derive(Debug, Clone, PartialEq)]
pub(super) enum FolderAction {
    Pin {
        chat_id: String,
        index: Option<usize>,
    },
    Unpin {
        chat_id: String,
    },
    Reorder {
        chat_ids: Vec<String>,
    },
}

impl FolderAction {
    fn to_json(&self, folder_id: &str) -> Value {
        match self {
            FolderAction::Pin { chat_id, index } => {
                let mut action =
                    json!({"action": "AddItem", "folderId": folder_id, "itemId": chat_id});
                if let Some(index) = index {
                    action["itemIndex"] = json!(index);
                }
                action
            }
            FolderAction::Unpin { chat_id } => {
                json!({"action": "RemoveItem", "folderId": folder_id, "itemId": chat_id})
            }
            FolderAction::Reorder { chat_ids } => {
                json!({"action": "ReorderItems", "folderId": folder_id, "newItemsOrder": chat_ids})
            }
        }
    }
}

pub(super) fn write_body(action: &FolderAction, state: &PinnedChats) -> Value {
    json!({
        "folderHierarchyVersion": state.hierarchy_version,
        "supportsAdditionalSystemGeneratedFolders": true,
        "supportsSliceItems": true,
        "actions": [action.to_json(&state.folder_id)],
    })
}

impl<T: CsaTransport> Pins<T> {
    pub async fn pinned_chats(&self) -> Result<PinnedChats> {
        let url = format!("{}/conversationFolders?{FOLDER_QUERY}", self.base_url);
        let answer = self.transport.send(Request::get(url)).await?;
        ensure_success(&answer)?;
        parse_pinned_chats(&answer.body)
    }

    pub async fn pin_chat(&self, chat_id: &str, index: Option<usize>) -> Result<PinnedChats> {
        let action = FolderAction::Pin {
            chat_id: chat_id.to_owned(),
            index,
        };
        self.write_folder(action).await
    }

    pub async fn unpin_chat(&self, chat_id: &str) -> Result<PinnedChats> {
        let action = FolderAction::Unpin {
            chat_id: chat_id.to_owned(),
        };
        self.write_folder(action).await
    }

    pub async fn reorder_chats(&self, chat_ids: &[String]) -> Result<PinnedChats> {
        let action = FolderAction::Reorder {
            chat_ids: chat_ids.to_vec(),
        };
        self.write_folder(action).await
    }

    async fn write_folder(&self, action: FolderAction) -> Result<PinnedChats> {
        let mut state = self.pinned_chats().await?;
        for attempt in 0..2 {
            let url = format!("{}/conversationFolders?{FOLDER_QUERY}", self.base_url);
            let request = Request::with_body(Method::Post, url, write_body(&action, &state));
            let answer = self.transport.send(request).await?;
            match answer.status {
                200..=299 => return parse_pinned_chats(&answer.body),
                412 if attempt == 0 => {
                    state = match parse_pinned_chats(&answer.body) {
                        Ok(current) => current,
                        Err(_) => self.pinned_chats().await?,
                    };
                }
                412 => return Err(Error::VersionConflict),
                400 if error_code(&answer.body).is_some_and(|code| NO_OP_CODES.contains(&code)) => {
                    return self.pinned_chats().await;
                }
                _ => return Err(api_error(&answer)),
            }
        }
        Err(Error::VersionConflict)
    }
}

pub(super) fn ensure_success(answer: &ApiResponse) -> Result<()> {
    if answer.is_success() {
        Ok(())
    } else {
        Err(api_error(answer))
    }
}

pub(super) fn api_error(answer: &ApiResponse) -> Error {
    Error::Session(session::Error::api(
        answer.status,
        "csa",
        answer.body.clone(),
    ))
}
