use std::collections::HashMap;

use chatsvc::{ConversationRef, EmotionReaction, MessageEmotions};
use chrono::{DateTime, Utc};
use store::MessageRecord;

use crate::engine::{Conversation, SyncEngine};
use crate::remote::Remote;
use crate::stored::{ReactionInfo, reactions};

const PAGE_MARGIN: usize = 20;
const MAX_PAGE_SIZE: usize = 200;
const MAX_GROUPS: usize = 10;

impl<R: Remote> SyncEngine<R> {
    /// Graph reports reactions without their skin tone; Teams reads the chat service
    /// `emotions`, so the records take theirs from there. Failures keep the Graph reactions.
    pub(crate) async fn apply_chat_emotions(
        &self,
        conversation_id: &str,
        records: &mut [MessageRecord],
    ) {
        let Ok(conversation) = self.resolve_stored(conversation_id) else {
            return;
        };
        let groups = emotion_groups(conversation_id, &conversation, records);
        for (target, members) in groups.into_iter().take(MAX_GROUPS) {
            let Some(newest) = members
                .iter()
                .map(|index| &records[*index])
                .max_by_key(|record| record.created_at)
                .map(|record| record.message_id.clone())
            else {
                continue;
            };
            let page_size = (members.len() + PAGE_MARGIN).min(MAX_PAGE_SIZE);
            let Ok(found) = self
                .remote
                .message_emotions(&target, &newest, page_size)
                .await
            else {
                continue;
            };
            for MessageEmotions {
                message_id,
                reactions: found_reactions,
            } in found
            {
                if let Some(index) = members
                    .iter()
                    .find(|index| records[**index].message_id == message_id)
                {
                    let record = &mut records[*index];
                    record.reactions_json = reactions_json(record, &found_reactions);
                }
            }
        }
    }
}

fn emotion_groups(
    conversation_id: &str,
    conversation: &Conversation,
    records: &[MessageRecord],
) -> Vec<(ConversationRef, Vec<usize>)> {
    let live = (0..records.len()).filter(|index| !records[*index].deleted);
    match conversation {
        Conversation::Chat => vec![(ConversationRef::chat(conversation_id), live.collect())],
        Conversation::Channel { .. } => {
            let mut groups: Vec<(ConversationRef, Vec<usize>)> = Vec::new();
            let mut positions: HashMap<Option<String>, usize> = HashMap::new();
            for index in live {
                let root = records[index].reply_to_id.clone();
                let position = *positions.entry(root.clone()).or_insert_with(|| {
                    let target = match &root {
                        Some(root_id) => ConversationRef::channel_reply(conversation_id, root_id),
                        None => ConversationRef::channel_root(conversation_id),
                    };
                    groups.push((target, Vec::new()));
                    groups.len() - 1
                });
                groups[position].1.push(index);
            }
            groups
        }
    }
}

fn reactions_json(record: &MessageRecord, found: &[EmotionReaction]) -> String {
    let names: HashMap<Option<String>, Option<String>> = reactions(record)
        .into_iter()
        .map(|reaction| (reaction.user_id, reaction.user_name))
        .collect();
    let converted: Vec<ReactionInfo> = found
        .iter()
        .map(|reaction| ReactionInfo {
            reaction_type: chatsvc::reaction_glyph_for_key(&reaction.key)
                .unwrap_or_else(|| reaction.key.clone()),
            user_id: reaction.user_id.clone(),
            user_name: names.get(&reaction.user_id).cloned().flatten(),
            created_at: reaction
                .time_ms
                .and_then(DateTime::<Utc>::from_timestamp_millis),
        })
        .collect();
    serde_json::to_string(&converted).unwrap_or_else(|_| record.reactions_json.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, reply_to: Option<&str>) -> MessageRecord {
        MessageRecord {
            conversation_id: "19:c".into(),
            message_id: id.into(),
            reply_to_id: reply_to.map(str::to_owned),
            reactions_json:
                r#"[{"reaction_type":"👍","user_id":"aaa","user_name":"Lea","created_at":null}]"#
                    .into(),
            ..MessageRecord::default()
        }
    }

    #[test]
    fn chat_service_keys_replace_graph_reactions_and_keep_names() {
        let found = [EmotionReaction {
            key: "yes-tone2".into(),
            user_id: Some("aaa".into()),
            time_ms: Some(1_792_135_855_841),
        }];
        let json = reactions_json(&record("1", None), &found);
        let parsed: Vec<ReactionInfo> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0].reaction_type, "👍🏼");
        assert_eq!(parsed[0].user_name.as_deref(), Some("Lea"));
        assert!(parsed[0].created_at.is_some());
        assert_eq!(reactions_json(&record("1", None), &[]), "[]");
    }

    #[test]
    fn channel_messages_group_by_thread() {
        let records = [
            record("r1", None),
            record("a", Some("r1")),
            record("b", Some("r1")),
        ];
        let groups = emotion_groups(
            "19:c",
            &Conversation::Channel {
                team_id: "t".into(),
            },
            &records,
        );
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, ConversationRef::channel_root("19:c"));
        assert_eq!(groups[1].0, ConversationRef::channel_reply("19:c", "r1"));
        assert_eq!(groups[1].1, vec![1, 2]);
    }
}
