use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, Utc};
use gpui_kit::*;
use mcp::{AgentHub, AgentPolicy, Proposal, ReactionOutcome, ReactionProposal, UiEvent};
use store::{MessageRecord, Store};
use teams_core::CoreEvent;
use tokio::sync::mpsc::UnboundedReceiver;

use super::body::body_blocks;
use super::extras::message_extras;
use super::image_scale::downscale;
use super::source::AppSource;
use super::{AgentApi, ProposalEntry, ProposalPicture, ProposalStatus, Thumbnail};
use crate::backend::Engine;
use crate::rows::reaction_glyph;
use crate::runtime;

const PREVIEW_CHARACTERS: usize = 200;
const NEW_MESSAGE_BATCH: usize = 50;
const CLOCK_SKEW_ALLOWANCE: chrono::TimeDelta = chrono::TimeDelta::minutes(2);
const REACTION_WATCH_WINDOW: usize = 50;
const THUMBNAIL_EDGE: u32 = 96;
const SEED_RETRY: std::time::Duration = std::time::Duration::from_secs(5);

pub fn drain_ui_events(
    mut receiver: UnboundedReceiver<UiEvent>,
    api: WeakEntity<AgentApi>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        while let Some(event) = receiver.recv().await {
            let pictures = match &event {
                UiEvent::ProposalChanged(proposal) | UiEvent::ProposalStale(proposal)
                    if !proposal.images.is_empty() =>
                {
                    let proposal = proposal.clone();
                    cx.background_executor()
                        .spawn(async move { build_pictures(&proposal) })
                        .await
                }
                _ => Vec::new(),
            };
            if api
                .update(cx, |api, cx| api.apply_ui_event(event, pictures, cx))
                .is_err()
            {
                return;
            }
        }
    })
    .detach();
}

pub fn build_pictures(proposal: &Proposal) -> Vec<ProposalPicture> {
    proposal
        .images
        .iter()
        .map(|image| ProposalPicture {
            name: image.name.clone(),
            bytes: image.bytes.clone(),
            thumbnail: downscale(&image.bytes, THUMBNAIL_EDGE).ok().map(|scaled| {
                let format = if scaled.mime_type == "image/png" {
                    ImageFormat::Png
                } else {
                    ImageFormat::Jpeg
                };
                Thumbnail {
                    image: Arc::new(Image::from_bytes(format, scaled.bytes)),
                    dimensions: (scaled.width, scaled.height),
                }
            }),
        })
        .collect()
}

fn mention_labels(proposal: &Proposal, store: &Store) -> Vec<String> {
    let user_ids: Vec<String> = proposal
        .mentions
        .iter()
        .map(|mention| mention.user_id.clone())
        .collect();
    let mut names = store.display_names(&user_ids).unwrap_or_default();
    proposal
        .mentions
        .iter()
        .map(|mention| {
            names
                .remove(&mention.user_id)
                .unwrap_or_else(|| format!("{} (not in your contacts)", mention.name))
        })
        .collect()
}

fn entry(
    proposal: Proposal,
    images: Vec<ProposalPicture>,
    status: ProposalStatus,
    store: &Store,
) -> ProposalEntry {
    ProposalEntry {
        body: Arc::new(body_blocks(&proposal.text, &proposal.mentions)),
        mention_labels: mention_labels(&proposal, store),
        proposal,
        images,
        status,
    }
}

pub fn apply(
    proposals: &mut HashMap<String, ProposalEntry>,
    reactions: &mut HashMap<(String, String), ReactionProposal>,
    event: UiEvent,
    pictures: Vec<ProposalPicture>,
    store: &Store,
) {
    match event {
        UiEvent::ProposalChanged(proposal) => {
            proposals.insert(
                proposal.conversation_id.clone(),
                entry(proposal, pictures, ProposalStatus::Pending, store),
            );
        }
        UiEvent::ProposalStale(proposal) => {
            proposals.insert(
                proposal.conversation_id.clone(),
                entry(
                    proposal,
                    pictures,
                    ProposalStatus::Stale {
                        answered_at: Local::now(),
                    },
                    store,
                ),
            );
        }
        UiEvent::ProposalRemoved { conversation_id } => {
            proposals.remove(&conversation_id);
        }
        UiEvent::ReactionProposalChanged(reaction) => {
            reactions.insert(
                (
                    reaction.conversation_id.clone(),
                    reaction.message_id.clone(),
                ),
                reaction,
            );
        }
        UiEvent::ReactionProposalRemoved {
            conversation_id,
            message_id,
        } => {
            reactions.remove(&(conversation_id, message_id));
        }
    }
}

#[derive(Default)]
pub struct ReactionWatch {
    hashes: HashMap<String, HashMap<String, u64>>,
}

impl ReactionWatch {
    /// The first look at a conversation only records the state.
    pub fn observe(&mut self, conversation_id: &str, current: HashMap<String, u64>) -> Vec<String> {
        let Some(previous) = self
            .hashes
            .insert(conversation_id.to_owned(), current.clone())
        else {
            return Vec::new();
        };
        current
            .into_iter()
            .filter(|(message_id, now)| {
                previous.get(message_id).is_some_and(|before| before != now)
            })
            .map(|(message_id, _)| message_id)
            .collect()
    }

    pub fn forget(&mut self, conversation_id: &str) {
        self.hashes.remove(conversation_id);
    }

    pub fn clear(&mut self) {
        self.hashes.clear();
    }

    pub fn known(&self) -> Vec<String> {
        self.hashes.keys().cloned().collect()
    }

    fn knows_conversation(&self, conversation_id: &str) -> bool {
        self.hashes.contains_key(conversation_id)
    }

    fn retain_readable(&mut self, policy: &AgentPolicy) {
        self.hashes
            .retain(|conversation_id, _| policy.can_read_any_kind(conversation_id));
    }

    #[cfg(test)]
    pub fn knows(&self, conversation_id: &str) -> bool {
        self.knows_conversation(conversation_id)
    }

    fn seed(&mut self, conversation_id: &str, state: HashMap<String, u64>) {
        self.hashes
            .entry(conversation_id.to_owned())
            .or_insert(state);
    }
}

fn known_conversation_ids(store: &Store) -> Vec<String> {
    let chat_ids = store
        .recent_chats(usize::MAX >> 1)
        .unwrap_or_default()
        .into_iter()
        .map(|chat| chat.id);
    let channel_ids = store
        .sidebar()
        .map(|sidebar| sidebar.teams)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|team| team.channels)
        .map(|channel| channel.id);
    chat_ids.chain(channel_ids).collect()
}

pub type SharedWatch = Mutex<ReactionWatch>;

fn locked(watch: &SharedWatch) -> std::sync::MutexGuard<'_, ReactionWatch> {
    watch
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns false while the user id is still unknown.
pub async fn seed_reaction_watch(
    watch: &SharedWatch,
    hub: &AgentHub,
    store: &Arc<Store>,
    my_user_id: Option<&str>,
) -> bool {
    let policy = hub.policy();
    if !policy.enabled {
        locked(watch).clear();
        return true;
    }
    let Some(my_user_id) = my_user_id.map(str::to_owned) else {
        return false;
    };
    let wanted: Vec<String> = known_conversation_ids(store)
        .into_iter()
        .filter(|conversation_id| policy.can_read_any_kind(conversation_id))
        .collect();
    let fresh: Vec<String> = {
        let mut watch = locked(watch);
        watch.retain_readable(&policy);
        wanted
            .into_iter()
            .filter(|conversation_id| !watch.knows_conversation(conversation_id))
            .collect()
    };
    let store = store.clone();
    let seeds = tokio::task::spawn_blocking(move || {
        fresh
            .into_iter()
            .map(|conversation_id| {
                let state = reaction_state(&store, &conversation_id, &my_user_id);
                (conversation_id, state)
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let mut watch = locked(watch);
    for (conversation_id, state) in seeds {
        watch.seed(&conversation_id, state);
    }
    true
}

pub async fn reseed_if_needed(
    watch: &SharedWatch,
    seeded: &mut Option<AgentPolicy>,
    hub: &AgentHub,
    store: &Arc<Store>,
    my_user_id: Option<&str>,
) {
    let policy = hub.policy();
    if seeded.as_ref() == Some(&policy) {
        return;
    }
    if seed_reaction_watch(watch, hub, store, my_user_id).await {
        *seeded = Some(policy);
    }
}

pub fn rescan_watched(
    watch: &SharedWatch,
    hub: &AgentHub,
    store: &Store,
    my_user_id: Option<&str>,
) {
    let mut conversation_ids = locked(watch).known();
    conversation_ids.extend(hub.conversations_with_pending_reactions());
    conversation_ids.sort();
    conversation_ids.dedup();
    for conversation_id in conversation_ids {
        publish_reaction_changes(watch, hub, store, &conversation_id, my_user_id);
        expire_reaction_proposals(hub, store, &conversation_id, my_user_id);
    }
}

fn reactions_hash(reactions_json: &str, my_user_id: &str) -> u64 {
    let record = MessageRecord {
        reactions_json: reactions_json.to_owned(),
        ..MessageRecord::default()
    };
    let mut hasher = DefaultHasher::new();
    let mut summaries = teams_core::reaction_summaries(&record, Some(my_user_id));
    summaries.sort_by(|left, right| left.emoji.cmp(&right.emoji));
    for summary in summaries {
        (summary.emoji, summary.count, summary.mine).hash(&mut hasher);
    }
    hasher.finish()
}

fn reaction_state(store: &Store, conversation_id: &str, my_user_id: &str) -> HashMap<String, u64> {
    store
        .reaction_state_of_sender(conversation_id, my_user_id, REACTION_WATCH_WINDOW)
        .unwrap_or_default()
        .into_iter()
        .map(|(message_id, json)| (message_id, reactions_hash(&json, my_user_id)))
        .collect()
}

pub fn publish_reaction_changes(
    watch: &SharedWatch,
    hub: &AgentHub,
    store: &Store,
    conversation_id: &str,
    my_user_id: Option<&str>,
) {
    let policy = hub.policy();
    if !(policy.enabled && policy.can_read_any_kind(conversation_id)) {
        locked(watch).forget(conversation_id);
        return;
    }
    let Some(my_user_id) = my_user_id else {
        return;
    };
    let changed = {
        let mut guard = locked(watch);
        let current = reaction_state(store, conversation_id, my_user_id);
        guard.observe(conversation_id, current)
    };
    if changed.is_empty() {
        return;
    }
    let found = store
        .messages_by_id(conversation_id, &changed)
        .unwrap_or_default();
    for message_id in changed {
        if let Some(message) = found.get(&message_id) {
            hub.reaction_changed(
                conversation_id,
                &message_id,
                message_extras(message, Some(my_user_id)).reactions,
            );
        }
    }
}

pub fn expire_reaction_proposals(
    hub: &AgentHub,
    store: &Store,
    conversation_id: &str,
    my_user_id: Option<&str>,
) {
    for reaction in hub.pending_reactions(conversation_id) {
        let message = store
            .messages_by_id(conversation_id, std::slice::from_ref(&reaction.message_id))
            .ok()
            .and_then(|mut found| found.remove(&reaction.message_id));
        let outcome = match message {
            None => Some(ReactionOutcome::Stale),
            Some(message) if message.deleted => Some(ReactionOutcome::Stale),
            Some(message) => own_reaction_outcome(&message, &reaction, my_user_id),
        };
        if let Some(outcome) = outcome {
            hub.resolve_reaction(&reaction.id, outcome);
        }
    }
}

/// An own reaction with the proposed emoji from after the proposal means the user accepted it.
fn own_reaction_outcome(
    message: &MessageRecord,
    proposal: &ReactionProposal,
    my_user_id: Option<&str>,
) -> Option<ReactionOutcome> {
    let wanted = reaction_glyph(&proposal.emoji);
    let own: Vec<_> = teams_core::reactions(message)
        .into_iter()
        .filter(|reaction| {
            my_user_id.is_some()
                && reaction.user_id.as_deref() == my_user_id
                && reaction_glyph(&reaction.reaction_type) == wanted
        })
        .collect();
    if own.is_empty() {
        return None;
    }
    let after = own.iter().any(|reaction| {
        reaction
            .created_at
            .is_some_and(|at| at >= proposal.created_at - CLOCK_SKEW_ALLOWANCE)
    });
    Some(if after {
        ReactionOutcome::Accepted
    } else {
        ReactionOutcome::Stale
    })
}

pub fn spawn_message_bridge(engine: Arc<Engine>, hub: Arc<AgentHub>, source: Arc<AppSource>) {
    let started_at = Utc::now() - CLOCK_SKEW_ALLOWANCE;
    let watch: Arc<SharedWatch> = Arc::default();
    let rescan = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(tokio::sync::Notify::new());
    drop(runtime::spawn({
        let (watch, hub, source, rescan, wake) = (
            watch.clone(),
            hub.clone(),
            source.clone(),
            rescan.clone(),
            wake.clone(),
        );
        async move {
            let mut seeded: Option<AgentPolicy> = None;
            let mut ticker = tokio::time::interval(SEED_RETRY);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = wake.notified() => {}
                }
                let Ok(store) = mcp::TeamsSource::store(source.as_ref()) else {
                    continue;
                };
                let my_id = mcp::TeamsSource::me_id(source.as_ref());
                reseed_if_needed(&watch, &mut seeded, &hub, &store, my_id.as_deref()).await;
                if rescan.swap(false, Ordering::SeqCst) {
                    let (watch, hub) = (watch.clone(), hub.clone());
                    let _ = tokio::task::spawn_blocking(move || {
                        rescan_watched(&watch, &hub, &store, my_id.as_deref())
                    })
                    .await;
                }
            }
        }
    }));
    drop(runtime::spawn(async move {
        let mut receiver = engine.subscribe();
        let mut watermarks: HashMap<String, DateTime<Utc>> = HashMap::new();
        loop {
            let conversation_id = match receiver.recv().await {
                Ok(CoreEvent::MessagesChanged { conversation_id }) => conversation_id,
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    rescan.store(true, Ordering::SeqCst);
                    wake.notify_one();
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            let Ok(store) = mcp::TeamsSource::store(source.as_ref()) else {
                continue;
            };
            let my_id = mcp::TeamsSource::me_id(source.as_ref());
            let watermark = watermarks
                .entry(conversation_id.clone())
                .or_insert(started_at);
            loop {
                let Ok(messages) =
                    store.messages_after(&conversation_id, *watermark, NEW_MESSAGE_BATCH)
                else {
                    break;
                };
                let batch_full = messages.len() == NEW_MESSAGE_BATCH;
                for message in messages {
                    *watermark = (*watermark).max(message.created_at);
                    if message.deleted {
                        continue;
                    }
                    let sender_is_me = my_id.is_some() && message.sender_id == my_id;
                    let preview: String = store::plain_text(&message.body_html)
                        .chars()
                        .take(PREVIEW_CHARACTERS)
                        .collect();
                    hub.message_arrived(
                        &conversation_id,
                        &message.message_id,
                        sender_is_me,
                        message.created_at,
                        message.sender_name,
                        &preview,
                    );
                }
                if !batch_full {
                    break;
                }
            }
            let (watch, hub, store, conversation_id) = (
                watch.clone(),
                hub.clone(),
                store.clone(),
                conversation_id.clone(),
            );
            let _ = tokio::task::spawn_blocking(move || {
                publish_reaction_changes(&watch, &hub, &store, &conversation_id, my_id.as_deref());
                expire_reaction_proposals(&hub, &store, &conversation_id, my_id.as_deref());
            })
            .await;
        }
    }));
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::{TimeZone, Utc};
    use mcp::{
        Access, AgentPolicy, EventKind, NewProposal, NewReactionProposal, ReactionOutcome, UiEvent,
    };
    use serde_json::json;
    use store::{MessageRecord, Store};

    use super::{
        AgentHub, ReactionWatch, SharedWatch, apply, expire_reaction_proposals, locked,
        publish_reaction_changes, reactions_hash, rescan_watched, reseed_if_needed,
        seed_reaction_watch,
    };
    use crate::runtime;

    const ME: &str = "user-me";
    const CONVERSATION: &str = "19:chat@thread.v2";

    fn message(message_id: &str, sender_id: &str, reactions: &[(&str, &str)]) -> MessageRecord {
        let reactions: Vec<_> = reactions
            .iter()
            .map(|(reaction_type, user_id)| {
                json!({"reaction_type": reaction_type, "user_id": user_id, "user_name": null})
            })
            .collect();
        MessageRecord {
            conversation_id: CONVERSATION.to_owned(),
            message_id: message_id.to_owned(),
            sender_id: Some(sender_id.to_owned()),
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
            body_html: "<p>hi</p>".to_owned(),
            attachments_json: "[]".to_owned(),
            reactions_json: serde_json::Value::Array(reactions).to_string(),
            mentions_json: "[]".to_owned(),
            links_json: "[]".to_owned(),
            ..MessageRecord::default()
        }
    }

    fn enabled_policy() -> AgentPolicy {
        AgentPolicy {
            enabled: true,
            chats: Access::All,
            propose: true,
            ..AgentPolicy::default()
        }
    }

    fn hub_with(store: &Arc<Store>) -> Arc<AgentHub> {
        let (ui, _receiver) = tokio::sync::mpsc::unbounded_channel();
        AgentHub::new(store.clone(), enabled_policy(), ui)
    }

    fn events(hub: &AgentHub) -> Vec<EventKind> {
        runtime::handle()
            .block_on(hub.wait_events(Some(0), Duration::from_millis(20)))
            .0
            .into_iter()
            .map(|event| event.kind)
            .collect()
    }

    fn run<T>(future: impl std::future::Future<Output = T>) -> T {
        runtime::handle().block_on(future)
    }

    fn seeded_store() -> Arc<Store> {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .upsert_chats(&[store::ChatRecord {
                id: CONVERSATION.to_owned(),
                kind: "group".to_owned(),
                ..store::ChatRecord::default()
            }])
            .unwrap();
        store.upsert_messages(&[message("old", ME, &[])]).unwrap();
        store
    }

    #[test]
    fn the_watch_reports_only_known_messages_whose_reactions_changed() {
        let mut watch = ReactionWatch::default();
        let state = |entries: &[(&str, u64)]| -> HashMap<String, u64> {
            entries
                .iter()
                .map(|(id, hash)| ((*id).to_owned(), *hash))
                .collect()
        };
        assert!(
            watch
                .observe(CONVERSATION, state(&[("old", 1), ("same", 5)]))
                .is_empty()
        );
        let changed = watch.observe(CONVERSATION, state(&[("old", 2), ("same", 5), ("new", 9)]));
        assert_eq!(changed, ["old"]);
        assert!(
            watch
                .observe(CONVERSATION, state(&[("old", 2), ("same", 5), ("new", 9)]))
                .is_empty()
        );
    }

    #[test]
    fn the_hash_follows_the_grouped_reactions_not_the_raw_json() {
        let like = |user: &str| format!(r#"{{"reaction_type":"like","user_id":"{user}"}}"#);
        let one = format!("[{}]", like("a"));
        let reordered = format!("[ {} ]", like("a"));
        let two = format!("[{},{}]", like("a"), like("b"));
        assert_eq!(reactions_hash(&one, ME), reactions_hash(&reordered, ME));
        assert_ne!(reactions_hash(&one, ME), reactions_hash(&two, ME));
        assert_ne!(
            reactions_hash(&one, ME),
            reactions_hash(&format!("[{}]", like(ME)), ME)
        );
    }

    #[test]
    fn a_reaction_on_an_old_message_emits_reaction_changed() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .upsert_messages(&[message("old", ME, &[]), message("theirs", "user-ada", &[])])
            .unwrap();
        let hub = hub_with(&store);
        let watch = SharedWatch::default();
        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        assert!(events(&hub).is_empty());

        store
            .upsert_messages(&[
                message("old", ME, &[("like", "user-ada"), ("heart", ME)]),
                message("theirs", "user-ada", &[("like", ME)]),
            ])
            .unwrap();
        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        let events = events(&hub);
        assert_eq!(events.len(), 1);
        let EventKind::ReactionChanged {
            message_id,
            reactions,
            ..
        } = &events[0]
        else {
            panic!("{events:?}");
        };
        assert_eq!(message_id, "old");
        let summary: Vec<_> = reactions
            .iter()
            .map(|reaction| (reaction.emoji.as_str(), reaction.count, reaction.mine))
            .collect();
        assert_eq!(summary, [("\u{1F44D}", 1, false), ("\u{2764}", 1, true)]);
    }

    #[test]
    fn reaction_proposals_go_stale_when_the_message_is_gone_or_already_reacted() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .upsert_messages(&[
                message("open", "user-ada", &[]),
                message("done", "user-ada", &[("like", ME)]),
                message("gone", "user-ada", &[]),
            ])
            .unwrap();
        let hub = hub_with(&store);
        for message_id in ["open", "done", "gone"] {
            hub.add_reaction_proposal(NewReactionProposal {
                conversation_id: CONVERSATION.to_owned(),
                message_id: message_id.to_owned(),
                emoji: "\u{1F44D}".to_owned(),
                ..NewReactionProposal::default()
            })
            .unwrap();
        }
        let mut gone = message("gone", "user-ada", &[]);
        gone.deleted = true;
        store.upsert_messages(&[gone]).unwrap();

        expire_reaction_proposals(&hub, &store, CONVERSATION, Some(ME));
        let pending: Vec<_> = hub
            .pending_reactions(CONVERSATION)
            .into_iter()
            .map(|reaction| reaction.message_id)
            .collect();
        assert_eq!(pending, ["open"]);
        let stale = events(&hub)
            .iter()
            .filter(|kind| {
                matches!(
                    kind,
                    EventKind::ReactionResolved {
                        outcome: ReactionOutcome::Stale,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(stale, 2);
    }

    #[test]
    fn a_reaction_proposal_sits_beside_the_message_proposal() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let hub = hub_with(&store);
        let proposal = hub
            .add_proposal(NewProposal {
                conversation_id: CONVERSATION.to_owned(),
                text: "hi".to_owned(),
                ..NewProposal::default()
            })
            .unwrap();
        let reaction = hub
            .add_reaction_proposal(NewReactionProposal {
                conversation_id: CONVERSATION.to_owned(),
                message_id: "m1".to_owned(),
                emoji: "\u{1F44D}".to_owned(),
                ..NewReactionProposal::default()
            })
            .unwrap();
        let (mut proposals, mut reactions) = (HashMap::new(), HashMap::new());
        apply(
            &mut proposals,
            &mut reactions,
            UiEvent::ProposalChanged(proposal),
            Vec::new(),
            &store,
        );
        apply(
            &mut proposals,
            &mut reactions,
            UiEvent::ReactionProposalChanged(reaction),
            Vec::new(),
            &store,
        );
        assert_eq!(proposals.len(), 1);
        assert_eq!(reactions.len(), 1);
        apply(
            &mut proposals,
            &mut reactions,
            UiEvent::ReactionProposalRemoved {
                conversation_id: CONVERSATION.to_owned(),
                message_id: "m1".to_owned(),
            },
            Vec::new(),
            &store,
        );
        assert_eq!(proposals.len(), 1);
        assert!(reactions.is_empty());
    }

    #[test]
    fn the_watch_starts_lazily_only_for_readable_conversations_and_known_users() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store.upsert_messages(&[message("old", ME, &[])]).unwrap();
        let (ui, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let hub = AgentHub::new(store.clone(), AgentPolicy::default(), ui);
        let watch = SharedWatch::default();

        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        assert!(locked(&watch).known().is_empty());

        hub.set_policy(enabled_policy());
        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, None);
        assert!(locked(&watch).known().is_empty());

        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        assert!(locked(&watch).knows(CONVERSATION));

        hub.set_policy(AgentPolicy::default());
        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        assert!(locked(&watch).known().is_empty());
    }

    #[test]
    fn the_first_reaction_after_the_start_seed_is_reported() {
        let store = seeded_store();
        let hub = hub_with(&store);
        let watch = SharedWatch::default();
        assert!(run(seed_reaction_watch(&watch, &hub, &store, Some(ME))));
        assert!(locked(&watch).knows(CONVERSATION));

        store
            .upsert_messages(&[message("old", ME, &[("like", "user-ada")])])
            .unwrap();
        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        assert_eq!(events(&hub).len(), 1);
    }

    #[test]
    fn switching_the_policy_on_reseeds_and_an_unknown_user_retries() {
        let store = seeded_store();
        let (ui, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let hub = AgentHub::new(store.clone(), AgentPolicy::default(), ui);
        let watch = SharedWatch::default();
        let mut seeded = None;

        run(reseed_if_needed(
            &watch,
            &mut seeded,
            &hub,
            &store,
            Some(ME),
        ));
        assert!(!locked(&watch).knows(CONVERSATION));

        hub.set_policy(enabled_policy());
        run(reseed_if_needed(&watch, &mut seeded, &hub, &store, None));
        assert!(!locked(&watch).knows(CONVERSATION));
        assert_ne!(seeded, Some(enabled_policy()));

        run(reseed_if_needed(
            &watch,
            &mut seeded,
            &hub,
            &store,
            Some(ME),
        ));
        assert!(locked(&watch).knows(CONVERSATION));
        assert_eq!(seeded, Some(enabled_policy()));

        store
            .upsert_messages(&[message("old", ME, &[("heart", "user-ada")])])
            .unwrap();
        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        assert_eq!(events(&hub).len(), 1);

        hub.set_policy(AgentPolicy::default());
        run(reseed_if_needed(
            &watch,
            &mut seeded,
            &hub,
            &store,
            Some(ME),
        ));
        assert!(!locked(&watch).knows(CONVERSATION));
    }

    #[test]
    fn narrowing_the_policy_forgets_conversations_and_widening_seeds_the_new_ones() {
        let store = seeded_store();
        let hub = hub_with(&store);
        let watch = SharedWatch::default();
        run(seed_reaction_watch(&watch, &hub, &store, Some(ME)));
        assert!(locked(&watch).knows(CONVERSATION));

        hub.set_policy(AgentPolicy {
            enabled: true,
            chats: Access::None,
            ..AgentPolicy::default()
        });
        run(seed_reaction_watch(&watch, &hub, &store, Some(ME)));
        assert!(!locked(&watch).knows(CONVERSATION));

        store
            .upsert_messages(&[message("old", ME, &[("like", "user-ada")])])
            .unwrap();
        hub.set_policy(enabled_policy());
        run(seed_reaction_watch(&watch, &hub, &store, Some(ME)));
        publish_reaction_changes(&watch, &hub, &store, CONVERSATION, Some(ME));
        assert!(events(&hub).is_empty());
    }

    #[test]
    fn a_lagged_receiver_rescans_the_watched_conversations() {
        let store = seeded_store();
        let hub = hub_with(&store);
        let watch = SharedWatch::default();
        run(seed_reaction_watch(&watch, &hub, &store, Some(ME)));
        store
            .upsert_messages(&[message("old", ME, &[("like", "user-ada")])])
            .unwrap();
        rescan_watched(&watch, &hub, &store, Some(ME));
        assert_eq!(events(&hub).len(), 1);
    }

    #[test]
    fn an_own_reaction_from_after_the_proposal_resolves_it_as_accepted() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let hub = hub_with(&store);
        let reaction = hub
            .add_reaction_proposal(NewReactionProposal {
                conversation_id: CONVERSATION.to_owned(),
                message_id: "m1".to_owned(),
                emoji: "\u{1F44D}".to_owned(),
                ..NewReactionProposal::default()
            })
            .unwrap();
        let after = (reaction.created_at + chrono::Duration::seconds(5)).to_rfc3339();
        let mut accepted = message("m1", "user-ada", &[]);
        accepted.reactions_json = format!(
            r#"[{{"reaction_type":"like","user_id":"{ME}","user_name":null,"created_at":"{after}"}}]"#
        );
        store.upsert_messages(&[accepted]).unwrap();
        expire_reaction_proposals(&hub, &store, CONVERSATION, Some(ME));
        assert!(hub.pending_reactions(CONVERSATION).is_empty());
        assert!(events(&hub).iter().any(|kind| matches!(
            kind,
            EventKind::ReactionResolved {
                outcome: ReactionOutcome::Accepted,
                ..
            }
        )));
    }

    #[test]
    fn a_reaction_just_before_the_proposal_still_counts_as_accepted_within_the_clock_skew() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let hub = hub_with(&store);
        let reaction = hub
            .add_reaction_proposal(NewReactionProposal {
                conversation_id: CONVERSATION.to_owned(),
                message_id: "m1".to_owned(),
                emoji: "\u{1F44D}".to_owned(),
                ..NewReactionProposal::default()
            })
            .unwrap();
        let at =
            |seconds: i64| (reaction.created_at + chrono::Duration::seconds(seconds)).to_rfc3339();
        let reacted = |when: String| {
            let mut record = message("m1", "user-ada", &[]);
            record.reactions_json = format!(
                r#"[{{"reaction_type":"like","user_id":"{ME}","user_name":null,"created_at":"{when}"}}]"#
            );
            record
        };
        assert_eq!(
            super::own_reaction_outcome(&reacted(at(-1)), &reaction, Some(ME)),
            Some(ReactionOutcome::Accepted)
        );
        assert_eq!(
            super::own_reaction_outcome(&reacted(at(-3600)), &reaction, Some(ME)),
            Some(ReactionOutcome::Stale)
        );
    }

    #[test]
    fn a_rescan_also_expires_reaction_proposals_of_unwatched_conversations() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .upsert_messages(&[message("m1", "user-ada", &[])])
            .unwrap();
        let hub = hub_with(&store);
        hub.add_reaction_proposal(NewReactionProposal {
            conversation_id: CONVERSATION.to_owned(),
            message_id: "m1".to_owned(),
            emoji: "\u{1F44D}".to_owned(),
            ..NewReactionProposal::default()
        })
        .unwrap();
        let mut gone = message("m1", "user-ada", &[]);
        gone.deleted = true;
        store.upsert_messages(&[gone]).unwrap();
        rescan_watched(&SharedWatch::default(), &hub, &store, Some(ME));
        assert!(hub.pending_reactions(CONVERSATION).is_empty());
    }
}
