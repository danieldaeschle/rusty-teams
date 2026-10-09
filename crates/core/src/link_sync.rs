use std::collections::HashSet;

use chatsvc::{ConversationRef, MessageLinks};
use store::MessageRecord;

use crate::engine::{Conversation, Delta, SyncEngine};
use crate::error::Result;
use crate::links::{LinkPreview, has_links_markup, has_stored_links, is_public_link};
use crate::remote::Remote;

const LINKS_PAGE_SIZE: usize = 50;
const MAX_REPLY_THREADS: usize = 5;

impl<R: Remote> SyncEngine<R> {
    /// `None` when the page has no title and no image, or the link is not public.
    pub async fn link_preview_for(&self, url: &str) -> Result<Option<LinkPreview>> {
        if !is_public_link(url) {
            return Ok(None);
        }
        let cached = self
            .link_previews
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(url)
            .cloned();
        if let Some(preview) = cached {
            return Ok(preview);
        }
        let info = self.remote.link_info(url).await?;
        let preview = LinkPreview::from_info(url, &info);
        self.link_previews
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(url.to_owned(), preview.clone());
        Ok(preview)
    }

    /// Network failures leave the records as they are.
    pub(crate) async fn attach_delta_links(
        &self,
        conversation_id: &str,
        delta: &mut Delta,
    ) -> bool {
        let mut records: Vec<&mut MessageRecord> = delta
            .added
            .iter_mut()
            .chain(delta.updated.iter_mut())
            .collect();
        self.attach_links(conversation_id, &mut records).await
    }

    /// Once per conversation and session, for cached messages stored before their links were known.
    pub(crate) async fn backfill_links(&self, conversation_id: &str) -> bool {
        let first = self
            .links_backfilled
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(conversation_id.to_owned());
        if !first {
            return false;
        }
        let Ok(mut cached) = self.store.messages(conversation_id, None, LINKS_PAGE_SIZE) else {
            return false;
        };
        let mut records: Vec<&mut MessageRecord> = cached
            .iter_mut()
            .filter(|record| !has_stored_links(&record.links_json))
            .collect();
        self.attach_links(conversation_id, &mut records).await
    }

    pub(crate) async fn attach_links(
        &self,
        conversation_id: &str,
        records: &mut [&mut MessageRecord],
    ) -> bool {
        let Ok(conversation) = self.resolve(conversation_id) else {
            return false;
        };
        let targets = link_conversations(conversation_id, &conversation, records);
        let mut changed = false;
        for target in targets {
            let Ok(found) = self.remote.message_links(&target, LINKS_PAGE_SIZE).await else {
                continue;
            };
            for MessageLinks {
                message_id,
                links_json,
            } in found
            {
                if !has_stored_links(&links_json) {
                    continue;
                }
                let Some(record) = records.iter_mut().find(|record| {
                    record.message_id == message_id
                        && has_links_markup(record)
                        && record.links_json != links_json
                }) else {
                    continue;
                };
                record.links_json = links_json;
                changed |= self
                    .store
                    .set_message_links(conversation_id, &message_id, &record.links_json)
                    .unwrap_or(false);
            }
        }
        changed
    }
}

fn link_conversations(
    conversation_id: &str,
    conversation: &Conversation,
    records: &[&mut MessageRecord],
) -> Vec<ConversationRef> {
    let candidates: Vec<&MessageRecord> = records
        .iter()
        .map(|record| &**record)
        .filter(|record| has_links_markup(record))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    match conversation {
        Conversation::Chat => vec![ConversationRef::chat(conversation_id)],
        Conversation::Channel { .. } => {
            let mut targets = Vec::new();
            if candidates.iter().any(|record| record.reply_to_id.is_none()) {
                targets.push(ConversationRef::channel_root(conversation_id));
            }
            let mut seen = HashSet::new();
            let roots = candidates
                .iter()
                .filter_map(|record| record.reply_to_id.as_deref())
                .filter(|root_id| seen.insert(*root_id))
                .take(MAX_REPLY_THREADS);
            targets.extend(
                roots.map(|root_id| ConversationRef::channel_reply(conversation_id, root_id)),
            );
            targets
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(message_id: &str, reply_to_id: Option<&str>, body_html: &str) -> MessageRecord {
        MessageRecord {
            message_id: message_id.to_owned(),
            reply_to_id: reply_to_id.map(str::to_owned),
            body_html: body_html.to_owned(),
            ..MessageRecord::default()
        }
    }

    fn targets(
        conversation: &Conversation,
        mut records: Vec<MessageRecord>,
    ) -> Vec<ConversationRef> {
        let refs: Vec<&mut MessageRecord> = records.iter_mut().collect();
        link_conversations("19:c", conversation, &refs)
    }

    #[test]
    fn messages_without_anchors_need_no_request() {
        assert!(targets(&Conversation::Chat, vec![record("1", None, "<p>hi</p>")]).is_empty());
    }

    #[test]
    fn a_chat_is_read_once() {
        let found = targets(
            &Conversation::Chat,
            vec![
                record("1", None, "<a href=\"https://a.example\">a</a>"),
                record("2", None, "<a href=\"https://b.example\">b</a>"),
            ],
        );
        assert_eq!(found, vec![ConversationRef::chat("19:c")]);
    }

    #[test]
    fn channel_replies_are_read_per_thread_with_a_cap() {
        let anchor = "<a href=\"https://a.example\">a</a>";
        let mut records = vec![record("1", None, anchor)];
        records.extend(
            (0..8).map(|index| record(&format!("r{index}"), Some(&format!("root{index}")), anchor)),
        );
        let found = targets(
            &Conversation::Channel {
                team_id: "t".into(),
            },
            records,
        );
        assert_eq!(found.len(), 1 + MAX_REPLY_THREADS);
        assert_eq!(found[0], ConversationRef::channel_root("19:c"));
        assert_eq!(found[1], ConversationRef::channel_reply("19:c", "root0"));
    }
}
