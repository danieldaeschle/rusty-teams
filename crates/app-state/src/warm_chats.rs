use std::collections::HashSet;

use crate::app_state::AppState;
use crate::runtime;

const RECENT_CHAT_COUNT: usize = 15;

#[derive(Default)]
pub struct WarmChats {
    requested: HashSet<String>,
}

impl WarmChats {
    fn take_new(&mut self, chat_ids: Vec<String>) -> Vec<String> {
        chat_ids
            .into_iter()
            .filter(|chat_id| self.requested.insert(chat_id.clone()))
            .collect()
    }
}

pub fn recent_chat_ids(chats: &[store::ChatRecord], count: usize) -> Vec<String> {
    let mut recent: Vec<_> = chats.iter().collect();
    recent.sort_by(|left, right| right.last_message_at.cmp(&left.last_message_at));
    recent
        .into_iter()
        .take(count)
        .map(|chat| chat.id.clone())
        .collect()
}

impl AppState {
    pub(crate) fn warm_recent_chats(&mut self) {
        let chat_ids = recent_chat_ids(&self.sidebar.chats, RECENT_CHAT_COUNT);
        self.warm(chat_ids);
    }

    pub fn warm_chat(&mut self, chat_id: &str) {
        self.warm(vec![chat_id.to_owned()]);
    }

    fn warm(&mut self, chat_ids: Vec<String>) {
        let Some(engine) = self.engine.clone().filter(|_| !self.mode.demo) else {
            return;
        };
        let chat_ids = self.warm_chats.take_new(chat_ids);
        if chat_ids.is_empty() {
            return;
        }
        let store = self.store.clone();
        runtime::handle().spawn(async move {
            for chat_id in chat_ids {
                let cached = store
                    .messages(&chat_id, None, 1)
                    .is_ok_and(|messages| !messages.is_empty());
                if !cached {
                    let _ = engine.fetch_newer(&chat_id).await;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use store::ChatRecord;

    use super::*;

    fn chat(id: &str, hour: Option<u32>) -> ChatRecord {
        ChatRecord {
            id: id.to_owned(),
            last_message_at: hour.map(|hour| Utc.with_ymd_and_hms(2026, 11, 11, hour, 0, 0).unwrap()),
            ..Default::default()
        }
    }

    #[test]
    fn the_newest_chats_are_warmed_first() {
        let chats = [chat("old", Some(8)), chat("never", None), chat("new", Some(12)), chat("mid", Some(10))];
        assert_eq!(recent_chat_ids(&chats, 2), ["new", "mid"]);
    }

    #[test]
    fn a_chat_is_warmed_only_once() {
        let mut warm = WarmChats::default();
        assert_eq!(warm.take_new(vec!["a".into(), "b".into()]), ["a", "b"]);
        assert_eq!(warm.take_new(vec!["b".into(), "c".into()]), ["c"]);
    }
}
