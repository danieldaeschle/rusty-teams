use std::collections::HashMap;

use chatsvc::ConversationState;
use chrono::{DateTime, Duration, TimeZone, Utc};
use graph::Chat;
use store::Store;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::mapping::graph_hidden;
use crate::remote::Remote;

const META_LOCALLY_HIDDEN: &str = "locally_hidden_chats";
const HIDE_PREFIX: &str = "hide";
const UNHIDE_PREFIX: &str = "unhide";

fn unhide_override_window() -> Duration {
    Duration::minutes(10)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LocallyHidden {
    hidden_after: HashMap<String, DateTime<Utc>>,
    unhidden_at: HashMap<String, DateTime<Utc>>,
}

impl LocallyHidden {
    pub(crate) fn load(store: &Store) -> Self {
        let text = store
            .meta(META_LOCALLY_HIDDEN)
            .ok()
            .flatten()
            .unwrap_or_default();
        let mut locally = LocallyHidden::default();
        for line in text.lines() {
            let mut fields = line.splitn(3, '\t');
            let (Some(prefix), Some(chat_id), Some(millis)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Some(at) = millis
                .parse()
                .ok()
                .and_then(|millis| Utc.timestamp_millis_opt(millis).single())
            else {
                continue;
            };
            match prefix {
                HIDE_PREFIX => locally.hidden_after.insert(chat_id.to_owned(), at),
                UNHIDE_PREFIX => locally.unhidden_at.insert(chat_id.to_owned(), at),
                _ => None,
            };
        }
        locally
    }

    pub(crate) fn save(&self, store: &Store) -> Result<()> {
        let mut lines: Vec<String> = [
            (HIDE_PREFIX, &self.hidden_after),
            (UNHIDE_PREFIX, &self.unhidden_at),
        ]
        .into_iter()
        .flat_map(|(prefix, entries)| {
            entries
                .iter()
                .map(move |(chat_id, at)| format!("{prefix}\t{chat_id}\t{}", at.timestamp_millis()))
        })
        .collect();
        lines.sort_unstable();
        store.set_meta(META_LOCALLY_HIDDEN, &lines.join("\n"))?;
        Ok(())
    }

    pub(crate) fn hide(&mut self, chat_id: &str, last_message_at: DateTime<Utc>) {
        self.unhidden_at.remove(chat_id);
        self.hidden_after
            .insert(chat_id.to_owned(), last_message_at);
    }

    pub(crate) fn unhide(&mut self, chat_id: &str, now: DateTime<Utc>) {
        self.hidden_after.remove(chat_id);
        self.unhidden_at.insert(chat_id.to_owned(), now);
    }

    fn unhide_override_active(&self, chat_id: &str, now: DateTime<Utc>) -> bool {
        self.unhidden_at
            .get(chat_id)
            .is_some_and(|at| now - *at < unhide_override_window())
    }

    pub(crate) fn reported_hidden(&mut self, chat_id: &str, now: DateTime<Utc>) -> bool {
        if self.unhide_override_active(chat_id, now) {
            return false;
        }
        self.hidden_after.remove(chat_id);
        true
    }
}

pub(crate) fn resolve_hidden(
    chat: &Chat,
    states: &HashMap<String, ConversationState>,
    locally: &mut LocallyHidden,
    now: DateTime<Utc>,
) -> bool {
    let reported = states.get(&chat.id).and_then(|state| state.hidden);
    if locally.unhidden_at.contains_key(&chat.id) {
        if reported == Some(false) || !locally.unhide_override_active(&chat.id, now) {
            locally.unhidden_at.remove(&chat.id);
        } else {
            return false;
        }
    }
    let cleared_until_ms = states
        .get(&chat.id)
        .and_then(|state| state.history_cleared_ms);
    if history_cleared(chat, cleared_until_ms) {
        return true;
    }
    if reported == Some(true) {
        locally.hidden_after.remove(&chat.id);
        return true;
    }
    if let Some(hidden_after) = locally.hidden_after.get(&chat.id).copied() {
        if chat
            .last_message_time()
            .is_some_and(|newest| newest > hidden_after)
        {
            locally.hidden_after.remove(&chat.id);
            return false;
        }
        return true;
    }
    reported.unwrap_or_else(|| graph_hidden(chat))
}

/// Teams stores the last message time plus one millisecond, or now when the chat has no message.
pub(crate) fn cleared_until_ms(last_message_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> i64 {
    last_message_at.map_or(now.timestamp_millis(), |last| last.timestamp_millis() + 1)
}

/// Teams hides a chat whose history was removed until a newer message arrives.
pub(crate) fn history_cleared(chat: &Chat, cleared_until_ms: Option<i64>) -> bool {
    cleared_until_ms.is_some_and(|cleared_until_ms| {
        chat.last_message_time()
            .is_none_or(|newest| newest.timestamp_millis() <= cleared_until_ms)
    })
}

impl<R: Remote> SyncEngine<R> {
    pub(crate) fn with_locally_hidden<T>(
        &self,
        change: impl FnOnce(&mut LocallyHidden) -> T,
    ) -> Result<T> {
        let _guard = self.locally_hidden_lock.lock().unwrap();
        let mut locally = LocallyHidden::load(&self.store);
        let before = locally.clone();
        let result = change(&mut locally);
        if locally != before {
            locally.save(&self.store)?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn chat(last_message: Option<&str>, graph_hidden: bool) -> Chat {
        serde_json::from_value(json!({
            "id": "19:a",
            "chatType": "meeting",
            "viewpoint": {"isHidden": graph_hidden},
            "lastMessagePreview": last_message.map(|at| json!({"id": "1", "createdDateTime": at})),
        }))
        .unwrap()
    }

    fn states(hidden: Option<bool>) -> HashMap<String, ConversationState> {
        HashMap::from([(
            "19:a".to_owned(),
            ConversationState {
                id: "19:a".to_owned(),
                muted: false,
                hidden,
                read: chatsvc::ReadHorizon::default(),
                history_cleared_ms: None,
            },
        )])
    }

    #[test]
    fn removed_history_ends_one_millisecond_after_the_last_message() {
        let last = at("2026-10-01T10:00:00Z");
        assert_eq!(
            cleared_until_ms(Some(last), now()),
            last.timestamp_millis() + 1
        );
        assert_eq!(cleared_until_ms(None, now()), now().timestamp_millis());
    }

    #[test]
    fn removed_history_hides_the_chat_until_a_newer_message() {
        let mut locally = LocallyHidden::default();
        let cleared_ms = at("2026-10-01T10:00:00Z").timestamp_millis() + 1;
        let mut cleared = states(Some(false));
        cleared.get_mut("19:a").unwrap().history_cleared_ms = Some(cleared_ms);
        assert!(resolve_hidden(
            &chat(Some("2026-10-01T10:00:00Z"), false),
            &cleared,
            &mut locally,
            now()
        ));
        assert!(resolve_hidden(
            &chat(None, false),
            &cleared,
            &mut locally,
            now()
        ));
        assert!(!resolve_hidden(
            &chat(Some("2026-10-02T08:00:00Z"), false),
            &cleared,
            &mut locally,
            now()
        ));
    }

    fn hidden_after(text: &str) -> LocallyHidden {
        let mut locally = LocallyHidden::default();
        locally.hide("19:a", at(text));
        locally
    }

    fn now() -> DateTime<Utc> {
        at("2030-01-01T12:00:00Z")
    }

    #[test]
    fn chatsvc_visible_beats_the_graph_hidden_flag() {
        let mut locally = LocallyHidden::default();
        let chat = chat(None, true);
        assert!(!resolve_hidden(
            &chat,
            &states(Some(false)),
            &mut locally,
            now()
        ));
        assert!(resolve_hidden(&chat, &HashMap::new(), &mut locally, now()));
    }

    #[test]
    fn a_local_hide_survives_a_chatsvc_that_still_says_visible() {
        let mut locally = hidden_after("2026-10-10T10:00:00Z");
        let chat = chat(Some("2026-10-10T09:00:00Z"), false);
        assert!(resolve_hidden(
            &chat,
            &states(Some(false)),
            &mut locally,
            now()
        ));
        assert_eq!(locally, hidden_after("2026-10-10T10:00:00Z"));
    }

    #[test]
    fn only_a_strictly_newer_message_unhides() {
        let mut locally = hidden_after("2026-10-10T10:00:00Z");
        let same = chat(Some("2026-10-10T10:00:00Z"), false);
        assert!(resolve_hidden(
            &same,
            &states(Some(false)),
            &mut locally,
            now()
        ));
        let newer = chat(Some("2026-10-10T10:00:01Z"), false);
        assert!(!resolve_hidden(
            &newer,
            &states(Some(false)),
            &mut locally,
            now()
        ));
        assert_eq!(locally, LocallyHidden::default());
    }

    #[test]
    fn the_hide_marker_is_the_servers_message_time_so_clock_skew_does_not_matter() {
        let mut locally = hidden_after("2026-10-10T10:00:00Z");
        let chat = chat(Some("2026-10-10T10:00:30Z"), false);
        let skewed_now = at("2026-10-10T09:00:00Z");
        assert!(!resolve_hidden(
            &chat,
            &states(Some(false)),
            &mut locally,
            skewed_now
        ));
    }

    #[test]
    fn chatsvc_reporting_hidden_takes_over_and_drops_the_local_entry() {
        let mut locally = hidden_after("2026-10-10T10:00:00Z");
        let chat = chat(Some("2026-10-10T11:00:00Z"), false);
        assert!(resolve_hidden(
            &chat,
            &states(Some(true)),
            &mut locally,
            now()
        ));
        assert_eq!(locally, LocallyHidden::default());
    }

    #[test]
    fn a_local_unhide_overrides_a_reported_hide_until_chatsvc_catches_up_or_time_runs_out() {
        let chat = chat(None, false);
        let mut locally = LocallyHidden::default();
        locally.unhide("19:a", now());
        assert!(!resolve_hidden(
            &chat,
            &states(Some(true)),
            &mut locally,
            now()
        ));
        assert!(!resolve_hidden(
            &chat,
            &states(Some(true)),
            &mut locally,
            now() + Duration::minutes(9)
        ));
        assert!(resolve_hidden(
            &chat,
            &states(Some(true)),
            &mut locally,
            now() + Duration::minutes(11)
        ));
        assert_eq!(locally, LocallyHidden::default());

        locally.unhide("19:a", now());
        assert!(!resolve_hidden(
            &chat,
            &states(Some(false)),
            &mut locally,
            now()
        ));
        assert_eq!(locally, LocallyHidden::default());
    }

    #[test]
    fn a_reported_hide_for_any_chat_drops_its_local_entry_unless_an_unhide_is_fresh() {
        let mut locally = hidden_after("2026-10-10T10:00:00Z");
        assert!(locally.reported_hidden("19:a", now()));
        assert_eq!(locally, LocallyHidden::default());

        locally.unhide("19:a", now());
        assert!(!locally.reported_hidden("19:a", now()));
        assert!(locally.reported_hidden("19:a", now() + Duration::minutes(11)));
    }

    #[test]
    fn entries_round_trip_through_the_store() {
        let store = Store::open_in_memory().unwrap();
        let mut locally = hidden_after("2026-10-10T10:00:00Z");
        locally.unhide("19:b", at("2026-10-10T11:00:00Z"));
        locally.save(&store).unwrap();
        assert_eq!(LocallyHidden::load(&store), locally);
    }
}
