use graph::MAX_PRESENCE_IDS;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::events::CoreEvent;
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
            let mut known = self
                .presences
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for entry in found {
                let presence = Presence {
                    availability: Availability::from_service(&entry.availability),
                    in_call: is_call_activity(entry.activity.as_deref()),
                };
                changed |= known.insert(entry.user_id, presence) != Some(presence);
            }
        }
        if changed {
            let _ = self.events.send(CoreEvent::PresenceChanged);
        }
        Ok(())
    }
}
