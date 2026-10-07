use chrono::{DateTime, Duration, Utc};
use store::ChatRecord;

use crate::engine::{SidebarSummary, SyncEngine};
use crate::error::Result;
use crate::events::CoreEvent;
use crate::mapping::{channel_record, chat_record, team_record};
use crate::remote::{ChatsPage, Remote};

const META_CHATS_FULL_AT: &str = "chats_full_refreshed_at";
const META_TEAMS_AT: &str = "teams_refreshed_at";

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
        let mut page = self.remote.chats_page(self.config.chat_page_size).await?;
        loop {
            refresh.pages += 1;
            refresh.fetched += page.items.len();
            let (records, hidden) = split_hidden(&page, &my_user_id);
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
                break;
            }
            page = self.remote.chats_at(&link).await?;
        }
        self.store.remove_chats(&hidden_ids)?;
        if full {
            if complete {
                self.store.remove_chats_except(&kept_ids)?;
            }
            self.store.set_meta_time(META_CHATS_FULL_AT, Utc::now())?;
        }
        Ok(refresh)
    }

    fn chat_refresh_is_full(&self) -> Result<bool> {
        let last_full = self.store.meta_time(META_CHATS_FULL_AT)?;
        Ok(self.store.chat_count()? == 0
            || is_due(last_full, self.config.full_chat_refresh_interval))
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

fn split_hidden(page: &ChatsPage, my_user_id: &str) -> (Vec<ChatRecord>, Vec<String>) {
    let mut records = Vec::new();
    let mut hidden = Vec::new();
    for chat in &page.items {
        match chat_record(chat, my_user_id) {
            Some(record) => records.push(record),
            None => hidden.push(chat.id.clone()),
        }
    }
    (records, hidden)
}
