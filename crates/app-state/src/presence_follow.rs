use gpui_kit::*;
use serde::{Deserialize, Serialize};
use store::Store;

use crate::app_state::{AppEvent, AppState};
use crate::data::{Presence, PresenceKind};

const FOLLOWED_META_KEY: &str = "presence.followed";
pub const NOTIFY_WHEN_AVAILABLE: &str = "Notify when available";
pub const TURN_OFF_NOTIFICATIONS: &str = "Turn off notifications";
pub const MANAGE_STATUS_NOTIFICATIONS: &str = "Manage status notifications";
pub const STATUS_NOTIFICATIONS_DESCRIPTION: &str =
    "Keep track of someone's status and get notified when they appear available or offline.";
pub const TURN_OFF: &str = "Turn off";
pub const NOW_AVAILABLE: &str = "Now available";
pub const NOW_OFFLINE: &str = "Now offline";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowedPerson {
    pub user_id: String,
    pub name: String,
}

pub fn load_followed(store: &Store) -> Vec<FollowedPerson> {
    store
        .meta(FOLLOWED_META_KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save_followed(store: &Store, followed: &[FollowedPerson]) {
    if let Ok(json) = serde_json::to_string(followed) {
        let _ = store.set_meta(FOLLOWED_META_KEY, &json);
    }
}

/// Teams only tells about a change to available or offline from another known status.
pub fn status_change_text(previous: Presence, next: PresenceKind) -> Option<&'static str> {
    let Presence::Live(previous) = previous else {
        return None;
    };
    if previous == next || previous == PresenceKind::Unknown {
        return None;
    }
    match next {
        PresenceKind::Available => Some(NOW_AVAILABLE),
        PresenceKind::Offline => Some(NOW_OFFLINE),
        _ => None,
    }
}

pub fn follow_notice(name: &str, follow: bool) -> String {
    if follow {
        format!("You are now following the presence of {name}")
    } else {
        format!("You are no longer following the presence of {name}")
    }
}

impl AppState {
    pub fn one_on_one_partner(&self, chat_id: &str) -> Option<FollowedPerson> {
        if teams_core::is_notes_chat(chat_id) {
            return None;
        }
        let chat = self.sidebar.chats.iter().find(|chat| chat.id == chat_id)?;
        if !crate::data::is_one_on_one(chat) {
            return None;
        }
        crate::data::others(chat, self.directory.me.as_ref())
            .into_iter()
            .find_map(|(user_id, name)| {
                let user_id = user_id.filter(|user_id| !store::is_bot_id(user_id))?;
                Some(FollowedPerson { user_id, name })
            })
    }

    pub fn is_presence_followed(&self, user_id: &str) -> bool {
        self.followed_presence
            .iter()
            .any(|person| person.user_id == user_id)
    }

    pub fn set_presence_followed(
        &mut self,
        user_id: &str,
        name: &str,
        follow: bool,
        cx: &mut Context<Self>,
    ) {
        if follow == self.is_presence_followed(user_id) {
            return;
        }
        if follow {
            self.followed_presence.push(FollowedPerson {
                user_id: user_id.to_owned(),
                name: name.to_owned(),
            });
            self.request_presence(vec![user_id.to_owned()], cx);
        } else {
            self.followed_presence
                .retain(|person| person.user_id != user_id);
        }
        save_followed(&self.store, &self.followed_presence);
        self.raise_notice(follow_notice(name, follow), None, cx);
        cx.notify();
    }

    pub fn watch_followed_presence(&mut self, cx: &mut Context<Self>) {
        let user_ids: Vec<String> = self
            .followed_presence
            .iter()
            .map(|person| person.user_id.clone())
            .collect();
        if !user_ids.is_empty() {
            self.request_presence(user_ids, cx);
        }
    }

    pub(crate) fn note_followed_change(
        &mut self,
        user_id: &str,
        previous: Presence,
        next: PresenceKind,
    ) {
        if !self.is_presence_followed(user_id) {
            return;
        }
        if let Some(text) = status_change_text(previous, next) {
            self.followed_changes.push((user_id.to_owned(), text));
        }
    }

    pub(crate) fn announce_followed_changes(&mut self, cx: &mut Context<Self>) {
        for (user_id, text) in std::mem::take(&mut self.followed_changes) {
            cx.emit(AppEvent::FollowedStatus { user_id, text });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NOW_AVAILABLE, NOW_OFFLINE, follow_notice, status_change_text};
    use crate::data::{Presence, PresenceKind};

    #[test]
    fn only_a_change_to_available_or_offline_is_announced() {
        let busy = Presence::Live(PresenceKind::Busy);
        assert_eq!(
            status_change_text(busy, PresenceKind::Available),
            Some(NOW_AVAILABLE)
        );
        assert_eq!(
            status_change_text(busy, PresenceKind::Offline),
            Some(NOW_OFFLINE)
        );
        assert_eq!(status_change_text(busy, PresenceKind::Away), None);
        assert_eq!(status_change_text(busy, PresenceKind::OutOfOffice), None);
        let available = Presence::Live(PresenceKind::Available);
        assert_eq!(status_change_text(available, PresenceKind::Available), None);
    }

    #[test]
    fn the_first_known_status_is_not_announced() {
        assert_eq!(
            status_change_text(Presence::Loading, PresenceKind::Available),
            None
        );
        assert_eq!(
            status_change_text(
                Presence::Cached(PresenceKind::Busy),
                PresenceKind::Available
            ),
            None
        );
        assert_eq!(
            status_change_text(
                Presence::Live(PresenceKind::Unknown),
                PresenceKind::Available
            ),
            None
        );
    }

    #[test]
    fn the_notice_uses_the_teams_text() {
        assert_eq!(
            follow_notice("Anna", true),
            "You are now following the presence of Anna"
        );
        assert_eq!(
            follow_notice("Anna", false),
            "You are no longer following the presence of Anna"
        );
    }
}
