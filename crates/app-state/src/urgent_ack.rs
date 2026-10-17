use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use gpui_kit::Context;
use store::{MessageImportance, MessageRecord};

use crate::app_state::AppState;
use crate::notify::urgent_repeat::{REPEAT_INTERVAL_MINUTES, REPEAT_WINDOW_MINUTES};
use crate::runtime;

const RECENT_MESSAGES: usize = 30;

/// Teams acknowledges an urgent message from someone else once it is shown; after the repeat window there is nothing left to stop.
pub fn urgent_to_ack(
    records: &[MessageRecord],
    my_user_id: &str,
    now: DateTime<Utc>,
    acked: &HashSet<String>,
) -> Vec<String> {
    let oldest = now - Duration::minutes(REPEAT_WINDOW_MINUTES + REPEAT_INTERVAL_MINUTES);
    records
        .iter()
        .filter(|record| {
            record.importance == MessageImportance::Urgent
                && !record.deleted
                && record.created_at >= oldest
                && record
                    .sender_id
                    .as_deref()
                    .is_some_and(|sender| sender != my_user_id)
                && !acked.contains(&record.message_id)
        })
        .map(|record| record.message_id.clone())
        .collect()
}

impl AppState {
    pub fn ack_urgent_messages(&mut self, chat_id: &str, _cx: &mut Context<Self>) {
        if self.mode.demo || self.mode.read_only {
            return;
        }
        let (Some(engine), Some(me)) = (self.engine.clone(), self.directory.me.as_ref()) else {
            return;
        };
        let Ok(records) = self.store.messages(chat_id, None, RECENT_MESSAGES) else {
            return;
        };
        let message_ids = urgent_to_ack(&records, &me.user_id, Utc::now(), &self.acked_urgent);
        if message_ids.is_empty() {
            return;
        }
        self.acked_urgent.extend(message_ids.iter().cloned());
        let chat_id = chat_id.to_owned();
        drop(runtime::spawn(async move {
            for message_id in message_ids {
                let _ = engine.ack_urgent_message(&chat_id, &message_id).await;
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 17, 9, 30, 0).unwrap()
    }

    fn record(
        id: &str,
        sender: &str,
        minutes_ago: i64,
        importance: MessageImportance,
    ) -> MessageRecord {
        MessageRecord {
            conversation_id: "chat".into(),
            message_id: id.into(),
            sender_id: Some(sender.into()),
            created_at: now() - Duration::minutes(minutes_ago),
            importance,
            ..Default::default()
        }
    }

    #[test]
    fn acks_recent_urgent_messages_from_others_once() {
        let records = [
            record("urgent", "lea", 3, MessageImportance::Urgent),
            record("own", "me", 3, MessageImportance::Urgent),
            record("important", "lea", 3, MessageImportance::High),
            record("old", "lea", 40, MessageImportance::Urgent),
            record("done", "lea", 1, MessageImportance::Urgent),
        ];
        let acked = HashSet::from(["done".to_owned()]);
        assert_eq!(urgent_to_ack(&records, "me", now(), &acked), ["urgent"]);
    }
}
