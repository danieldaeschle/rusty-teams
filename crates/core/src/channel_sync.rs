use chrono::{DateTime, Duration, Utc};
use graph::Message;
use store::{MessageRecord, SyncState};

use crate::engine::{Delta, SyncEngine, sorted, thread_records};
use crate::error::Result;
use crate::mapping::message_record;
use crate::remote::{DeltaPage, Remote};

const MAX_DELTA_AGE_DAYS: i64 = 200;

struct DeltaRun {
    items: Vec<Message>,
    link: Option<String>,
}

impl<R: Remote> SyncEngine<R> {
    pub(crate) async fn fetch_newer_channel(
        &self,
        team_id: &str,
        channel_id: &str,
    ) -> Result<Delta> {
        let mut previous = self.store.sync_state(channel_id)?.unwrap_or_default();
        if let Some(link) = previous.delta_link.take() {
            let start = DeltaStart::Link(link);
            let with_link = SyncState {
                delta_link: None,
                ..previous.clone()
            };
            if let Ok(delta) = self
                .fetch_channel_delta(team_id, channel_id, &with_link, start)
                .await
            {
                return Ok(delta);
            }
            self.store.set_sync_state(channel_id, &previous)?;
        }
        self.refresh_channel_first_page(team_id, channel_id, previous)
            .await
    }

    async fn refresh_channel_first_page(
        &self,
        team_id: &str,
        channel_id: &str,
        previous: SyncState,
    ) -> Result<Delta> {
        let started_at = Utc::now();
        let page = self
            .remote
            .channel_messages(team_id, channel_id, self.config.page_size)
            .await?;
        let next_link = page.next_link.clone();
        let records = thread_records(channel_id, &page.items);
        let newest_fetched = records.iter().map(|record| record.created_at).max();
        let state = match previous.newest_seen {
            None => SyncState {
                newest_seen: newest_fetched,
                oldest_loaded: records.iter().map(|record| record.created_at).min(),
                has_more: next_link.is_some(),
                older_cursor: next_link,
                delta_link: None,
            },
            Some(known) => SyncState {
                newest_seen: Some(known.max(newest_fetched.unwrap_or(known))),
                ..previous.clone()
            },
        };
        let mut delta = self.ingest(channel_id, records)?;
        self.store.set_sync_state(channel_id, &state)?;
        let since = bootstrap_start(previous.newest_seen, started_at);
        if let Ok(extra) = self
            .fetch_channel_delta(team_id, channel_id, &state, DeltaStart::Since(since))
            .await
        {
            delta.merge(extra);
        }
        Ok(delta)
    }

    async fn fetch_channel_delta(
        &self,
        team_id: &str,
        channel_id: &str,
        state: &SyncState,
        start: DeltaStart,
    ) -> Result<Delta> {
        let run = self.run_delta(team_id, channel_id, start).await?;
        let records = self.delta_records(team_id, channel_id, &run.items).await?;
        let newest = records.iter().map(|record| record.created_at).max();
        let delta = self.ingest(channel_id, records)?;
        let newest_seen = match (state.newest_seen, newest) {
            (Some(known), Some(found)) => Some(known.max(found)),
            (known, found) => known.or(found),
        };
        self.store.set_sync_state(
            channel_id,
            &SyncState {
                newest_seen,
                delta_link: run.link,
                ..state.clone()
            },
        )?;
        Ok(delta)
    }

    async fn run_delta(
        &self,
        team_id: &str,
        channel_id: &str,
        start: DeltaStart,
    ) -> Result<DeltaRun> {
        let mut page = match start {
            DeltaStart::Link(link) => self.remote.channel_delta_at(&link).await?,
            DeltaStart::Since(since) => {
                self.remote
                    .channel_delta(team_id, channel_id, Some(since))
                    .await?
            }
        };
        let mut items = Vec::new();
        for _ in 1..self.config.max_delta_pages.max(1) {
            items.append(&mut page.items);
            let DeltaPage {
                next_link: Some(next),
                ..
            } = &page
            else {
                break;
            };
            page = self.remote.channel_delta_at(next).await?;
        }
        items.append(&mut page.items);
        let link = page.delta_link.or(page.next_link);
        Ok(DeltaRun { items, link })
    }

    async fn delta_records(
        &self,
        team_id: &str,
        channel_id: &str,
        items: &[Message],
    ) -> Result<Vec<MessageRecord>> {
        let mut records: Vec<MessageRecord> = items
            .iter()
            .filter_map(|message| message_record(channel_id, message))
            .collect();
        let root_ids: Vec<String> = items
            .iter()
            .filter(|message| message.reply_to_id.is_none() && !message.is_deleted())
            .filter(|message| message_record(channel_id, message).is_some())
            .map(|message| message.id.clone())
            .collect();
        if !root_ids.is_empty() {
            let listings = self
                .remote
                .channel_replies(team_id, channel_id, &root_ids)
                .await?;
            for listing in listings {
                records.extend(
                    listing?
                        .iter()
                        .filter_map(|reply| message_record(channel_id, reply)),
                );
            }
        }
        let vanished: Vec<String> = items
            .iter()
            .filter(|message| message.is_deleted() && message_record(channel_id, message).is_none())
            .map(|message| message.id.clone())
            .collect();
        if !vanished.is_empty() {
            let cached = self.store.messages_by_id(channel_id, &vanished)?;
            records.extend(cached.into_values().map(|record| MessageRecord {
                deleted: true,
                body_html: String::new(),
                ..record
            }));
        }
        Ok(records)
    }

    pub(crate) async fn refresh_channel_thread(
        &self,
        team_id: &str,
        channel_id: &str,
        root_id: &str,
    ) -> Result<Delta> {
        let listings = self
            .remote
            .channel_replies(team_id, channel_id, &[root_id.to_owned()])
            .await?;
        let mut records = Vec::new();
        for listing in listings {
            records.extend(
                listing?
                    .iter()
                    .filter_map(|reply| message_record(channel_id, reply)),
            );
        }
        self.ingest(channel_id, records)
    }

    pub(crate) async fn load_older_channel(
        &self,
        channel_id: &str,
        state: SyncState,
    ) -> Result<Vec<MessageRecord>> {
        let Some(cursor) = state.older_cursor.as_deref() else {
            self.store.set_sync_state(
                channel_id,
                &SyncState {
                    has_more: false,
                    ..state
                },
            )?;
            return Ok(Vec::new());
        };
        let page = self.remote.channel_messages_at(cursor).await?;
        let records = thread_records(channel_id, &page.items);
        self.ingest(channel_id, records.clone())?;
        let page_oldest = records.iter().map(|record| record.created_at).min();
        self.store.set_sync_state(
            channel_id,
            &SyncState {
                oldest_loaded: page_oldest.or(state.oldest_loaded),
                has_more: page.next_link.is_some(),
                older_cursor: page.next_link,
                ..state
            },
        )?;
        Ok(sorted(records))
    }
}

enum DeltaStart {
    Link(String),
    Since(DateTime<Utc>),
}

fn bootstrap_start(newest_seen: Option<DateTime<Utc>>, started_at: DateTime<Utc>) -> DateTime<Utc> {
    let oldest_allowed = started_at - Duration::days(MAX_DELTA_AGE_DAYS);
    newest_seen.unwrap_or(started_at).max(oldest_allowed)
}
