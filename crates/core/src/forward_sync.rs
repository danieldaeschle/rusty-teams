use std::time::Duration;

use store::MessageRecord;

use crate::engine::{Conversation, SyncEngine};
use crate::error::Error;
use crate::forwarded::{FORWARDED_CONTENT_TYPE, lacks_forward_content, mark_forwards_unavailable};
use crate::mapping::message_record;
use crate::remote::Remote;

/// Graph answered for good: the original is gone or not readable for this account.
fn is_gone(error: &Error) -> bool {
    matches!(
        error,
        Error::Graph(graph::Error::Session(session::Error::Api {
            status: 403 | 404 | 410,
            ..
        }))
    )
}

impl<R: Remote> SyncEngine<R> {
    /// Once per conversation and session (again after a failed Graph read), for cached forwards stored before their original message was kept.
    pub(crate) async fn heal_forwards(&self, conversation_id: &str) -> bool {
        let first = self
            .forwards_healed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(conversation_id.to_owned());
        if !first {
            return false;
        }
        let in_chat = match self.resolve_stored(conversation_id) {
            Ok(Conversation::Chat) => true,
            Ok(Conversation::Channel { .. }) => false,
            Err(_) => return false,
        };
        let Ok(cached) = self
            .store
            .messages_with_attachment_type(conversation_id, FORWARDED_CONTENT_TYPE)
        else {
            return false;
        };
        let stale_ids: Vec<String> = cached
            .into_iter()
            .filter(lacks_forward_content)
            .map(|record| record.message_id)
            .collect();
        if stale_ids.is_empty() {
            return false;
        }
        if !in_chat {
            let given_up = self.given_up_forwards(conversation_id, &stale_ids);
            return self
                .ingest(conversation_id, given_up)
                .is_ok_and(|delta| !delta.is_empty());
        }
        let Ok(answers) = self
            .remote
            .chat_messages_by_id(conversation_id, &stale_ids)
            .await
        else {
            self.retry_forward_heal(conversation_id);
            return false;
        };
        let mut healed = Vec::new();
        let mut given_up_ids = Vec::new();
        let mut retry = false;
        for (message_id, answer) in stale_ids.iter().zip(answers) {
            let fresh = match answer {
                Ok(message) => message_record(conversation_id, &message),
                Err(error) if is_gone(&error) => None,
                Err(_) => {
                    retry = true;
                    continue;
                }
            };
            match fresh {
                Some(record) if !lacks_forward_content(&record) => healed.push(record),
                _ => given_up_ids.push(message_id.clone()),
            }
        }
        if retry {
            self.retry_forward_heal(conversation_id);
        }
        self.apply_chat_emotions(conversation_id, &mut healed).await;
        healed.extend(self.given_up_forwards(conversation_id, &given_up_ids));
        self.ingest(conversation_id, healed)
            .is_ok_and(|delta| !delta.is_empty())
    }

    fn retry_forward_heal(&self, conversation_id: &str) {
        self.forwards_healed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(conversation_id);
    }

    /// Read again, as the delta fetch running alongside may have stored a newer version meanwhile.
    fn given_up_forwards(
        &self,
        conversation_id: &str,
        message_ids: &[String],
    ) -> Vec<MessageRecord> {
        if message_ids.is_empty() {
            return Vec::new();
        }
        self.store
            .messages_by_id(conversation_id, message_ids)
            .map(|current| {
                current
                    .values()
                    .filter_map(mark_forwards_unavailable)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Heals every chat that still holds forwards without their original, newest chat first, one Graph batch per chat.
    pub async fn heal_all_forwards(&self, pause: Duration) {
        let Ok(conversation_ids) = self
            .store
            .conversations_with_stale_attachments(FORWARDED_CONTENT_TYPE)
        else {
            return;
        };
        for conversation_id in conversation_ids {
            if self.heal_forwards(&conversation_id).await {
                self.announce(&conversation_id, true);
            }
            tokio::time::sleep(pause).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_definite_graph_answer_gives_a_forward_up() {
        let api = |status| {
            Error::Graph(graph::Error::Session(session::Error::api(
                status,
                "https://graph.microsoft.com/v1.0/chats/c/messages/m",
                serde_json::json!({}),
            )))
        };
        assert!(is_gone(&api(404)));
        assert!(is_gone(&api(403)));
        assert!(!is_gone(&api(429)));
        assert!(!is_gone(&api(503)));
        assert!(!is_gone(&Error::Unsupported("batched message reads")));
    }
}
