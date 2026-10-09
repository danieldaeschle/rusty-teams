use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chatsvc::{ChatSection, ChatSectionSettings, Pins, UserSettings, pins::SessionTransport};
use chrono::Utc;
use session::Session;
use store::{ChannelLayoutRecord, FolderRecord, Store, TeamLayoutRecord};

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::events::CoreEvent;
use crate::remote::Remote;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Favorites,
    UserCreated,
    Recent,
    Meeting,
    Muted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatFolder {
    pub id: String,
    pub name: String,
    pub kind: FolderKind,
    pub expanded: bool,
    pub conversation_ids: Vec<String>,
}

pub trait FolderSource: Send + Sync {
    fn folders(&self) -> BoxFuture<'_, Result<Vec<ChatFolder>>>;
    fn pinned_channels(&self) -> BoxFuture<'_, Result<Vec<String>>>;
    fn team_layout(&self) -> BoxFuture<'_, Result<Vec<TeamLayoutRecord>>>;
    fn section_settings(&self) -> BoxFuture<'_, Result<ChatSectionSettings>>;
    fn set_section_enabled(&self, section: ChatSection, enabled: bool)
    -> BoxFuture<'_, Result<()>>;
    fn set_folder_expanded<'a>(
        &'a self,
        folder_id: &'a str,
        expanded: bool,
    ) -> BoxFuture<'a, Result<()>>;
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
    settings: UserSettings,
}

impl ChatsvcFolderSource {
    pub fn new(session: &Session) -> Self {
        ChatsvcFolderSource {
            pins: Pins::new(session),
            settings: UserSettings::new(session),
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

    fn section_settings(&self) -> BoxFuture<'_, Result<ChatSectionSettings>> {
        Box::pin(async { Ok(self.settings.chat_sections().await?) })
    }

    fn set_section_enabled(
        &self,
        section: ChatSection,
        enabled: bool,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { Ok(self.settings.set_chat_section(section, enabled).await?) })
    }

    fn set_folder_expanded<'a>(
        &'a self,
        folder_id: &'a str,
        expanded: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(self.pins.set_folder_expanded(folder_id, expanded).await?) })
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
                chatsvc::FolderKind::Recent => FolderKind::Recent,
                chatsvc::FolderKind::Meeting => FolderKind::Meeting,
                chatsvc::FolderKind::Muted => FolderKind::Muted,
            },
            expanded: folder.expanded,
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
const KIND_RECENT: &str = "RecentChats";
const KIND_MEETING: &str = "MeetingChats";
const KIND_MUTED: &str = "MutedChats";
const META_MUTED_SECTION: &str = "chat_section_muted";
const META_MEETING_SECTION: &str = "chat_section_meeting";

fn to_record(folder: &ChatFolder) -> FolderRecord {
    FolderRecord {
        id: folder.id.clone(),
        name: folder.name.clone(),
        kind: match folder.kind {
            FolderKind::Favorites => KIND_FAVORITES,
            FolderKind::UserCreated => KIND_USER_CREATED,
            FolderKind::Recent => KIND_RECENT,
            FolderKind::Meeting => KIND_MEETING,
            FolderKind::Muted => KIND_MUTED,
        }
        .to_owned(),
        expanded: folder.expanded,
        conversation_ids: folder.conversation_ids.clone(),
    }
}

fn from_record(record: FolderRecord) -> ChatFolder {
    ChatFolder {
        id: record.id,
        name: record.name,
        kind: match record.kind.as_str() {
            KIND_FAVORITES => FolderKind::Favorites,
            KIND_RECENT => FolderKind::Recent,
            KIND_MEETING => FolderKind::Meeting,
            KIND_MUTED => FolderKind::Muted,
            _ => FolderKind::UserCreated,
        },
        expanded: record.expanded,
        conversation_ids: record.conversation_ids,
    }
}

pub fn stored_section_settings(store: &Store) -> ChatSectionSettings {
    let read = |key: &str| {
        store
            .meta(key)
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok())
    };
    ChatSectionSettings {
        muted: read(META_MUTED_SECTION),
        meeting: read(META_MEETING_SECTION),
    }
}

pub fn store_section_settings(store: &Store, settings: &ChatSectionSettings) -> Result<()> {
    for (key, value) in [
        (META_MUTED_SECTION, settings.muted),
        (META_MEETING_SECTION, settings.meeting),
    ] {
        let text = value.map(|enabled| enabled.to_string()).unwrap_or_default();
        store.set_meta(key, &text)?;
    }
    Ok(())
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
        self.refresh_section_settings(source).await;
        self.refresh_team_layout(source).await
    }

    async fn refresh_section_settings(&self, source: &Arc<dyn FolderSource>) {
        let outcome = match source.section_settings().await {
            Ok(settings) => self.apply_section_settings(&settings),
            Err(error) => Err(error),
        };
        if let Err(error) = outcome {
            self.report(format!("cannot refresh chat list settings: {error}"));
        }
    }

    fn apply_section_settings(&self, settings: &ChatSectionSettings) -> Result<()> {
        if stored_section_settings(&self.store) != *settings {
            store_section_settings(&self.store, settings)?;
            let _ = self.events.send(CoreEvent::FoldersChanged);
        }
        Ok(())
    }

    pub async fn set_section_enabled(&self, section: ChatSection, enabled: bool) -> Result<()> {
        self.folder_source()?
            .set_section_enabled(section, enabled)
            .await
    }

    pub async fn set_folder_expanded(&self, folder_id: &str, expanded: bool) -> Result<()> {
        self.folder_source()?
            .set_folder_expanded(folder_id, expanded)
            .await
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
