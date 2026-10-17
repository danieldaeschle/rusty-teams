use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use chatsvc::settings::{
    parse_chat_sections, parse_default_reactions, parse_skin_tone, parse_teams_order,
};
use chatsvc::{
    ChannelNotificationLevel, ChannelNotifications, ChannelSettings, ChannelTarget,
    ChannelVisibility, ChatSection, ChatSectionSettings, Pins, UserSettings,
    pins::SessionTransport,
};
use chrono::Utc;
use serde_json::Value;
use session::Session;
use store::{ChannelLayoutRecord, ChannelTabRecord, FolderRecord, Store, TeamLayoutRecord};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderChange {
    Create {
        name: String,
    },
    Rename {
        folder_id: String,
        name: String,
    },
    Delete {
        folder_id: String,
    },
    Move {
        folder_id: String,
        target_folder_id: String,
    },
}

pub trait FolderSource: Send + Sync {
    fn folders(&self) -> BoxFuture<'_, Result<Vec<ChatFolder>>>;
    fn pinned_channels(&self) -> BoxFuture<'_, Result<Vec<String>>>;
    fn team_layout(&self) -> BoxFuture<'_, Result<Vec<TeamLayoutRecord>>>;
    fn section_settings(&self) -> BoxFuture<'_, Result<ChatSectionSettings>>;
    fn teams_order(&self) -> BoxFuture<'_, Result<Vec<(String, i64)>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn preferred_skin_tone(&self) -> Option<u8> {
        None
    }
    fn set_skin_tone(&self, _tone: u8) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn default_reactions(&self) -> Option<Vec<String>> {
        None
    }
    fn set_default_reactions<'a>(
        &'a self,
        _reaction_ids: &'a [String],
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
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
        index: Option<usize>,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>>;
    fn remove_from_folder<'a>(
        &'a self,
        conversation_id: &'a str,
        folder_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>>;
    fn change_folders(&self, change: FolderChange) -> BoxFuture<'_, Result<Vec<ChatFolder>>>;
    fn set_channel_pinned<'a>(
        &'a self,
        _channel_id: &'a str,
        _pinned: bool,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async { Err(Error::Unsupported("channel pins without chatsvc")) })
    }
    fn set_channel_shown<'a>(
        &'a self,
        _target: ChannelTarget<'a>,
        _own_user_id: &'a str,
        _shown: bool,
        _now_ms: i64,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(Error::Unsupported("channel visibility without chatsvc")) })
    }
    fn set_team_shown<'a>(
        &'a self,
        _team_thread_id: &'a str,
        _shown: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(Error::Unsupported("team visibility without chatsvc")) })
    }
}

pub struct ChatsvcFolderSource {
    pins: Pins<SessionTransport>,
    visibility: ChannelVisibility,
    settings: UserSettings,
    channel_settings: ChannelSettings,
    properties: Mutex<Option<Value>>,
    skin_tone: Mutex<Option<u8>>,
    default_reactions: Mutex<Option<Vec<String>>>,
    group_ids: Mutex<HashMap<String, String>>,
}

impl ChatsvcFolderSource {
    pub fn new(session: &Session) -> Self {
        ChatsvcFolderSource {
            pins: Pins::new(session),
            visibility: ChannelVisibility::new(session),
            settings: UserSettings::new(session),
            channel_settings: ChannelSettings::new(session),
            properties: Mutex::new(None),
            skin_tone: Mutex::new(None),
            default_reactions: Mutex::new(None),
            group_ids: Mutex::new(HashMap::new()),
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

    fn set_channel_pinned<'a>(
        &'a self,
        channel_id: &'a str,
        pinned: bool,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move {
            let answer = if pinned {
                self.pins.pin_channel(channel_id).await?
            } else {
                self.pins.unpin_channel(channel_id).await?
            };
            Ok(answer.channel_ids)
        })
    }

    fn set_channel_shown<'a>(
        &'a self,
        target: ChannelTarget<'a>,
        own_user_id: &'a str,
        shown: bool,
        now_ms: i64,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            Ok(self
                .visibility
                .set_channel_shown(&target, own_user_id, shown, now_ms)
                .await?)
        })
    }

    fn set_team_shown<'a>(
        &'a self,
        team_thread_id: &'a str,
        shown: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            Ok(self
                .visibility
                .set_team_shown(team_thread_id, shown)
                .await?)
        })
    }

    fn team_layout(&self) -> BoxFuture<'_, Result<Vec<TeamLayoutRecord>>> {
        Box::pin(async {
            let layout = self.pins.team_layout().await?;
            *self.group_ids.lock().unwrap() = group_ids_by_thread(&layout);
            let unknown: Vec<String> = layout
                .iter()
                .flat_map(|team| &team.channels)
                .filter(|channel| channel.notifications.is_none())
                .map(|channel| channel.channel_id.clone())
                .collect();
            let fallback = if unknown.is_empty() {
                HashMap::new()
            } else {
                self.channel_settings
                    .fetch(&unknown)
                    .await
                    .unwrap_or_default()
            };
            Ok(layout
                .into_iter()
                .map(|team| layout_record(team, &fallback))
                .collect())
        })
    }

    fn section_settings(&self) -> BoxFuture<'_, Result<ChatSectionSettings>> {
        Box::pin(async {
            let properties = self.settings.properties().await?;
            let sections = parse_chat_sections(&properties);
            *self.skin_tone.lock().unwrap() = parse_skin_tone(&properties);
            *self.default_reactions.lock().unwrap() = parse_default_reactions(&properties);
            *self.properties.lock().unwrap() = Some(properties);
            Ok(sections)
        })
    }

    fn preferred_skin_tone(&self) -> Option<u8> {
        *self.skin_tone.lock().unwrap()
    }

    fn set_skin_tone(&self, tone: u8) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { Ok(self.settings.set_skin_tone(tone).await?) })
    }

    fn default_reactions(&self) -> Option<Vec<String>> {
        self.default_reactions.lock().unwrap().clone()
    }

    fn set_default_reactions<'a>(
        &'a self,
        reaction_ids: &'a [String],
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(self.settings.set_default_reactions(reaction_ids).await?) })
    }

    fn teams_order(&self) -> BoxFuture<'_, Result<Vec<(String, i64)>>> {
        Box::pin(async {
            let cached = self.properties.lock().unwrap().take();
            let properties = match cached {
                Some(properties) => properties,
                None => self.settings.properties().await?,
            };
            let known = self.group_ids.lock().unwrap().clone();
            let group_ids = if known.is_empty() {
                group_ids_by_thread(&self.pins.team_layout().await?)
            } else {
                known
            };
            Ok(order_by_group_id(
                parse_teams_order(&properties),
                &group_ids,
            ))
        })
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
        index: Option<usize>,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>> {
        Box::pin(async move {
            Ok(convert(
                self.pins
                    .move_to_folder(conversation_id, target_folder_id, index)
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

    fn change_folders(&self, change: FolderChange) -> BoxFuture<'_, Result<Vec<ChatFolder>>> {
        Box::pin(async move {
            let folders = match &change {
                FolderChange::Create { name } => self.pins.create_folder(name).await?,
                FolderChange::Rename { folder_id, name } => {
                    self.pins.rename_folder(folder_id, name).await?
                }
                FolderChange::Delete { folder_id } => self.pins.delete_folder(folder_id).await?,
                FolderChange::Move {
                    folder_id,
                    target_folder_id,
                } => self.pins.move_folder(folder_id, target_folder_id).await?,
            };
            Ok(convert(folders))
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

fn stored_team_order(teams: &[store::SidebarTeam]) -> Vec<(String, i64)> {
    teams
        .iter()
        .enumerate()
        .map(|(position, team)| (team.team.id.clone(), position as i64))
        .collect()
}

fn group_ids_by_thread(layout: &[chatsvc::TeamLayout]) -> HashMap<String, String> {
    layout
        .iter()
        .filter_map(|team| Some((team.team_id.clone(), team.group_id.clone()?)))
        .collect()
}

fn order_by_group_id(
    order: Vec<(String, i64)>,
    group_ids: &HashMap<String, String>,
) -> Vec<(String, i64)> {
    order
        .into_iter()
        .map(|(team_id, position)| match group_ids.get(&team_id) {
            Some(group_id) => (group_id.clone(), position),
            None => (team_id, position),
        })
        .collect()
}

fn sort_teams_by_order(teams: &mut [TeamLayoutRecord], order: &[(String, i64)]) {
    let positions: HashMap<&str, i64> = order
        .iter()
        .map(|(team_id, position)| (team_id.as_str(), *position))
        .collect();
    teams.sort_by_key(|team| {
        positions
            .get(team.team_id.as_str())
            .copied()
            .unwrap_or(i64::MAX)
    });
}

fn resolve_notifications(
    channel: &chatsvc::ChannelLayout,
    fallback: &HashMap<String, ChannelNotifications>,
) -> ChannelNotifications {
    channel
        .notifications
        .or_else(|| fallback.get(&channel.channel_id).copied())
        .unwrap_or_else(|| ChannelNotifications::from_followed(channel.followed))
}

fn notification_record(notifications: ChannelNotifications) -> store::ChannelNotifications {
    store::ChannelNotifications {
        level: match notifications.level {
            ChannelNotificationLevel::BannerAndFeed => {
                store::ChannelNotificationLevel::BannerAndFeed
            }
            ChannelNotificationLevel::Feed => store::ChannelNotificationLevel::Feed,
            ChannelNotificationLevel::Off => store::ChannelNotificationLevel::Off,
        },
        include_replies: notifications.include_replies,
    }
}

pub(crate) fn chatsvc_notifications(
    notifications: store::ChannelNotifications,
) -> ChannelNotifications {
    ChannelNotifications {
        level: match notifications.level {
            store::ChannelNotificationLevel::BannerAndFeed => {
                ChannelNotificationLevel::BannerAndFeed
            }
            store::ChannelNotificationLevel::Feed => ChannelNotificationLevel::Feed,
            store::ChannelNotificationLevel::Off => ChannelNotificationLevel::Off,
        },
        include_replies: notifications.include_replies,
    }
}

fn layout_record(
    team: chatsvc::TeamLayout,
    fallback: &HashMap<String, ChannelNotifications>,
) -> TeamLayoutRecord {
    TeamLayoutRecord {
        team_id: team.group_id.unwrap_or(team.team_id),
        hidden: team.hidden,
        channels: team
            .channels
            .into_iter()
            .map(|channel| ChannelLayoutRecord {
                notifications: notification_record(resolve_notifications(&channel, fallback)),
                channel_id: channel.channel_id,
                general: channel.general,
                hidden: channel.hidden,
                tabs: channel
                    .tabs
                    .into_iter()
                    .map(|tab| ChannelTabRecord {
                        tab_id: tab.id,
                        name: tab.name,
                        definition_id: tab.definition_id,
                        open_url: tab.open_url,
                    })
                    .collect(),
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
const META_ADAPTIVE_RAIL: &str = "adaptive_rail";

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
        adaptive_rail: read(META_ADAPTIVE_RAIL),
    }
}

pub fn store_section_settings(store: &Store, settings: &ChatSectionSettings) -> Result<()> {
    for (key, value) in [
        (META_MUTED_SECTION, settings.muted),
        (META_MEETING_SECTION, settings.meeting),
        (META_ADAPTIVE_RAIL, settings.adaptive_rail),
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
        let folders = match tokio::try_join!(source.folders(), source.pinned_channels()) {
            Ok((folders, channels)) => self.store_folders(&folders, &channels),
            Err(error) => Err(error),
        };
        self.refresh_section_settings(source).await;
        let layout = self.refresh_team_layout(source).await;
        match (folders, layout) {
            (Err(folders_error), Err(layout_error)) => {
                Err(Error::Folders(format!("{folders_error}; {layout_error}")))
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    async fn refresh_section_settings(&self, source: &Arc<dyn FolderSource>) {
        let outcome = match source.section_settings().await {
            Ok(settings) => self
                .apply_section_settings(&settings)
                .and_then(|()| match source.preferred_skin_tone() {
                    Some(tone) => crate::skin_tone::store_account_skin_tone(&self.store, tone),
                    None => Ok(()),
                })
                .and_then(|()| {
                    crate::default_reactions::store_account_default_reactions(
                        &self.store,
                        source.default_reactions().as_deref(),
                    )
                }),
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

    pub async fn set_skin_tone(&self, tone: u8) -> Result<()> {
        self.folder_source()?.set_skin_tone(tone).await?;
        crate::skin_tone::store_account_skin_tone(&self.store, tone)
    }

    pub async fn set_default_reactions(&self, reaction_ids: &[String]) -> Result<()> {
        self.folder_source()?
            .set_default_reactions(reaction_ids)
            .await?;
        crate::default_reactions::store_account_default_reactions(&self.store, Some(reaction_ids))
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
        let mut layout = source.team_layout().await?;
        let before = self.store.sidebar()?.teams;
        let order = match source.teams_order().await {
            Ok(order) => order,
            Err(error) => {
                self.report(format!("cannot read the team order: {error}"));
                stored_team_order(&before)
            }
        };
        sort_teams_by_order(&mut layout, &order);
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
        index: Option<usize>,
    ) -> Result<()> {
        let folders = self
            .folder_source()?
            .move_to_folder(conversation_id, target_folder_id, index)
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

    pub async fn change_folders(&self, change: FolderChange) -> Result<()> {
        let folders = self.folder_source()?.change_folders(change).await?;
        self.store_folders(&folders, &self.store.pinned_channel_ids()?)
    }

    pub(crate) fn folder_source(&self) -> Result<&Arc<dyn FolderSource>> {
        self.folder_source
            .as_ref()
            .ok_or(Error::Unsupported("chat folders without a folder source"))
    }

    pub(crate) fn store_folders(
        &self,
        folders: &[ChatFolder],
        channel_ids: &[String],
    ) -> Result<()> {
        let previous = (self.chat_folders()?, self.store.pinned_channel_ids()?);
        let records: Vec<FolderRecord> = folders.iter().map(to_record).collect();
        self.store.replace_folders(&records, channel_ids)?;
        if previous != (folders.to_vec(), channel_ids.to_vec()) {
            let _ = self.events.send(CoreEvent::FoldersChanged);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(
        notifications: Option<ChannelNotifications>,
        followed: bool,
    ) -> chatsvc::ChannelLayout {
        chatsvc::ChannelLayout {
            channel_id: "19:a".into(),
            general: false,
            hidden: false,
            tabs: Vec::new(),
            notifications,
            followed,
        }
    }

    fn level(level: ChannelNotificationLevel, include_replies: bool) -> ChannelNotifications {
        ChannelNotifications {
            level,
            include_replies,
        }
    }

    fn team(team_id: &str) -> TeamLayoutRecord {
        TeamLayoutRecord {
            team_id: team_id.into(),
            hidden: false,
            channels: Vec::new(),
        }
    }

    fn team_ids(teams: &[TeamLayoutRecord]) -> Vec<&str> {
        teams.iter().map(|team| team.team_id.as_str()).collect()
    }

    #[test]
    fn listed_teams_come_first_by_order_and_unlisted_follow_in_server_order() {
        let mut teams = vec![team("x"), team("b"), team("y"), team("a"), team("z")];
        let order = vec![
            ("a".to_owned(), 0),
            ("b".to_owned(), 1),
            ("gone".to_owned(), 2),
        ];
        sort_teams_by_order(&mut teams, &order);
        assert_eq!(team_ids(&teams), ["a", "b", "x", "y", "z"]);
    }

    #[test]
    fn real_payloads_give_the_team_order_teams_web_shows() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/team_order.json")).unwrap();
        let layout = chatsvc::pins::parse_team_layout(&fixture["csaTeams"]).unwrap();
        let group_ids = group_ids_by_thread(&layout);
        let order = order_by_group_id(parse_teams_order(&fixture["userProperties"]), &group_ids);
        let mut records: Vec<TeamLayoutRecord> = layout
            .into_iter()
            .map(|team| layout_record(team, &HashMap::new()))
            .collect();
        sort_teams_by_order(&mut records, &order);
        let store = Store::open_in_memory().unwrap();
        let names: HashMap<String, String> = fixture["csaTeams"]["teams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|team| {
                let group_id = team["teamSiteInformation"]["groupId"].as_str().unwrap();
                let name = team["displayName"].as_str().unwrap();
                (group_id.to_owned(), name.to_owned())
            })
            .collect();
        let graph_teams: Vec<store::TeamRecord> = names
            .iter()
            .map(|(group_id, name)| store::TeamRecord {
                id: group_id.clone(),
                name: name.clone(),
            })
            .collect();
        store.upsert_teams(&graph_teams).unwrap();
        store.replace_team_layout(&records).unwrap();

        let shown: Vec<String> = store
            .sidebar()
            .unwrap()
            .teams
            .into_iter()
            .filter(|team| !team.hidden)
            .map(|team| team.team.id)
            .collect();
        let expected: Vec<String> = fixture["teamsWebShownTeams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|team| team["groupId"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(shown, expected);
    }

    #[test]
    fn an_empty_order_keeps_the_server_order() {
        let mut teams = vec![team("c"), team("a"), team("b")];
        sort_teams_by_order(&mut teams, &[]);
        assert_eq!(team_ids(&teams), ["c", "a", "b"]);
    }

    #[test]
    fn channel_value_beats_the_notification_service_beats_the_followed_flag() {
        let off = level(ChannelNotificationLevel::Off, false);
        let banner = level(ChannelNotificationLevel::BannerAndFeed, true);
        let fallback = HashMap::from([("19:a".to_owned(), banner)]);
        assert_eq!(
            resolve_notifications(&channel(Some(off), true), &fallback),
            off
        );
        assert_eq!(
            resolve_notifications(&channel(None, false), &fallback),
            banner
        );
        assert_eq!(
            resolve_notifications(&channel(None, true), &HashMap::new()),
            banner
        );
        assert_eq!(
            resolve_notifications(&channel(None, false), &HashMap::new()),
            ChannelNotifications::default()
        );
    }
}
