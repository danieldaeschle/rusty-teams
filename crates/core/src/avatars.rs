use chrono::{DateTime, Duration, Utc};
use store::{AvatarRecord, is_bot_id};

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::events::CoreEvent;
use crate::remote::Remote;

const AVATAR_MAX_AGE_DAYS: i64 = 7;
const AVATAR_BATCH: usize = 60;
const PHOTO_RENDITION_SINCE: &str = "2026-10-17T17:00:00Z";

fn avatar_cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    let since = DateTime::parse_from_rfc3339(PHOTO_RENDITION_SINCE)
        .map(|since| since.with_timezone(&Utc))
        .unwrap_or(DateTime::UNIX_EPOCH);
    (now - Duration::days(AVATAR_MAX_AGE_DAYS)).max(since)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avatar {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

impl<R: Remote> SyncEngine<R> {
    pub fn avatar(&self, user_id: &str) -> Option<Avatar> {
        let record = self.store.avatar(user_id).ok().flatten()?;
        Some(Avatar {
            bytes: record.bytes?,
            content_type: record.content_type,
        })
    }

    pub async fn fetch_avatars(&self, user_ids: &[String]) -> Result<()> {
        let cutoff = avatar_cutoff(Utc::now());
        let mut unique = user_ids.to_vec();
        unique.sort();
        unique.dedup();
        let wanted = self.store.avatar_ids_needing_fetch(&unique, cutoff)?;
        let claimed = self.claim_avatar_ids(wanted);
        let outcome = self.fetch_claimed_avatars(&claimed).await;
        self.release_avatar_ids(&claimed);
        outcome
    }

    async fn fetch_claimed_avatars(&self, claimed: &[String]) -> Result<()> {
        let (bots, people): (Vec<String>, Vec<String>) = claimed
            .iter()
            .cloned()
            .partition(|user_id| is_bot_id(user_id));
        let mut first_error = None;
        let mut fetched_any = false;
        if !bots.is_empty() {
            match self.fetch_bot_icons(&bots).await {
                Ok(changed) => {
                    fetched_any = true;
                    if !changed.is_empty() {
                        let _ = self
                            .events
                            .send(CoreEvent::AvatarsChanged { user_ids: changed });
                    }
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        for chunk in people.chunks(AVATAR_BATCH) {
            let photos = match self.remote.user_photos(chunk).await {
                Ok(photos) => photos,
                Err(error) => {
                    first_error.get_or_insert(error);
                    continue;
                }
            };
            let mut changed = Vec::new();
            for (user_id, photo) in chunk.iter().zip(photos) {
                let (bytes, content_type) = match photo {
                    Ok(Some(photo)) => (Some(photo.bytes), photo.content_type),
                    Ok(None) => (None, String::new()),
                    Err(error) => {
                        first_error.get_or_insert(error);
                        continue;
                    }
                };
                fetched_any = true;
                if bytes.is_some() {
                    changed.push(user_id.clone());
                }
                self.store.upsert_avatar(&AvatarRecord {
                    user_id: user_id.clone(),
                    bytes,
                    content_type,
                    fetched_at: Utc::now(),
                })?;
            }
            if !changed.is_empty() {
                let _ = self
                    .events
                    .send(CoreEvent::AvatarsChanged { user_ids: changed });
            }
        }
        match first_error {
            Some(error) if !fetched_any => Err(error),
            Some(error) => {
                self.report(format!("some avatars could not be fetched: {error}"));
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn claim_avatar_ids(&self, wanted: Vec<String>) -> Vec<String> {
        let mut in_flight = self
            .avatars_in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        wanted
            .into_iter()
            .filter(|user_id| in_flight.insert(user_id.clone()))
            .collect()
    }

    fn release_avatar_ids(&self, claimed: &[String]) {
        let mut in_flight = self
            .avatars_in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for user_id in claimed {
            in_flight.remove(user_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photos_fetched_before_the_larger_rendition_count_as_stale() {
        let since = DateTime::parse_from_rfc3339(PHOTO_RENDITION_SINCE)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(avatar_cutoff(since + Duration::days(1)), since);
        let later = since + Duration::days(30);
        assert_eq!(
            avatar_cutoff(later),
            later - Duration::days(AVATAR_MAX_AGE_DAYS)
        );
    }
}
