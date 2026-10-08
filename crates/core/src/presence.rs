use chatsvc::{PresenceUpdate, TrouterEndpoint};
use graph::MAX_PRESENCE_IDS;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::events::CoreEvent;
use crate::receipts::locked;
use crate::remote::Remote;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    Busy,
    DoNotDisturb,
    Away,
    Offline,
    Unknown,
}

impl Availability {
    pub fn from_service(name: &str) -> Self {
        match name {
            "Available" | "AvailableIdle" => Availability::Available,
            "Busy" | "BusyIdle" => Availability::Busy,
            "DoNotDisturb" => Availability::DoNotDisturb,
            "Away" | "BeRightBack" => Availability::Away,
            "Offline" => Availability::Offline,
            _ => Availability::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    pub availability: Availability,
    pub in_call: bool,
}

fn is_call_activity(activity: Option<&str>) -> bool {
    matches!(activity, Some("InACall" | "InAConferenceCall"))
}

impl<R: Remote> SyncEngine<R> {
    pub fn presence(&self, user_id: &str) -> Option<Presence> {
        self.presences
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(user_id)
            .copied()
    }

    pub async fn refresh_presence(&self, user_ids: &[String]) -> Result<()> {
        let mut unique = user_ids.to_vec();
        unique.sort();
        unique.dedup();
        let mut changed = false;
        for chunk in unique.chunks(MAX_PRESENCE_IDS) {
            let found = self.remote.presences(chunk).await?;
            changed |= self.record_presences(found.iter().map(|entry| {
                (
                    &entry.user_id,
                    entry.availability.as_str(),
                    entry.activity.as_deref(),
                )
            }));
        }
        if changed {
            let _ = self.events.send(CoreEvent::PresenceChanged);
        }
        Ok(())
    }

    pub fn apply_presence(&self, updates: &[PresenceUpdate]) {
        let changed = self.record_presences(updates.iter().map(|update| {
            (
                &update.user_id,
                update.availability.as_str(),
                update.activity.as_deref(),
            )
        }));
        if changed {
            let _ = self.events.send(CoreEvent::PresenceChanged);
        }
    }

    pub async fn watch_presence(&self, user_ids: &[String]) -> Result<()> {
        let _subscribing = self.presence_subscribing.lock().await;
        let added = {
            let mut watched = locked(&self.watched_presence);
            let mut added: Vec<String> = user_ids
                .iter()
                .filter(|user_id| watched.insert((*user_id).clone()))
                .cloned()
                .collect();
            added.sort();
            added
        };
        let endpoint = locked(&self.presence_endpoint).clone();
        let Some(endpoint) = endpoint.filter(|_| !added.is_empty()) else {
            return Ok(());
        };
        let outcome = self
            .remote
            .subscribe_presence(&endpoint.endpoint_id, &endpoint.trouter_uri, &added, false)
            .await;
        if outcome.is_err() {
            let mut watched = locked(&self.watched_presence);
            for user_id in &added {
                watched.remove(user_id);
            }
        }
        outcome
    }

    /// No-op for the endpoint already subscribed; a failed subscribe forgets it so the next announcement retries.
    pub async fn presence_endpoint(&self, endpoint: TrouterEndpoint) -> Result<()> {
        let _subscribing = self.presence_subscribing.lock().await;
        if locked(&self.presence_endpoint).as_ref() == Some(&endpoint) {
            return Ok(());
        }
        let outcome = self.subscribe_watched(&endpoint).await;
        *locked(&self.presence_endpoint) = outcome.is_ok().then_some(endpoint);
        outcome
    }

    pub async fn resubscribe_presence(&self) -> Result<()> {
        let _subscribing = self.presence_subscribing.lock().await;
        let endpoint = locked(&self.presence_endpoint).clone();
        match endpoint {
            Some(endpoint) => self.subscribe_watched(&endpoint).await,
            None => Ok(()),
        }
    }

    async fn subscribe_watched(&self, endpoint: &TrouterEndpoint) -> Result<()> {
        let mut watched: Vec<String> = locked(&self.watched_presence).iter().cloned().collect();
        watched.sort();
        if watched.is_empty() {
            return Ok(());
        }
        self.remote
            .subscribe_presence(&endpoint.endpoint_id, &endpoint.trouter_uri, &watched, true)
            .await
    }

    fn record_presences<'a>(
        &self,
        found: impl Iterator<Item = (&'a String, &'a str, Option<&'a str>)>,
    ) -> bool {
        let mut known = locked(&self.presences);
        let mut changed = false;
        for (user_id, availability, activity) in found {
            let presence = Presence {
                availability: Availability::from_service(availability),
                in_call: is_call_activity(activity),
            };
            changed |= known.insert(user_id.clone(), presence) != Some(presence);
        }
        changed
    }
}
