use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chatsvc::{Pins, pins::SessionTransport};
use session::Session;
use chrono::Utc;
use store::{ChannelLayoutRecord, FolderRecord, TeamLayoutRecord};

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::events::CoreEvent;
use crate::remote::Remote;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Favorites,
    UserCreated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatFolder {
    pub id: String,
    pub name: String,
    pub kind: FolderKind,
    pub conversation_ids: Vec<String>,
}

pub trait FolderSource: Send + Sync {
    fn folders(&self) -> BoxFuture<'_, Result<Vec<ChatFolder>>>;
    fn pinned_channels(&self) -> BoxFuture<'_, Result<Vec<String>>>;
    fn team_layout(&self) -> BoxFuture<'_, Result<Vec<TeamLayoutRecord>>>;
    fn move_to_folder<'a>(
        &'a self,
        conversation_id: &'a str,
        target_folder_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>>;
    fn remove_from_folder<'a>(
        &'a self,
        conversation_id: &'a str,
        folder_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>>;
}

pub struct ChatsvcFolderSource {
    pins: Pins<SessionTransport>,
}

impl ChatsvcFolderSource {
    pub fn new(session: &Session) -> Self {
        ChatsvcFolderSource {
            pins: Pins::new(session),
        }
    }
}

impl FolderSource for ChatsvcFolderSource {
    fn folders(&self) -> BoxFuture<'_, Result<Vec<ChatFolder>>> {
        Box::pin(async { Ok(convert(self.pins.folders().await?)) })
    }

    fn pinned_channels(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async { Ok(self.pins.pinned_channels().await?.channel_ids) })
    }

    fn team_layout(&self) -> BoxFuture<'_, Result<Vec<TeamLayoutRecord>>> {
        Box::pin(async {
            Ok(self
                .pins
                .team_layout()
                .await?
                .into_iter()
                .map(layout_record)
                .collect())
        })
    }

    fn move_to_folder<'a>(
        &'a self,
        conversation_id: &'a str,
        target_folder_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>> {
        Box::pin(async move {
            Ok(convert(
                self.pins
                    .move_to_folder(conversation_id, target_folder_id)
                    .await?,
            ))
        })
    }

    fn remove_from_folder<'a>(
        &'a self,
        conversation_id: &'a str,
        folder_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>> {
        Box::pin(async move {
            Ok(convert(
                self.pins
                    .remove_from_folder(conversation_id, folder_id)
                    .await?,
            ))
        })
    }
}

fn convert(folders: chatsvc::Folders) -> Vec<ChatFolder> {
    folders
        .folders
        .into_iter()
        .map(|folder| ChatFolder {
            id: folder.id,
            name: folder.name,
            kind: match folder.kind {
                chatsvc::FolderKind::Favorites => FolderKind::Favorites,
                chatsvc::FolderKind::UserCreated => FolderKind::UserCreated,
            },
            conversation_ids: folder.conversation_ids,
        })
        .collect()
}

fn layout_record(team: chatsvc::TeamLayout) -> TeamLayoutRecord {
    TeamLayoutRecord {
        team_id: team.team_id,
        hidden: team.hidden,
        channels: team
            .channels
            .into_iter()
            .map(|channel| ChannelLayoutRecord {
                channel_id: channel.channel_id,
                general: channel.general,
                hidden: channel.hidden,
            })
            .collect(),
    }
}

const META_TEAM_LAYOUT_AT: &str = "team_layout_refreshed_at";
const KIND_FAVORITES: &str = "Favorites";
const KIND_USER_CREATED: &str = "UserCreated";

fn to_record(folder: &ChatFolder) -> FolderRecord {
    FolderRecord {
        id: folder.id.clone(),
        name: folder.name.clone(),
        kind: match folder.kind {
            FolderKind::Favorites => KIND_FAVORITES,
            FolderKind::UserCreated => KIND_USER_CREATED,
        }
        .to_owned(),
        conversation_ids: folder.conversation_ids.clone(),
    }
}

fn from_record(record: FolderRecord) -> ChatFolder {
    ChatFolder {
        id: record.id,
        name: record.name,
        kind: if record.kind == KIND_FAVORITES {
            FolderKind::Favorites
        } else {
            FolderKind::UserCreated
        },
        conversation_ids: record.conversation_ids,
    }
}

impl<R: Remote> SyncEngine<R> {
    pub fn with_folder_source(mut self, source: Arc<dyn FolderSource>) -> Self {
        self.folder_source = Some(source);
        self
    }

    pub fn chat_folders(&self) -> Result<Vec<ChatFolder>> {
        Ok(self.store.folders()?.into_iter().map(from_record).collect())
    }

    pub fn pinned_channels(&self) -> Result<Vec<String>> {
        Ok(self.store.pinned_channel_ids()?)
    }

    pub async fn refresh_folders(&self) -> Result<()> {
        let source = self.folder_source()?;
        let (folders, channels) = tokio::try_join!(source.folders(), source.pinned_channels())?;
        self.store_folders(&folders, &channels)?;
        self.refresh_team_layout(source).await
    }

    async fn refresh_team_layout(&self, source: &Arc<dyn FolderSource>) -> Result<()> {
        let last = self.store.meta_time(META_TEAM_LAYOUT_AT)?;
        if last.is_some_and(|last| Utc::now() - last < self.config.team_layout_refresh_interval) {
            return Ok(());
        }
        let layout = source.team_layout().await?;
        let before = self.store.sidebar()?.teams;
        self.store.replace_team_layout(&layout)?;
        self.store.set_meta_time(META_TEAM_LAYOUT_AT, Utc::now())?;
        if self.store.sidebar()?.teams != before {
            let _ = self.events.send(CoreEvent::SidebarChanged);
        }
        Ok(())
    }

    pub async fn move_to_folder(
        &self,
        conversation_id: &str,
        target_folder_id: &str,
    ) -> Result<()> {
        let folders = self
            .folder_source()?
            .move_to_folder(conversation_id, target_folder_id)
            .await?;
        self.store_folders(&folders, &self.store.pinned_channel_ids()?)
    }

    pub async fn remove_from_folder(&self, conversation_id: &str, folder_id: &str) -> Result<()> {
        let folders = self
            .folder_source()?
            .remove_from_folder(conversation_id, folder_id)
            .await?;
        self.store_folders(&folders, &self.store.pinned_channel_ids()?)
    }

    fn folder_source(&self) -> Result<&Arc<dyn FolderSource>> {
        self.folder_source
            .as_ref()
            .ok_or(Error::Unsupported("chat folders without a folder source"))
    }

    fn store_folders(&self, folders: &[ChatFolder], channel_ids: &[String]) -> Result<()> {
        let previous = (self.chat_folders()?, self.store.pinned_channel_ids()?);
        let records: Vec<FolderRecord> = folders.iter().map(to_record).collect();
        self.store.replace_folders(&records, channel_ids)?;
        if previous != (folders.to_vec(), channel_ids.to_vec()) {
            let _ = self.events.send(CoreEvent::FoldersChanged);
        }
        Ok(())
    }
}
