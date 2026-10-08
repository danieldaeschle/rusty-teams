use chatsvc::{ChatApp, InvokeRequest, InvokeResponse, TaskContent, TaskContinue, TaskResponse};
use serde_json::Value;

use crate::adaptive_card::{CardAction, task_value};
use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::receipts::locked;
use crate::remote::Remote;

const TASK_SUBMIT_NAME: &str = "task/submit";
const DEFAULT_DIALOG_WIDTH: u32 = 520;
const DEFAULT_DIALOG_HEIGHT: u32 = 400;
const MAX_FAILURE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub enum CardActionOutcome {
    Sent,
    ReplaceCard(String),
    Dialog(TaskDialog),
    Message(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskDialog {
    pub title: Option<String>,
    pub width: u32,
    pub height: u32,
    pub kind: TaskDialogKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskDialogKind {
    Url { url: String, fallback_url: String },
    Card(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogIdentity {
    pub user_id: String,
    pub display_name: String,
    pub tenant_id: String,
}

impl<R: Remote> SyncEngine<R> {
    /// Cached per conversation after the first successful lookup.
    pub async fn chat_apps(&self, conversation_id: &str) -> Result<Vec<ChatApp>> {
        if let Some(cached) = locked(&self.chat_apps).get(conversation_id) {
            return Ok(cached.clone());
        }
        let apps = self.remote.chat_apps(conversation_id).await?;
        locked(&self.chat_apps).insert(conversation_id.to_owned(), apps.clone());
        Ok(apps)
    }

    pub fn cached_chat_apps(&self, conversation_id: &str) -> Option<Vec<ChatApp>> {
        locked(&self.chat_apps).get(conversation_id).cloned()
    }

    pub async fn dialog_identity(&self, conversation_id: &str) -> Result<DialogIdentity> {
        let user_id = self.my_user_id().await?;
        self.ensure_display_name().await?;
        let tenant_id = self.my_tenant_id(conversation_id).await?;
        Ok(DialogIdentity {
            user_id,
            display_name: self.me().map(|me| me.display_name).unwrap_or_default(),
            tenant_id,
        })
    }

    pub async fn card_action(
        &self,
        conversation_id: &str,
        message_id: &str,
        action: &CardAction,
    ) -> Result<CardActionOutcome> {
        let payload = action
            .invoke_payload()
            .ok_or(Error::Unsupported("this card action"))?;
        self.invoke_card(conversation_id, message_id, payload.name, payload.value)
            .await
    }

    pub async fn task_submit(
        &self,
        conversation_id: &str,
        message_id: &str,
        data: Value,
    ) -> Result<CardActionOutcome> {
        self.invoke_card(
            conversation_id,
            message_id,
            TASK_SUBMIT_NAME,
            task_value(data),
        )
        .await
    }

    async fn invoke_card(
        &self,
        conversation_id: &str,
        message_id: &str,
        name: &str,
        value: Value,
    ) -> Result<CardActionOutcome> {
        let (app, bot_id) = self.card_app(conversation_id, message_id).await?;
        self.ensure_display_name().await?;
        let display_name = self.me().map(|me| me.display_name).unwrap_or_default();
        let response = self
            .remote
            .invoke_card(InvokeRequest {
                bot_id,
                app_id: app.app_id,
                name: name.to_owned(),
                value,
                display_name,
                server_message_id: message_id.to_owned(),
                client_message_id: None,
                conversation_id: conversation_id.to_owned(),
            })
            .await?;
        Ok(outcome_of(response))
    }

    pub async fn card_app(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<(ChatApp, String)> {
        let sender_application_id = self
            .store
            .messages_by_id(conversation_id, &[message_id.to_owned()])?
            .remove(message_id)
            .and_then(|record| record.sender_application_id);
        let apps = self.chat_apps(conversation_id).await?;
        let matched = sender_application_id.as_deref().and_then(|sender| {
            apps.iter()
                .find(|app| app.has_bot(sender) || app.app_id == sender)
        });
        let mut with_bots = apps.iter().filter(|app| !app.bot_ids.is_empty());
        let app = match (matched, with_bots.next(), with_bots.next()) {
            (Some(app), _, _) | (None, Some(app), None) => app,
            _ => return Err(Error::Unsupported("a card from an unknown app")),
        };
        let bot_id = sender_application_id
            .filter(|sender| app.has_bot(sender))
            .or_else(|| app.bot_ids.first().cloned())
            .ok_or(Error::Unsupported("a card from an app without a bot"))?;
        Ok((app.clone(), bot_id))
    }
}

fn outcome_of(response: InvokeResponse) -> CardActionOutcome {
    match response {
        InvokeResponse::Empty => CardActionOutcome::Sent,
        InvokeResponse::Card(card) => CardActionOutcome::ReplaceCard(card.to_string()),
        InvokeResponse::Message(text) | InvokeResponse::Task(TaskResponse::Message(text)) => {
            CardActionOutcome::Message(text)
        }
        InvokeResponse::Task(TaskResponse::Continue(task)) => {
            CardActionOutcome::Dialog(dialog_of(task))
        }
        InvokeResponse::Failed {
            status_code,
            message,
        } => {
            let detail: String = message.chars().take(MAX_FAILURE_CHARS).collect();
            CardActionOutcome::Failed(format!("The app answered {status_code}: {detail}"))
        }
    }
}

fn dialog_of(task: TaskContinue) -> TaskDialog {
    TaskDialog {
        title: task.title,
        width: task.width.unwrap_or(DEFAULT_DIALOG_WIDTH),
        height: task.height.unwrap_or(DEFAULT_DIALOG_HEIGHT),
        kind: match task.content {
            TaskContent::Url { url, fallback_url } => TaskDialogKind::Url {
                fallback_url: fallback_url.unwrap_or_else(|| url.clone()),
                url,
            },
            TaskContent::Card(card) => TaskDialogKind::Card(card.to_string()),
        },
    }
}
