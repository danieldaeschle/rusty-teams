use chatsvc::{
    ForcedAvailability, PresenceStatus, PresenceUpdate, StatusNote, TrouterEndpoint,
    WorkLocationKind,
};
use graph::MAX_PRESENCE_IDS;

use chrono::{DateTime, Utc};

use crate::engine::{META_USER_ID, SyncEngine};
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
pub enum Activity {
    InACall,
    InAConferenceCall,
    InAMeeting,
    Presenting,
    OutOfOffice,
}

impl Activity {
    pub fn from_service(name: &str) -> Option<Self> {
        match name {
            "InACall" => Some(Activity::InACall),
            "InAConferenceCall" => Some(Activity::InAConferenceCall),
            "InAMeeting" => Some(Activity::InAMeeting),
            "Presenting" => Some(Activity::Presenting),
            "OutOfOffice" => Some(Activity::OutOfOffice),
            _ => None,
        }
    }

    pub fn is_call(self) -> bool {
        matches!(self, Activity::InACall | Activity::InAConferenceCall)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    pub availability: Availability,
    pub activity: Option<Activity>,
}

impl Presence {
    pub fn in_call(&self) -> bool {
        self.activity.is_some_and(Activity::is_call)
    }
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

    pub fn concerns_me(&self, updates: &[PresenceUpdate]) -> bool {
        let my_user_id = self.store.meta(META_USER_ID).ok().flatten();
        updates
            .iter()
            .any(|update| my_user_id.as_deref() == Some(update.user_id.as_str()))
    }

    pub async fn refresh_own_status(&self) -> Result<PresenceStatus> {
        let _ = self.ensure_display_name().await;
        let my_user_id = self.my_user_id().await?;
        let status = self.remote.own_status(&my_user_id).await?;
        let changed = self.record_presences(std::iter::once((
            &my_user_id,
            status.availability.as_str(),
            status.activity.as_deref(),
        )));
        let _ = self
            .events
            .send(CoreEvent::PresenceStatusChanged(status.clone()));
        if changed {
            let _ = self.events.send(CoreEvent::PresenceChanged);
        }
        Ok(status)
    }

    pub async fn set_own_availability(&self, forced: Option<ForcedAvailability>) -> Result<()> {
        self.remote.set_availability(forced.as_ref()).await?;
        let _ = self.refresh_own_status().await;
        Ok(())
    }

    pub async fn set_own_status_note(&self, note: Option<StatusNote>) -> Result<()> {
        self.remote.set_status_note(note.as_ref()).await?;
        let _ = self.refresh_own_status().await;
        Ok(())
    }

    pub async fn set_own_work_location(
        &self,
        location: Option<(WorkLocationKind, DateTime<Utc>)>,
    ) -> Result<()> {
        self.remote.set_work_location(location).await?;
        let _ = self.refresh_own_status().await;
        Ok(())
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
                activity: activity.and_then(Activity::from_service),
            };
            changed |= known.insert(user_id.clone(), presence) != Some(presence);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_names_map_and_unknown_is_none() {
        assert_eq!(Activity::from_service("InACall"), Some(Activity::InACall));
        assert_eq!(
            Activity::from_service("InAConferenceCall"),
            Some(Activity::InAConferenceCall)
        );
        assert_eq!(
            Activity::from_service("InAMeeting"),
            Some(Activity::InAMeeting)
        );
        assert_eq!(
            Activity::from_service("Presenting"),
            Some(Activity::Presenting)
        );
        assert_eq!(
            Activity::from_service("OutOfOffice"),
            Some(Activity::OutOfOffice)
        );
        assert_eq!(Activity::from_service("Available"), None);
    }

    #[test]
    fn only_call_activities_count_as_in_call() {
        let presence = |activity| Presence {
            availability: Availability::Busy,
            activity,
        };
        assert!(presence(Some(Activity::InACall)).in_call());
        assert!(presence(Some(Activity::InAConferenceCall)).in_call());
        assert!(!presence(Some(Activity::InAMeeting)).in_call());
        assert!(!presence(None).in_call());
    }
}
