use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chatsvc::{ChatApp, TrouterEndpoint};
use chrono::{DateTime, Duration, Utc};
use graph::{DriveFolder, Graph, MESSAGE_PAGE_SIZE, Message};
use store::{ImageFileCache, MessageRecord, Sidebar, Store, SyncState};
use tokio::sync::{OnceCell, broadcast};

use crate::error::{Error, Result};
use crate::events::CoreEvent;
use crate::folders::FolderSource;
use crate::links::{LinkPreview, has_stored_links};
use crate::mapping::{flatten_thread, message_record};
use crate::people::DirectoryCache;
use crate::presence::Presence;
use crate::preview::OwnedPreview;
use crate::receipts::ReceiptCache;
use crate::remote::{Remote, RemotePage};

const EVENT_CAPACITY: usize = 64;
pub(crate) const META_USER_ID: &str = "me_user_id";
const META_TENANT_ID: &str = "me_tenant_id";
const META_DISPLAY_NAME: &str = "me_display_name";

#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub chat_limit: usize,
    pub chat_page_size: usize,
    pub page_size: usize,
    pub open_limit: usize,
    pub max_catch_up_pages: usize,
    pub max_delta_pages: usize,
    pub teams_refresh_interval: Duration,
    pub team_layout_refresh_interval: Duration,
    pub full_chat_refresh_interval: Duration,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            chat_limit: 200,
            chat_page_size: 25,
            page_size: MESSAGE_PAGE_SIZE,
            open_limit: MESSAGE_PAGE_SIZE,
            max_catch_up_pages: 10,
            max_delta_pages: 20,
            teams_refresh_interval: Duration::hours(6),
            team_layout_refresh_interval: Duration::minutes(15),
            full_chat_refresh_interval: Duration::days(7),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarSummary {
    pub chats: usize,
    pub chat_pages: usize,
    pub full_chat_refresh: bool,
    pub teams_refreshed: bool,
    pub teams: usize,
    pub channels: usize,
    pub failed_teams: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Delta {
    pub added: Vec<MessageRecord>,
    pub updated: Vec<MessageRecord>,
}

impl Delta {
    pub(crate) fn merge(&mut self, other: Delta) {
        for record in other.added {
            if !self
                .added
                .iter()
                .any(|known| known.message_id == record.message_id)
            {
                self.added.push(record);
            }
        }
        for record in other.updated {
            self.updated
                .retain(|known| known.message_id != record.message_id);
            self.updated.push(record);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub user_id: String,
    pub display_name: String,
}

pub(crate) enum Conversation {
    Chat,
    Channel { team_id: String },
}

pub struct SyncEngine<R: Remote = Graph> {
    pub(crate) remote: R,
    pub(crate) store: Arc<Store>,
    pub(crate) config: SyncConfig,
    pub(crate) events: broadcast::Sender<CoreEvent>,
    my_user_id: OnceCell<String>,
    my_tenant_id: OnceCell<String>,
    pub(crate) folder_source: Option<Arc<dyn FolderSource>>,
    pub(crate) presences: Mutex<HashMap<String, Presence>>,
    pub(crate) watched_presence: Mutex<HashSet<String>>,
    pub(crate) presence_endpoint: Mutex<Option<TrouterEndpoint>>,
    pub(crate) presence_subscribing: tokio::sync::Mutex<()>,
    pub(crate) avatars_in_flight: Mutex<HashSet<String>>,
    pub(crate) images_in_flight: Mutex<HashSet<String>>,
    pub(crate) image_files: Option<ImageFileCache>,
    pub(crate) directory_cache: Mutex<DirectoryCache>,
    preview_changed: AtomicBool,
    pub(crate) receipts: ReceiptCache,
    pub(crate) channel_folders: Mutex<HashMap<String, DriveFolder>>,
    pub(crate) chat_apps: Mutex<HashMap<String, Vec<ChatApp>>>,
    pub(crate) link_previews: Mutex<HashMap<String, Option<LinkPreview>>>,
    pub(crate) links_backfilled: Mutex<HashSet<String>>,
}

impl<R: Remote> SyncEngine<R> {
    pub fn new(remote: R, store: Arc<Store>) -> Self {
        Self::with_config(remote, store, SyncConfig::default())
    }

    pub fn with_config(remote: R, store: Arc<Store>, config: SyncConfig) -> Self {
        SyncEngine {
            remote,
            store,
            config,
            events: broadcast::channel(EVENT_CAPACITY).0,
            my_user_id: OnceCell::new(),
            my_tenant_id: OnceCell::new(),
            folder_source: None,
            presences: Mutex::new(HashMap::new()),
            watched_presence: Mutex::new(HashSet::new()),
            presence_endpoint: Mutex::new(None),
            presence_subscribing: tokio::sync::Mutex::new(()),
            avatars_in_flight: Mutex::new(HashSet::new()),
            images_in_flight: Mutex::new(HashSet::new()),
            image_files: None,
            directory_cache: Mutex::new(DirectoryCache::new()),
            preview_changed: AtomicBool::new(false),
            receipts: ReceiptCache::default(),
            channel_folders: Mutex::new(HashMap::new()),
            chat_apps: Mutex::new(HashMap::new()),
            link_previews: Mutex::new(HashMap::new()),
            links_backfilled: Mutex::new(HashSet::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn remote(&self) -> &R {
        &self.remote
    }

    pub fn sidebar(&self) -> Result<Sidebar> {
        Ok(self.store.sidebar()?)
    }

    /// Cached page, newest messages last. Call `fetch_newer` afterwards for the network delta.
    pub fn open_conversation(&self, conversation_id: &str) -> Result<Vec<MessageRecord>> {
        Ok(self
            .store
            .messages(conversation_id, None, self.config.open_limit)?)
    }

    pub async fn fetch_newer(&self, conversation_id: &str) -> Result<Delta> {
        let mut delta = match self.resolve(conversation_id)? {
            Conversation::Chat => self.fetch_newer_chat(conversation_id).await?,
            Conversation::Channel { team_id } => {
                self.fetch_newer_channel(&team_id, conversation_id).await?
            }
        };
        let links_changed = self.attach_delta_links(conversation_id, &mut delta).await
            | self.backfill_links(conversation_id).await;
        self.announce(conversation_id, !delta.is_empty() || links_changed);
        Ok(delta)
    }

    pub async fn refresh_thread(&self, conversation_id: &str, root_id: &str) -> Result<Delta> {
        let Conversation::Channel { team_id } = self.resolve(conversation_id)? else {
            return Err(Error::Unsupported("refreshing a thread of a chat"));
        };
        let delta = self
            .refresh_channel_thread(&team_id, conversation_id, root_id)
            .await?;
        self.announce(conversation_id, !delta.is_empty());
        Ok(delta)
    }

    pub async fn load_older(&self, conversation_id: &str) -> Result<Vec<MessageRecord>> {
        let conversation = self.resolve(conversation_id)?;
        if self.store.sync_state(conversation_id)?.is_none() {
            self.fetch_newer(conversation_id).await?;
        }
        let state = self.store.sync_state(conversation_id)?.unwrap_or_default();
        if !state.has_more {
            return Ok(Vec::new());
        }
        let records = match conversation {
            Conversation::Chat => self.load_older_chat(conversation_id, state).await?,
            Conversation::Channel { .. } => self.load_older_channel(conversation_id, state).await?,
        };
        self.announce(conversation_id, !records.is_empty());
        Ok(records)
    }

    /// Known after the first sidebar refresh.
    pub fn me(&self) -> Option<Me> {
        Some(Me {
            user_id: self.store.meta(META_USER_ID).ok()??,
            display_name: self.store.meta(META_DISPLAY_NAME).ok()??,
        })
    }

    /// Cached chat messages from others after the last read time, at least 1 for an unread chat.
    pub fn unread_count(&self, conversation_id: &str) -> u32 {
        let unread_count = || -> Result<usize> {
            if let Some(channel) = self.store.channel(conversation_id)? {
                return Ok(usize::from(channel.unread));
            }
            let Some(chat) = self.store.chat(conversation_id)?.filter(|chat| chat.unread) else {
                return Ok(0);
            };
            let my_user_id = self.store.meta(META_USER_ID)?.unwrap_or_default();
            let cached =
                self.store
                    .count_messages_after(conversation_id, chat.last_read_at, &my_user_id)?;
            Ok(cached.max(1))
        };
        unread_count().map_or(0, |count| u32::try_from(count).unwrap_or(u32::MAX))
    }

    pub(crate) async fn my_user_id(&self) -> Result<String> {
        let id = self
            .my_user_id
            .get_or_try_init(|| async {
                if let Some(cached) = self.store.meta(META_USER_ID)? {
                    return Ok::<_, Error>(cached);
                }
                self.remember_me().await
            })
            .await?;
        Ok(id.clone())
    }

    pub(crate) async fn ensure_display_name(&self) -> Result<()> {
        if self.store.meta(META_DISPLAY_NAME)?.is_none() {
            self.remember_me().await?;
        }
        Ok(())
    }

    async fn remember_me(&self) -> Result<String> {
        let user = self.remote.me().await?;
        self.store.set_meta(META_USER_ID, &user.id)?;
        if let Some(name) = user.display_name.as_deref() {
            self.store.set_meta(META_DISPLAY_NAME, name)?;
        }
        Ok(user.id)
    }

    pub(crate) async fn my_tenant_id(&self, chat_id: &str) -> Result<String> {
        let tenant = self
            .my_tenant_id
            .get_or_try_init(|| async {
                if let Some(cached) = self.store.meta(META_TENANT_ID)? {
                    return Ok::<_, Error>(cached);
                }
                let my_user_id = self.my_user_id().await?;
                let members = self.remote.chat_members(chat_id).await?;
                let tenant = members
                    .iter()
                    .find(|member| member.user_id.as_deref() == Some(my_user_id.as_str()))
                    .and_then(|member| member.tenant_id.clone())
                    .ok_or(Error::Unsupported("a chat without my tenant id"))?;
                self.store.set_meta(META_TENANT_ID, &tenant)?;
                Ok(tenant)
            })
            .await?;
        Ok(tenant.clone())
    }

    pub(crate) fn resolve(&self, conversation_id: &str) -> Result<Conversation> {
        if let Some(channel) = self.store.channel(conversation_id)? {
            return Ok(Conversation::Channel {
                team_id: channel.team_id,
            });
        }
        if self.store.chat(conversation_id)?.is_some() {
            return Ok(Conversation::Chat);
        }
        Err(Error::UnknownConversation(conversation_id.to_owned()))
    }

    async fn fetch_newer_chat(&self, conversation_id: &str) -> Result<Delta> {
        let previous = self.store.sync_state(conversation_id)?.unwrap_or_default();
        let mut collected = Vec::new();
        let mut before = None;
        let mut oldest_fetched: Option<DateTime<Utc>> = None;
        let mut reached_known = false;
        let mut has_next = false;
        for _ in 0..self.config.max_catch_up_pages.max(1) {
            let page = self
                .remote
                .chat_messages(conversation_id, before, self.config.page_size)
                .await?;
            let page_oldest = oldest_created(&page);
            has_next = page.next_link.is_some();
            collected.extend(
                page.items
                    .iter()
                    .filter_map(|message| message_record(conversation_id, message)),
            );
            oldest_fetched = [oldest_fetched, page_oldest].into_iter().flatten().min();
            let Some(known) = previous.newest_seen else {
                break;
            };
            reached_known = !has_next || page_oldest.is_none_or(|oldest| oldest <= known);
            if reached_known {
                break;
            }
            before = page_oldest;
        }
        let newest_fetched = collected.iter().map(|record| record.created_at).max();
        let state = match previous.newest_seen {
            None => SyncState {
                newest_seen: newest_fetched,
                oldest_loaded: oldest_fetched,
                has_more: has_next,
                older_cursor: None,
                delta_link: None,
            },
            Some(known) if reached_known => SyncState {
                newest_seen: Some(known.max(newest_fetched.unwrap_or(known))),
                ..previous
            },
            Some(known) => SyncState {
                newest_seen: Some(known.max(newest_fetched.unwrap_or(known))),
                oldest_loaded: oldest_fetched,
                has_more: true,
                older_cursor: None,
                delta_link: None,
            },
        };
        let delta = self.ingest(conversation_id, collected)?;
        self.store.set_sync_state(conversation_id, &state)?;
        Ok(delta)
    }

    async fn load_older_chat(
        &self,
        conversation_id: &str,
        state: SyncState,
    ) -> Result<Vec<MessageRecord>> {
        let page = self
            .remote
            .chat_messages(conversation_id, state.oldest_loaded, self.config.page_size)
            .await?;
        let page_oldest = oldest_created(&page);
        let has_more = page.next_link.is_some() && page_oldest.is_some();
        let records: Vec<_> = page
            .items
            .iter()
            .filter_map(|message| message_record(conversation_id, message))
            .collect();
        self.ingest(conversation_id, records.clone())?;
        self.store.set_sync_state(
            conversation_id,
            &SyncState {
                oldest_loaded: page_oldest.or(state.oldest_loaded),
                has_more,
                ..state
            },
        )?;
        Ok(sorted(records))
    }

    pub(crate) fn ingest(
        &self,
        conversation_id: &str,
        mut records: Vec<MessageRecord>,
    ) -> Result<Delta> {
        let ids: Vec<String> = records
            .iter()
            .map(|record| record.message_id.clone())
            .collect();
        let existing = self.store.messages_by_id(conversation_id, &ids)?;
        for record in &mut records {
            if let Some(cached) = existing.get(&record.message_id)
                && !has_stored_links(&record.links_json)
            {
                record.links_json.clone_from(&cached.links_json);
            }
        }
        self.store.upsert_messages(&records)?;
        self.update_chat_preview(&records)?;
        let mut delta = Delta::default();
        for record in sorted(records) {
            match existing.get(&record.message_id) {
                None => delta.added.push(record),
                Some(cached) if *cached != record => delta.updated.push(record),
                Some(_) => {}
            }
        }
        Ok(delta)
    }

    pub(crate) fn update_chat_preview(&self, records: &[MessageRecord]) -> Result<()> {
        let Some(newest) = records.iter().max_by_key(|record| record.created_at) else {
            return Ok(());
        };
        let preview = OwnedPreview::from_record(newest);
        let changed = self.store.update_chat_preview(
            &newest.conversation_id,
            newest.created_at,
            &preview.as_store(),
        )?;
        if changed {
            self.preview_changed.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub(crate) fn announce(&self, conversation_id: &str, changed: bool) {
        if changed {
            let _ = self.events.send(CoreEvent::MessagesChanged {
                conversation_id: conversation_id.to_owned(),
            });
        }
        if self.preview_changed.swap(false, Ordering::SeqCst) {
            let _ = self.events.send(CoreEvent::SidebarChanged);
        }
    }

    pub(crate) fn report(&self, message: String) {
        let _ = self.events.send(CoreEvent::Error { message });
    }
}

fn oldest_created(page: &RemotePage) -> Option<DateTime<Utc>> {
    page.items
        .iter()
        .filter_map(|message| message.created_date_time)
        .min()
}

pub(crate) fn thread_records(channel_id: &str, threads: &[Message]) -> Vec<MessageRecord> {
    threads
        .iter()
        .flat_map(|thread| flatten_thread(channel_id, thread))
        .collect()
}

pub(crate) fn sorted(mut records: Vec<MessageRecord>) -> Vec<MessageRecord> {
    records.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.message_id.cmp(&right.message_id))
    });
    records
}
