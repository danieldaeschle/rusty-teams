use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use chatsvc::{EventKind, MemberHorizon, MessageEvent, StatusEvent, StatusKind};
use chrono::{DateTime, Utc};
use store::MessageRecord;

use crate::engine::{Conversation, SyncEngine};
use crate::error::Result;
use crate::events::CoreEvent;
use crate::remote::Remote;

pub const DEFAULT_RECEIPT_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptReader {
    pub name: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptState {
    Sent,
    Read {
        readers: Vec<ReceiptReader>,
        total_others: usize,
    },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OtherMember {
    name: String,
    read_until: DateTime<Utc>,
    read_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConversationReceipts {
    my_user_id: String,
    others: Vec<OtherMember>,
    total_others: usize,
}

pub(crate) struct ReceiptCache {
    conversations: Mutex<HashMap<String, Option<ConversationReceipts>>>,
    pending: Mutex<HashSet<String>>,
    pub(crate) debounce: Duration,
}

impl Default for ReceiptCache {
    fn default() -> Self {
        ReceiptCache {
            conversations: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashSet::new()),
            debounce: DEFAULT_RECEIPT_DEBOUNCE,
        }
    }
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn same_user(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn user_of(member_id: &str) -> &str {
    member_id.rsplit(':').next().unwrap_or(member_id)
}

fn build(
    my_user_id: &str,
    horizons: &[MemberHorizon],
    members: &[store::MemberRecord],
) -> Option<ConversationReceipts> {
    let mut others: Vec<OtherMember> = horizons
        .iter()
        .filter(|horizon| !same_user(user_of(&horizon.member_id), my_user_id))
        .map(|horizon| {
            let user_id = user_of(&horizon.member_id);
            let name = members
                .iter()
                .find(|member| {
                    member
                        .user_id
                        .as_deref()
                        .is_some_and(|known| same_user(known, user_id))
                })
                .map(|member| member.display_name.clone())
                .unwrap_or_default();
            OtherMember {
                name,
                read_until: horizon.read_until,
                read_at: horizon.read_at.unwrap_or(horizon.read_until),
            }
        })
        .collect();
    if others.is_empty() {
        return None;
    }
    others.sort_by(|left, right| left.name.cmp(&right.name));
    let known_others = members
        .iter()
        .filter(|member| {
            member
                .user_id
                .as_deref()
                .is_none_or(|known| !same_user(known, my_user_id))
        })
        .count();
    Some(ConversationReceipts {
        my_user_id: my_user_id.to_owned(),
        total_others: known_others.max(others.len()),
        others,
    })
}

fn state_of(receipts: &ConversationReceipts, record: &MessageRecord) -> ReceiptState {
    let own = record
        .sender_id
        .as_deref()
        .is_some_and(|sender| same_user(sender, &receipts.my_user_id));
    if !own || record.deleted {
        return ReceiptState::Unknown;
    }
    let mut readers: Vec<ReceiptReader> = receipts
        .others
        .iter()
        .filter(|member| member.read_until >= record.created_at)
        .map(|member| ReceiptReader {
            name: member.name.clone(),
            at: member.read_at,
        })
        .collect();
    if readers.is_empty() {
        return ReceiptState::Sent;
    }
    readers.sort_by(|left, right| {
        left.at
            .cmp(&right.at)
            .then_with(|| left.name.cmp(&right.name))
    });
    ReceiptState::Read {
        readers,
        total_others: receipts.total_others,
    }
}

impl<R: Remote> SyncEngine<R> {
    pub fn with_receipt_debounce(mut self, debounce: Duration) -> Self {
        self.receipts.debounce = debounce;
        self
    }

    /// Own messages in chats only. Unknown for others' messages, channels, deleted messages and disabled receipts.
    pub fn receipt_state(&self, conversation_id: &str, message_id: &str) -> ReceiptState {
        let record = self
            .store
            .messages_by_id(conversation_id, &[message_id.to_owned()])
            .ok()
            .and_then(|mut found| found.remove(message_id));
        record.map_or(ReceiptState::Unknown, |record| {
            self.receipt_state_for(&record)
        })
    }

    /// Memory only, cheap enough to call per rendered message.
    pub fn receipt_state_for(&self, record: &MessageRecord) -> ReceiptState {
        let conversations = locked(&self.receipts.conversations);
        match conversations.get(&record.conversation_id) {
            Some(Some(receipts)) => state_of(receipts, record),
            _ => ReceiptState::Unknown,
        }
    }

    /// Call when a chat opens. Channels are skipped.
    pub async fn refresh_receipts(&self, conversation_id: &str) -> Result<()> {
        if matches!(self.resolve(conversation_id)?, Conversation::Channel { .. }) {
            return Ok(());
        }
        let my_user_id = self.my_user_id().await?;
        let horizons = self.remote.consumption_horizons(conversation_id).await?;
        let members = self
            .store
            .chat(conversation_id)?
            .map(|chat| chat.members)
            .unwrap_or_default();
        let fresh = build(&my_user_id, &horizons, &members);
        let changed = locked(&self.receipts.conversations)
            .insert(conversation_id.to_owned(), fresh.clone())
            .is_none_or(|previous| previous != fresh);
        if changed {
            let _ = self.events.send(CoreEvent::ReceiptsChanged {
                conversation_id: conversation_id.to_owned(),
            });
        }
        Ok(())
    }

    /// Calls within the debounce window return at once, the first call does the single refresh.
    pub async fn refresh_receipts_debounced(&self, conversation_id: &str) -> Result<()> {
        if !locked(&self.receipts.pending).insert(conversation_id.to_owned()) {
            return Ok(());
        }
        tokio::time::sleep(self.receipts.debounce).await;
        locked(&self.receipts.pending).remove(conversation_id);
        self.refresh_receipts(conversation_id).await
    }

    /// Only conversations opened before are refreshed. Opening a chat fetches anyway.
    pub async fn handle_receipt_event(&self, event: &MessageEvent) {
        if event.kind != EventKind::ReadReceipt {
            return;
        }
        let Some(conversation_id) = event.conversation_id.as_deref() else {
            return;
        };
        if locked(&self.receipts.conversations).contains_key(conversation_id) {
            let _ = self.refresh_receipts_debounced(conversation_id).await;
        }
    }

    /// Refetches every known conversation after a reconnect or lost messages.
    pub async fn handle_receipt_status(&self, status: &StatusEvent) {
        if !matches!(status.kind, StatusKind::Connected | StatusKind::MessageLoss) {
            return;
        }
        let known: Vec<String> = locked(&self.receipts.conversations)
            .keys()
            .cloned()
            .collect();
        for conversation_id in known {
            let _ = self.refresh_receipts(&conversation_id).await;
        }
    }
}
