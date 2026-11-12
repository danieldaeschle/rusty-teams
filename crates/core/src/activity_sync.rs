use std::collections::HashSet;

use chatsvc::{ActivitySnapshot, MentionActivity, mentions_of};
use chrono::Utc;
use store::Store;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::events::CoreEvent;
use crate::remote::Remote;

const META_ACTIVITY_FEED: &str = "activity_feed";

pub fn stored_activity_feed(store: &Store) -> Option<ActivitySnapshot> {
    let text = store.meta(META_ACTIVITY_FEED).ok().flatten()?;
    serde_json::from_str(&text).ok()
}

#[derive(Default)]
pub(crate) struct ActivityState {
    refreshing: bool,
    refresh_requested: bool,
    snapshot: Option<ActivitySnapshot>,
    loaded: bool,
    marked_read: HashSet<String>,
    marked_all_read_ms: Option<i64>,
}

impl ActivityState {
    fn begin_refresh(&mut self) -> bool {
        if self.refreshing {
            self.refresh_requested = true;
            return false;
        }
        self.refreshing = true;
        true
    }

    fn refresh_again(&mut self) -> bool {
        if std::mem::take(&mut self.refresh_requested) {
            return true;
        }
        self.refreshing = false;
        false
    }

    /// A fetch can start before a mark-read PUT lands; keep the mark until the server shows it.
    fn apply_local_marks(&mut self, snapshot: &mut ActivitySnapshot) {
        self.marked_read.retain(|message_id| {
            snapshot
                .items
                .iter()
                .any(|item| item.message_id == *message_id && !snapshot.is_read(item))
        });
        for message_id in &self.marked_read {
            snapshot.mark_read(message_id);
        }
        match self.marked_all_read_ms {
            Some(marked_ms) if snapshot.horizons.bookmark_ms >= marked_ms => {
                self.marked_all_read_ms = None;
            }
            Some(marked_ms) => snapshot.mark_all_read(marked_ms),
            None => {}
        }
    }

    fn load(&mut self, store: &Store) {
        if !self.loaded {
            self.snapshot = stored_activity_feed(store);
            self.loaded = true;
        }
    }

    fn current(&mut self, store: &Store) -> Option<ActivitySnapshot> {
        self.load(store);
        self.snapshot.clone()
    }

    fn commit(&mut self, store: &Store, snapshot: ActivitySnapshot) -> Result<bool> {
        self.load(store);
        if self.snapshot.as_ref() == Some(&snapshot) {
            return Ok(false);
        }
        let Ok(text) = serde_json::to_string(&snapshot) else {
            return Ok(false);
        };
        store.set_meta(META_ACTIVITY_FEED, &text)?;
        self.snapshot = Some(snapshot);
        Ok(true)
    }
}

impl<R: Remote> SyncEngine<R> {
    /// The server feed, stored for the Activity panel; its mentions feed the channel badges.
    pub(crate) async fn refresh_activity_feed(&self) -> Result<Vec<MentionActivity>> {
        let mut snapshot = self.remote.activity_feed().await?;
        let mentions;
        let changed = {
            let mut state = self.activity.lock().unwrap();
            state.apply_local_marks(&mut snapshot);
            mentions = mentions_of(&snapshot);
            state.commit(&self.store, snapshot)?
        };
        if changed {
            let _ = self.events.send(CoreEvent::ActivityChanged);
        }
        Ok(mentions)
    }

    /// A push on the feed conversation: one refresh at a time, a push during it triggers one more.
    pub async fn refresh_activity(&self) {
        if !self.activity.lock().unwrap().begin_refresh() {
            return;
        }
        loop {
            if let Err(error) = self.refresh_activity_feed().await {
                self.report(format!("cannot refresh the activity feed: {error}"));
            }
            if !self.activity.lock().unwrap().refresh_again() {
                break;
            }
        }
    }

    pub async fn mark_activity_read(&self, message_id: &str) -> Result<()> {
        self.activity
            .lock()
            .unwrap()
            .marked_read
            .insert(message_id.to_owned());
        if let Err(error) = self.remote.mark_activity_read(message_id).await {
            self.activity.lock().unwrap().marked_read.remove(message_id);
            return Err(error);
        }
        self.update_stored_feed(|snapshot| {
            snapshot.mark_read(message_id);
        })
    }

    pub async fn mark_all_activity_read(&self) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        self.activity.lock().unwrap().marked_all_read_ms = Some(now_ms);
        if let Err(error) = self.remote.mark_all_activity_read(now_ms).await {
            self.activity.lock().unwrap().marked_all_read_ms = None;
            return Err(error);
        }
        self.update_stored_feed(|snapshot| snapshot.mark_all_read(now_ms))
    }

    fn update_stored_feed(&self, change: impl FnOnce(&mut ActivitySnapshot)) -> Result<()> {
        let mut state = self.activity.lock().unwrap();
        let Some(mut snapshot) = state.current(&self.store) else {
            return Ok(());
        };
        change(&mut snapshot);
        if state.commit(&self.store, snapshot)? {
            let _ = self.events.send(CoreEvent::ActivityChanged);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chatsvc::{ActivityHorizons, ActivityItem};

    use super::*;

    fn unread_item(message_id: &str) -> ActivityItem {
        ActivityItem {
            message_id: message_id.to_owned(),
            timestamp_ms: 20,
            version_ms: 20,
            ..ActivityItem::default()
        }
    }

    fn snapshot_of(items: Vec<ActivityItem>, bookmark_ms: i64) -> ActivitySnapshot {
        ActivitySnapshot {
            items,
            horizons: ActivityHorizons {
                seen_ms: 0,
                bookmark_ms,
            },
        }
    }

    #[test]
    fn a_push_during_a_refresh_runs_one_more_refresh() {
        let mut state = ActivityState::default();
        assert!(state.begin_refresh());
        assert!(!state.begin_refresh());
        assert!(!state.begin_refresh());
        assert!(state.refresh_again());
        assert!(!state.refresh_again());
        assert!(state.begin_refresh());
    }

    #[test]
    fn a_local_mark_survives_a_stale_fetch_until_the_server_shows_it() {
        let mut state = ActivityState::default();
        state.marked_read.insert("1".to_owned());
        let mut stale = snapshot_of(vec![unread_item("1"), unread_item("2")], 0);
        state.apply_local_marks(&mut stale);
        assert!(stale.is_read(&stale.items[0]));
        assert!(!stale.is_read(&stale.items[1]));
        assert!(state.marked_read.contains("1"));

        let mut fresh = snapshot_of(vec![unread_item("1")], 0);
        fresh.items[0].read_flag = true;
        state.apply_local_marks(&mut fresh);
        assert!(state.marked_read.is_empty());

        let mut marked_unread_elsewhere = snapshot_of(vec![unread_item("1")], 0);
        state.apply_local_marks(&mut marked_unread_elsewhere);
        assert!(!marked_unread_elsewhere.is_read(&marked_unread_elsewhere.items[0]));
    }

    #[test]
    fn a_pending_mark_all_holds_until_the_bookmark_reaches_it() {
        let mut state = ActivityState {
            marked_all_read_ms: Some(50),
            ..ActivityState::default()
        };
        let mut stale = snapshot_of(vec![unread_item("1")], 0);
        state.apply_local_marks(&mut stale);
        assert!(stale.is_read(&stale.items[0]));
        assert_eq!(state.marked_all_read_ms, Some(50));
        let mut fresh = snapshot_of(vec![unread_item("1")], 50);
        state.apply_local_marks(&mut fresh);
        assert_eq!(state.marked_all_read_ms, None);
    }

    #[test]
    fn writes_start_from_the_snapshot_in_memory() {
        let store = Store::open_in_memory().unwrap();
        let mut state = ActivityState::default();
        assert!(
            state
                .commit(&store, snapshot_of(vec![unread_item("1")], 0))
                .unwrap()
        );
        assert!(
            !state
                .commit(&store, snapshot_of(vec![unread_item("1")], 0))
                .unwrap()
        );
        store.set_meta(META_ACTIVITY_FEED, "not json").unwrap();
        assert_eq!(state.current(&store).unwrap().items.len(), 1);
    }
}
