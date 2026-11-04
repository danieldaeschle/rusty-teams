use std::sync::atomic::Ordering;

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

fn store_activity_feed(store: &Store, snapshot: &ActivitySnapshot) -> Result<bool> {
    if stored_activity_feed(store).as_ref() == Some(snapshot) {
        return Ok(false);
    }
    let Ok(text) = serde_json::to_string(snapshot) else {
        return Ok(false);
    };
    store.set_meta(META_ACTIVITY_FEED, &text)?;
    Ok(true)
}

impl<R: Remote> SyncEngine<R> {
    /// The server feed, stored for the Activity panel; its mentions feed the channel badges.
    pub(crate) async fn refresh_activity_feed(&self) -> Result<Vec<MentionActivity>> {
        let snapshot = self.remote.activity_feed().await?;
        let mentions = mentions_of(&snapshot);
        if store_activity_feed(&self.store, &snapshot)? {
            let _ = self.events.send(CoreEvent::ActivityChanged);
        }
        Ok(mentions)
    }

    /// A push on the feed conversation: one refresh at a time, later pushes in the same burst ride along.
    pub async fn refresh_activity(&self) {
        if self.activity_refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Err(error) = self.refresh_activity_feed().await {
            self.report(format!("cannot refresh the activity feed: {error}"));
        }
        self.activity_refreshing.store(false, Ordering::Release);
    }

    pub async fn mark_activity_read(&self, message_id: &str) -> Result<()> {
        self.remote.mark_activity_read(message_id).await?;
        self.update_stored_feed(|snapshot| {
            snapshot.mark_read(message_id);
        })
    }

    pub async fn mark_all_activity_read(&self) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        self.remote.mark_all_activity_read(now_ms).await?;
        self.update_stored_feed(|snapshot| snapshot.mark_all_read(now_ms))
    }

    fn update_stored_feed(&self, change: impl FnOnce(&mut ActivitySnapshot)) -> Result<()> {
        let Some(mut snapshot) = stored_activity_feed(&self.store) else {
            return Ok(());
        };
        change(&mut snapshot);
        store_activity_feed(&self.store, &snapshot)?;
        Ok(())
    }
}
