use std::collections::HashMap;

use chatsvc::ConversationState;
use chrono::{DateTime, Duration, Utc};
use store::{ChatRecord, Store};

use crate::chat_paging::ChatPaging;
use crate::engine::{SidebarSummary, SyncEngine};
use crate::error::Result;
use crate::events::CoreEvent;
use crate::hidden_chats::{LocallyHidden, resolve_hidden};
use crate::mapping::{channel_record, chat_record, team_record};
use crate::remote::{ChatsPage, Remote};

const META_CHATS_FULL_AT: &str = "chats_full_refreshed_at";
const META_TEAMS_AT: &str = "teams_refreshed_at";
const META_CHATS_NEXT_LINK: &str = "chats_next_link";
const META_CHATS_EXHAUSTED: &str = "chats_exhausted";

#[derive(Default)]
struct ChatRefresh {
    fetched: usize,
    pages: usize,
    full: bool,
}

#[derive(Default)]
struct TeamsRefresh {
    refreshed: bool,
    teams: usize,
    channels: usize,
    failed: usize,
}

impl<R: Remote> SyncEngine<R> {
    pub async fn refresh_sidebar(&self) -> Result<SidebarSummary> {
        self.refresh_sidebar_with(false).await
    }

    pub async fn refresh_sidebar_full(&self) -> Result<SidebarSummary> {
        self.refresh_sidebar_with(true).await
    }

    async fn refresh_sidebar_with(&self, force: bool) -> Result<SidebarSummary> {
        let (chats, teams) = tokio::join!(self.refresh_chats(force), self.refresh_teams(force));
        let chats = chats?;
        let _ = self.events.send(CoreEvent::SidebarChanged);
        Ok(SidebarSummary {
            chats: chats.fetched,
            chat_pages: chats.pages,
            full_chat_refresh: chats.full,
            teams_refreshed: teams.refreshed,
            teams: teams.teams,
            channels: teams.channels,
            failed_teams: teams.failed,
        })
    }

    async fn refresh_chats(&self, force: bool) -> Result<ChatRefresh> {
        let my_user_id = self.my_user_id().await?;
        self.ensure_display_name().await?;
        let full = force || self.chat_refresh_is_full()?;
        let mut refresh = ChatRefresh {
            full,
            ..ChatRefresh::default()
        };
        let mut kept_ids = Vec::new();
        let mut hidden_ids = Vec::new();
        let mut complete = false;
        let mut continuation = None;
        let states = self.refresh_chat_states().await;
        self.drop_chats_hidden_by_chatsvc(states.as_deref())?;
        let mut page = self.remote.chats_page(self.config.chat_page_size).await?;
        loop {
            refresh.pages += 1;
            refresh.fetched += page.items.len();
            let (records, hidden) = self.split_page(&page, &my_user_id)?;
            let any_changed = self.any_chat_changed(&records)?;
            self.store.upsert_chats(&records)?;
            kept_ids.extend(records.iter().map(|record| record.id.clone()));
            hidden_ids.extend(hidden);
            let wants_more = full || any_changed;
            let within_limit = refresh.fetched < self.config.chat_limit;
            let Some(link) = page.next_link.clone() else {
                complete = true;
                break;
            };
            if !wants_more || !within_limit {
                continuation = Some(link);
                break;
            }
            page = self.remote.chats_at(&link).await?;
        }
        self.store.remove_chats(&hidden_ids)?;
        self.apply_muted_chats(states);
        if full {
            if complete {
                self.store.remove_chats_except(&kept_ids)?;
            }
            self.store.set_meta_time(META_CHATS_FULL_AT, Utc::now())?;
            self.remember_chat_continuation(continuation.filter(|_| !complete))?;
        }
        Ok(refresh)
    }

    async fn refresh_chat_states(&self) -> Option<Vec<ConversationState>> {
        match self.remote.chat_states().await {
            Ok(states) => {
                *self.chat_states.lock().unwrap() = states
                    .iter()
                    .map(|state| (state.id.clone(), state.clone()))
                    .collect();
                Some(states)
            }
            Err(error) => {
                self.report(format!("cannot refresh muted chats: {error}"));
                None
            }
        }
    }

    fn drop_chats_hidden_by_chatsvc(&self, states: Option<&[ConversationState]>) -> Result<()> {
        let reported: Vec<&str> = states
            .unwrap_or_default()
            .iter()
            .filter(|state| state.hidden == Some(true))
            .map(|state| state.id.as_str())
            .collect();
        let now = Utc::now();
        let hidden: Vec<String> = self.with_locally_hidden(|locally| {
            reported
                .iter()
                .filter(|chat_id| locally.reported_hidden(chat_id, now))
                .map(|chat_id| (*chat_id).to_owned())
                .collect()
        })?;
        self.store.remove_chats(&hidden)?;
        Ok(())
    }

    fn apply_muted_chats(&self, states: Option<Vec<ConversationState>>) {
        let Some(states) = states else {
            return;
        };
        let muted: Vec<(String, bool)> = states
            .into_iter()
            .map(|state| (state.id, state.muted))
            .collect();
        if let Err(error) = self.store.apply_muted_states(&muted) {
            self.report(format!("cannot refresh muted chats: {error}"));
        }
    }

    fn chat_refresh_is_full(&self) -> Result<bool> {
        let last_full = self.store.meta_time(META_CHATS_FULL_AT)?;
        Ok(self.store.chat_count()? == 0
            || !self.chat_paging.lock().unwrap().is_known()
            || is_due(last_full, self.config.full_chat_refresh_interval))
    }

    fn remember_chat_continuation(&self, continuation: Option<String>) -> Result<()> {
        let mut paging = self.chat_paging.lock().unwrap();
        paging.adopt_continuation(continuation);
        store_chat_paging(&self.store, &paging)
    }

    pub fn has_older_chats(&self) -> bool {
        self.chat_paging.lock().unwrap().has_more()
    }

    pub fn is_loading_older_chats(&self) -> bool {
        self.chat_paging.lock().unwrap().is_loading()
    }

    pub async fn load_older_chats(&self) -> Result<usize> {
        let Some((link, epoch)) = self.chat_paging.lock().unwrap().begin() else {
            return Ok(0);
        };
        let _ = self.events.send(CoreEvent::SidebarChanged);
        let fetched = self.fetch_older_chats(&link).await;
        let loaded = self.apply_older_chats(fetched, epoch);
        let _ = self.events.send(CoreEvent::SidebarChanged);
        loaded
    }

    async fn fetch_older_chats(&self, link: &str) -> Result<(String, ChatsPage)> {
        let my_user_id = self.my_user_id().await?;
        if self.chat_states.lock().unwrap().is_empty() {
            self.refresh_chat_states().await;
        }
        let page = self.remote.chats_at(link).await?;
        Ok((my_user_id, page))
    }

    fn apply_older_chats(&self, fetched: Result<(String, ChatsPage)>, epoch: u64) -> Result<usize> {
        let mut paging = self.chat_paging.lock().unwrap();
        let (my_user_id, page) = match fetched {
            Ok(fetched) => fetched,
            Err(error) => {
                paging.fail(epoch);
                let _ = store_chat_paging(&self.store, &paging);
                return Err(error);
            }
        };
        if !paging.is_current(epoch) {
            paging.finish(epoch, None);
            return Ok(0);
        }
        match self.store_older_page(&my_user_id, &page) {
            Ok(()) => {
                paging.finish(epoch, page.next_link.clone());
                store_chat_paging(&self.store, &paging)?;
                Ok(page.items.len())
            }
            Err(error) => {
                paging.fail(epoch);
                let _ = store_chat_paging(&self.store, &paging);
                Err(error)
            }
        }
    }

    fn store_older_page(&self, my_user_id: &str, page: &ChatsPage) -> Result<()> {
        let (records, hidden) = self.split_page(page, my_user_id)?;
        self.store.upsert_chats(&records)?;
        self.store.remove_chats(&hidden)?;
        let muted: Vec<(String, bool)> = {
            let states = self.chat_states.lock().unwrap();
            records
                .iter()
                .filter_map(|record| states.get(&record.id))
                .map(|state| (state.id.clone(), state.muted))
                .collect()
        };
        self.store.apply_muted_states(&muted)?;
        Ok(())
    }

    fn split_page(
        &self,
        page: &ChatsPage,
        my_user_id: &str,
    ) -> Result<(Vec<ChatRecord>, Vec<String>)> {
        let now = Utc::now();
        self.with_locally_hidden(|locally| {
            split_hidden(
                page,
                my_user_id,
                &self.chat_states.lock().unwrap(),
                locally,
                now,
            )
        })
    }

    fn any_chat_changed(&self, records: &[ChatRecord]) -> Result<bool> {
        let ids: Vec<String> = records.iter().map(|record| record.id.clone()).collect();
        let cached = self.store.chat_last_message_times(&ids)?;
        Ok(records.iter().any(|record| match cached.get(&record.id) {
            None => true,
            Some(cached_time) => record.last_message_at > *cached_time,
        }))
    }

    async fn refresh_teams(&self, force: bool) -> TeamsRefresh {
        let mut refresh = TeamsRefresh::default();
        match self.store.meta_time(META_TEAMS_AT) {
            Ok(last) if !force && !is_due(last, self.config.teams_refresh_interval) => {
                return refresh;
            }
            Err(error) => {
                self.report(format!("cannot read the team refresh time: {error}"));
                return refresh;
            }
            Ok(_) => {}
        }
        if let Err(error) = self.refresh_teams_now(&mut refresh).await {
            self.report(format!("cannot refresh teams: {error}"));
            return refresh;
        }
        refresh.refreshed = true;
        if refresh.failed == 0
            && let Err(error) = self.store.set_meta_time(META_TEAMS_AT, Utc::now())
        {
            self.report(format!("cannot store the team refresh time: {error}"));
        }
        refresh
    }

    async fn refresh_teams_now(&self, refresh: &mut TeamsRefresh) -> Result<()> {
        let teams = self.remote.joined_teams().await?;
        let records: Vec<_> = teams.iter().map(team_record).collect();
        self.store.upsert_teams(&records)?;
        let team_ids: Vec<String> = records.iter().map(|team| team.id.clone()).collect();
        self.store.remove_teams_except(&team_ids)?;
        refresh.teams = records.len();
        let listings = self.remote.channels_for_teams(&team_ids).await?;
        for (team_id, listing) in team_ids.iter().zip(listings) {
            match listing {
                Ok(channels) => {
                    let channel_records: Vec<_> = channels
                        .iter()
                        .map(|channel| channel_record(team_id, channel))
                        .collect();
                    self.store.upsert_channels(&channel_records)?;
                    let kept: Vec<String> = channel_records
                        .iter()
                        .map(|channel| channel.id.clone())
                        .collect();
                    self.store.remove_channels_except(team_id, &kept)?;
                    refresh.channels += channel_records.len();
                }
                Err(error) => {
                    refresh.failed += 1;
                    self.report(format!("cannot list channels of a team: {error}"));
                }
            }
        }
        Ok(())
    }
}

fn is_due(last: Option<DateTime<Utc>>, interval: Duration) -> bool {
    last.is_none_or(|last| Utc::now() - last >= interval)
}

fn split_hidden(
    page: &ChatsPage,
    my_user_id: &str,
    states: &HashMap<String, ConversationState>,
    locally: &mut LocallyHidden,
    now: DateTime<Utc>,
) -> (Vec<ChatRecord>, Vec<String>) {
    let mut records = Vec::new();
    let mut hidden = Vec::new();
    for chat in &page.items {
        if resolve_hidden(chat, states, locally, now) {
            hidden.push(chat.id.clone());
        } else {
            records.push(chat_record(chat, my_user_id));
        }
    }
    (records, hidden)
}

pub(crate) fn stored_chat_paging(store: &Store) -> ChatPaging {
    let next_link = store
        .meta(META_CHATS_NEXT_LINK)
        .ok()
        .flatten()
        .filter(|link| !link.is_empty());
    let exhausted = store.meta(META_CHATS_EXHAUSTED).ok().flatten().as_deref() == Some("true");
    ChatPaging::restored(next_link, exhausted)
}

fn store_chat_paging(store: &Store, paging: &ChatPaging) -> Result<()> {
    store.set_meta(META_CHATS_NEXT_LINK, paging.next_link().unwrap_or_default())?;
    store.set_meta(META_CHATS_EXHAUSTED, &paging.is_exhausted().to_string())?;
    Ok(())
}
