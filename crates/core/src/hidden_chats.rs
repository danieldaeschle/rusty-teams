use std::collections::HashMap;

use chatsvc::ConversationState;
use chrono::{DateTime, TimeZone, Utc};
use graph::Chat;
use store::Store;

use crate::error::Result;
use crate::mapping::graph_hidden;

const META_LOCALLY_HIDDEN: &str = "locally_hidden_chats";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LocallyHidden {
    hidden_at: HashMap<String, DateTime<Utc>>,
}

impl LocallyHidden {
    pub(crate) fn load(store: &Store) -> Self {
        let text = store
            .meta(META_LOCALLY_HIDDEN)
            .ok()
            .flatten()
            .unwrap_or_default();
        let hidden_at = text
            .lines()
            .filter_map(|line| {
                let (chat_id, millis) = line.rsplit_once('\t')?;
                let at = Utc.timestamp_millis_opt(millis.parse().ok()?).single()?;
                Some((chat_id.to_owned(), at))
            })
            .collect();
        LocallyHidden { hidden_at }
    }

    pub(crate) fn save(&self, store: &Store) -> Result<()> {
        let mut lines: Vec<String> = self
            .hidden_at
            .iter()
            .map(|(chat_id, at)| format!("{chat_id}\t{}", at.timestamp_millis()))
            .collect();
        lines.sort_unstable();
        store.set_meta(META_LOCALLY_HIDDEN, &lines.join("\n"))?;
        Ok(())
    }

    pub(crate) fn insert(&mut self, chat_id: &str, at: DateTime<Utc>) {
        self.hidden_at.insert(chat_id.to_owned(), at);
    }

    pub(crate) fn remove(&mut self, chat_id: &str) -> bool {
        self.hidden_at.remove(chat_id).is_some()
    }
}

pub(crate) fn resolve_hidden(
    chat: &Chat,
    states: &HashMap<String, ConversationState>,
    locally: &mut LocallyHidden,
) -> bool {
    let reported = states.get(&chat.id).and_then(|state| state.hidden);
    if reported == Some(true) {
        locally.remove(&chat.id);
        return true;
    }
    if let Some(hidden_at) = locally.hidden_at.get(&chat.id).copied() {
        if chat
            .last_message_time()
            .is_some_and(|newest| newest > hidden_at)
        {
            locally.remove(&chat.id);
            return false;
        }
        return true;
    }
    reported.unwrap_or_else(|| graph_hidden(chat))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

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
            },
        )])
    }

    fn hidden_at(at: &str) -> LocallyHidden {
        let mut locally = LocallyHidden::default();
        locally.insert("19:a", at.parse().unwrap());
        locally
    }

    #[test]
    fn chatsvc_visible_beats_the_graph_hidden_flag() {
        let mut locally = LocallyHidden::default();
        let chat = chat(None, true);
        assert!(!resolve_hidden(&chat, &states(Some(false)), &mut locally));
        assert!(resolve_hidden(&chat, &HashMap::new(), &mut locally));
    }

    #[test]
    fn a_local_hide_survives_a_chatsvc_that_still_says_visible() {
        let mut locally = hidden_at("2026-10-10T10:00:00Z");
        let chat = chat(Some("2026-10-10T09:00:00Z"), false);
        assert!(resolve_hidden(&chat, &states(Some(false)), &mut locally));
        assert_eq!(locally, hidden_at("2026-10-10T10:00:00Z"));
    }

    #[test]
    fn a_newer_message_unhides_and_drops_the_local_entry() {
        let mut locally = hidden_at("2026-10-10T10:00:00Z");
        let chat = chat(Some("2026-10-10T11:00:00Z"), false);
        assert!(!resolve_hidden(&chat, &states(Some(false)), &mut locally));
        assert_eq!(locally, LocallyHidden::default());
    }

    #[test]
    fn chatsvc_reporting_hidden_takes_over_and_drops_the_local_entry() {
        let mut locally = hidden_at("2026-10-10T10:00:00Z");
        let chat = chat(Some("2026-10-10T11:00:00Z"), false);
        assert!(resolve_hidden(&chat, &states(Some(true)), &mut locally));
        assert_eq!(locally, LocallyHidden::default());
    }

    #[test]
    fn local_hides_round_trip_through_the_store() {
        let store = Store::open_in_memory().unwrap();
        let locally = hidden_at("2026-10-10T10:00:00Z");
        locally.save(&store).unwrap();
        assert_eq!(LocallyHidden::load(&store), locally);
    }
}
