use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use store::{
    AgentProposalMention, AgentProposalRecord, AgentReactionRecord, AttachmentImage, Store,
};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{Notify, Semaphore};

use crate::policy::AgentPolicy;
use crate::source::ReactionInfo;

const EVENT_CAPACITY: usize = 1000;
const CAPTION_CAPACITY: usize = 2000;
const PENDING_STATE: &str = "pending";
const PROPOSAL_PERMITS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalMention {
    pub user_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalImage {
    pub name: String,
    pub mime_type: String,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub conversation_id: String,
    pub reply_to_message_id: Option<String>,
    pub text: String,
    pub mentions: Vec<ProposalMention>,
    pub images: Vec<ProposalImage>,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewProposal {
    pub conversation_id: String,
    pub text: String,
    pub reply_to_message_id: Option<String>,
    pub mentions: Vec<ProposalMention>,
    pub images: Vec<ProposalImage>,
    pub agent: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionProposal {
    pub id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub emoji: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewReactionProposal {
    pub conversation_id: String,
    pub message_id: String,
    pub emoji: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptionLine {
    #[serde(skip)]
    pub caption_id: String,
    pub speaker: String,
    pub text: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Sent,
    Edited,
    Discarded,
    Stale,
    Superseded,
}

impl Outcome {
    fn state(self) -> &'static str {
        match self {
            Outcome::Sent => "sent",
            Outcome::Edited => "edited",
            Outcome::Discarded => "discarded",
            Outcome::Stale => "stale",
            Outcome::Superseded => "superseded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReactionOutcome {
    Accepted,
    Discarded,
    Stale,
    Superseded,
}

impl ReactionOutcome {
    fn state(self) -> &'static str {
        match self {
            ReactionOutcome::Accepted => "accepted",
            ReactionOutcome::Discarded => "discarded",
            ReactionOutcome::Stale => "stale",
            ReactionOutcome::Superseded => "superseded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    NewMessage {
        conversation_id: String,
        message_id: String,
        sender: Option<String>,
        preview: String,
    },
    UserTyping {
        conversation_id: String,
    },
    UserSent {
        conversation_id: String,
    },
    ProposalResolved {
        proposal_id: String,
        conversation_id: String,
        outcome: Outcome,
    },
    ReactionResolved {
        proposal_id: String,
        conversation_id: String,
        message_id: String,
        outcome: ReactionOutcome,
    },
    ReactionChanged {
        conversation_id: String,
        message_id: String,
        reactions: Vec<ReactionInfo>,
    },
}

impl EventKind {
    pub fn conversation_id(&self) -> &str {
        match self {
            EventKind::NewMessage {
                conversation_id, ..
            }
            | EventKind::UserTyping { conversation_id }
            | EventKind::UserSent { conversation_id }
            | EventKind::ProposalResolved {
                conversation_id, ..
            }
            | EventKind::ReactionResolved {
                conversation_id, ..
            }
            | EventKind::ReactionChanged {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub seq: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    ProposalChanged(Proposal),
    ProposalStale(Proposal),
    ProposalRemoved {
        conversation_id: String,
    },
    ReactionProposalChanged(ReactionProposal),
    ReactionProposalRemoved {
        conversation_id: String,
        message_id: String,
    },
}

#[derive(Default)]
struct HubState {
    policy: AgentPolicy,
    pending: HashMap<String, Proposal>,
    pending_reactions: HashMap<(String, String), ReactionProposal>,
    events: VecDeque<Event>,
    last_seq: u64,
    captions: HashMap<String, VecDeque<CaptionLine>>,
}

pub struct AgentHub {
    state: Mutex<HubState>,
    wake: Notify,
    proposal_permits: Arc<Semaphore>,
    store: Arc<Store>,
    ui: UnboundedSender<UiEvent>,
}

impl AgentHub {
    pub fn new(store: Arc<Store>, policy: AgentPolicy, ui: UnboundedSender<UiEvent>) -> Arc<Self> {
        Arc::new(AgentHub {
            state: Mutex::new(HubState {
                policy,
                ..HubState::default()
            }),
            wake: Notify::new(),
            proposal_permits: Arc::new(Semaphore::new(PROPOSAL_PERMITS)),
            store,
            ui,
        })
    }

    pub fn proposal_permits(&self) -> Arc<Semaphore> {
        self.proposal_permits.clone()
    }

    pub fn set_policy(&self, policy: AgentPolicy) {
        self.lock().policy = policy;
    }

    pub fn policy(&self) -> AgentPolicy {
        self.lock().policy.clone()
    }

    pub fn pending_proposal(&self, conversation_id: &str) -> Option<Proposal> {
        self.lock().pending.get(conversation_id).cloned()
    }

    pub fn pending_reactions(&self, conversation_id: &str) -> Vec<ReactionProposal> {
        self.lock()
            .pending_reactions
            .values()
            .filter(|reaction| reaction.conversation_id == conversation_id)
            .cloned()
            .collect()
    }

    pub fn conversations_with_pending_reactions(&self) -> Vec<String> {
        let mut conversations: Vec<String> = self
            .lock()
            .pending_reactions
            .values()
            .map(|reaction| reaction.conversation_id.clone())
            .collect();
        conversations.sort();
        conversations.dedup();
        conversations
    }

    pub fn restore_pending(&self) -> store::Result<()> {
        self.store.sweep_agent_proposal_images()?;
        let records = self.store.pending_agent_proposals()?;
        let reactions = self.store.pending_agent_reactions()?;
        let mut state = self.lock();
        for record in records {
            let proposal = proposal_from_record(record);
            let _ = self.ui.send(UiEvent::ProposalChanged(proposal.clone()));
            state
                .pending
                .insert(proposal.conversation_id.clone(), proposal);
        }
        for record in reactions {
            let reaction = reaction_from_record(record);
            let _ = self
                .ui
                .send(UiEvent::ReactionProposalChanged(reaction.clone()));
            state
                .pending_reactions
                .insert(reaction_key(&reaction), reaction);
        }
        Ok(())
    }

    pub fn add_proposal(&self, new: NewProposal) -> store::Result<Proposal> {
        let proposal = Proposal {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: new.conversation_id,
            reply_to_message_id: new.reply_to_message_id,
            text: new.text,
            mentions: new.mentions,
            images: new.images,
            agent: new.agent,
            session_id: new.session_id,
            created_at: now_millis(),
        };
        self.store.insert_agent_proposal(&AgentProposalRecord {
            id: proposal.id.clone(),
            conversation_id: proposal.conversation_id.clone(),
            reply_to_message_id: proposal.reply_to_message_id.clone(),
            text: proposal.text.clone(),
            mentions: proposal
                .mentions
                .iter()
                .map(|mention| AgentProposalMention {
                    user_id: mention.user_id.clone(),
                    name: mention.name.clone(),
                })
                .collect(),
            images: proposal
                .images
                .iter()
                .map(|image| AttachmentImage {
                    name: image.name.clone(),
                    format: image.mime_type.clone(),
                    bytes: image.bytes.to_vec(),
                    width: None,
                    height: None,
                })
                .collect(),
            agent: proposal.agent.clone(),
            session_id: proposal.session_id.clone(),
            created_at: proposal.created_at,
            state: PENDING_STATE.to_owned(),
            resolved_at: None,
        })?;
        let mut state = self.lock();
        if let Some(previous) = state
            .pending
            .insert(proposal.conversation_id.clone(), proposal.clone())
        {
            self.finish(&mut state, &previous, Outcome::Superseded);
        }
        let _ = self.ui.send(UiEvent::ProposalChanged(proposal.clone()));
        drop(state);
        self.wake.notify_waiters();
        Ok(proposal)
    }

    pub fn resolve(&self, proposal_id: &str, outcome: Outcome) -> bool {
        let mut state = self.lock();
        let Some(conversation_id) = state
            .pending
            .values()
            .find(|proposal| proposal.id == proposal_id)
            .map(|proposal| proposal.conversation_id.clone())
        else {
            return false;
        };
        let Some(proposal) = state.pending.remove(&conversation_id) else {
            return false;
        };
        self.finish(&mut state, &proposal, outcome);
        let _ = self.ui.send(UiEvent::ProposalRemoved { conversation_id });
        drop(state);
        self.wake.notify_waiters();
        true
    }

    pub fn add_reaction_proposal(
        &self,
        new: NewReactionProposal,
    ) -> store::Result<ReactionProposal> {
        let reaction = ReactionProposal {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: new.conversation_id,
            message_id: new.message_id,
            emoji: new.emoji,
            agent: new.agent,
            session_id: new.session_id,
            created_at: now_millis(),
        };
        self.store.insert_agent_reaction(&AgentReactionRecord {
            id: reaction.id.clone(),
            conversation_id: reaction.conversation_id.clone(),
            message_id: reaction.message_id.clone(),
            emoji: reaction.emoji.clone(),
            agent: reaction.agent.clone(),
            session_id: reaction.session_id.clone(),
            created_at: reaction.created_at,
            state: PENDING_STATE.to_owned(),
            resolved_at: None,
        })?;
        let mut state = self.lock();
        if let Some(previous) = state
            .pending_reactions
            .insert(reaction_key(&reaction), reaction.clone())
        {
            self.finish_reaction(&mut state, &previous, ReactionOutcome::Superseded);
        }
        let _ = self
            .ui
            .send(UiEvent::ReactionProposalChanged(reaction.clone()));
        drop(state);
        self.wake.notify_waiters();
        Ok(reaction)
    }

    pub fn resolve_reaction(&self, reaction_id: &str, outcome: ReactionOutcome) -> bool {
        let mut state = self.lock();
        let Some(key) = state
            .pending_reactions
            .iter()
            .find(|(_, reaction)| reaction.id == reaction_id)
            .map(|(key, _)| key.clone())
        else {
            return false;
        };
        let Some(reaction) = state.pending_reactions.remove(&key) else {
            return false;
        };
        self.finish_reaction(&mut state, &reaction, outcome);
        let _ = self.ui.send(UiEvent::ReactionProposalRemoved {
            conversation_id: reaction.conversation_id,
            message_id: reaction.message_id,
        });
        drop(state);
        self.wake.notify_waiters();
        true
    }

    pub fn reaction_changed(
        &self,
        conversation_id: &str,
        message_id: &str,
        reactions: Vec<ReactionInfo>,
    ) {
        self.push(EventKind::ReactionChanged {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            reactions,
        });
    }

    pub fn push_caption(&self, conversation_id: &str, caption_id: &str, speaker: &str, text: &str) {
        let mut state = self.lock();
        let lines = state
            .captions
            .entry(conversation_id.to_owned())
            .or_default();
        let line = CaptionLine {
            caption_id: caption_id.to_owned(),
            speaker: speaker.to_owned(),
            text: text.to_owned(),
            at: now_millis(),
        };
        match lines.back_mut() {
            Some(last) if last.caption_id == caption_id => *last = line,
            _ => lines.push_back(line),
        }
        if lines.len() > CAPTION_CAPACITY {
            lines.pop_front();
        }
    }

    pub fn live_captions(&self, conversation_id: &str) -> Vec<CaptionLine> {
        self.lock()
            .captions
            .get(conversation_id)
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear_captions(&self, conversation_id: &str) {
        self.lock().captions.remove(conversation_id);
    }

    pub fn user_typing(&self, conversation_id: &str) {
        self.push(EventKind::UserTyping {
            conversation_id: conversation_id.to_owned(),
        });
    }

    pub fn user_sent(&self, conversation_id: &str) {
        self.push(EventKind::UserSent {
            conversation_id: conversation_id.to_owned(),
        });
        self.stale_pending(conversation_id);
    }

    pub fn message_arrived(
        &self,
        conversation_id: &str,
        message_id: &str,
        sender_is_me: bool,
        created_at: DateTime<Utc>,
        sender: Option<String>,
        preview: &str,
    ) {
        if sender_is_me {
            let answers_pending = self
                .pending_proposal(conversation_id)
                .is_some_and(|proposal| created_at >= proposal.created_at);
            if answers_pending {
                self.stale_pending(conversation_id);
            }
            return;
        }
        self.push(EventKind::NewMessage {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            sender,
            preview: preview.to_owned(),
        });
    }

    pub async fn wait_events(&self, since: Option<u64>, timeout: Duration) -> (Vec<Event>, u64) {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut cursor = since.unwrap_or_else(|| self.lock().last_seq);
        loop {
            let notified = self.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let (events, next) = self.events_after(cursor);
            if !events.is_empty() {
                return (events, next);
            }
            cursor = next;
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return (Vec::new(), cursor);
            }
        }
    }

    fn events_after(&self, cursor: u64) -> (Vec<Event>, u64) {
        let state = self.lock();
        let events = state
            .events
            .iter()
            .filter(|event| event.seq > cursor)
            .filter(|event| state.policy.can_read_any_kind(event.kind.conversation_id()))
            .cloned()
            .collect();
        (events, state.last_seq.max(cursor))
    }

    fn stale_pending(&self, conversation_id: &str) {
        let mut state = self.lock();
        let Some(proposal) = state.pending.remove(conversation_id) else {
            return;
        };
        self.finish(&mut state, &proposal, Outcome::Stale);
        let _ = self.ui.send(UiEvent::ProposalStale(proposal));
        drop(state);
        self.wake.notify_waiters();
    }

    fn finish_reaction(
        &self,
        state: &mut HubState,
        reaction: &ReactionProposal,
        outcome: ReactionOutcome,
    ) {
        if let Err(error) =
            self.store
                .set_agent_reaction_state(&reaction.id, outcome.state(), Some(now_millis()))
        {
            eprintln!("mcp: could not store reaction proposal state: {error}");
        }
        push_event(
            state,
            EventKind::ReactionResolved {
                proposal_id: reaction.id.clone(),
                conversation_id: reaction.conversation_id.clone(),
                message_id: reaction.message_id.clone(),
                outcome,
            },
        );
    }

    fn finish(&self, state: &mut HubState, proposal: &Proposal, outcome: Outcome) {
        if let Err(error) =
            self.store
                .set_agent_proposal_state(&proposal.id, outcome.state(), Some(now_millis()))
        {
            eprintln!("mcp: could not store proposal state: {error}");
        }
        push_event(
            state,
            EventKind::ProposalResolved {
                proposal_id: proposal.id.clone(),
                conversation_id: proposal.conversation_id.clone(),
                outcome,
            },
        );
    }

    fn push(&self, kind: EventKind) {
        push_event(&mut self.lock(), kind);
        self.wake.notify_waiters();
    }

    fn lock(&self) -> MutexGuard<'_, HubState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn now_millis() -> DateTime<Utc> {
    DateTime::from_timestamp_millis(Utc::now().timestamp_millis()).unwrap_or_default()
}

fn push_event(state: &mut HubState, kind: EventKind) {
    state.last_seq += 1;
    if state.events.len() == EVENT_CAPACITY {
        state.events.pop_front();
    }
    state.events.push_back(Event {
        seq: state.last_seq,
        kind,
    });
}

fn reaction_key(reaction: &ReactionProposal) -> (String, String) {
    (
        reaction.conversation_id.clone(),
        reaction.message_id.clone(),
    )
}

fn reaction_from_record(record: AgentReactionRecord) -> ReactionProposal {
    ReactionProposal {
        id: record.id,
        conversation_id: record.conversation_id,
        message_id: record.message_id,
        emoji: record.emoji,
        agent: record.agent,
        session_id: record.session_id,
        created_at: record.created_at,
    }
}

fn proposal_from_record(record: AgentProposalRecord) -> Proposal {
    Proposal {
        id: record.id,
        conversation_id: record.conversation_id,
        reply_to_message_id: record.reply_to_message_id,
        text: record.text,
        mentions: record
            .mentions
            .into_iter()
            .map(|mention| ProposalMention {
                user_id: mention.user_id,
                name: mention.name,
            })
            .collect(),
        images: record
            .images
            .into_iter()
            .map(|image| ProposalImage {
                name: image.name,
                mime_type: image.format,
                bytes: Arc::new(image.bytes),
            })
            .collect(),
        agent: record.agent,
        session_id: record.session_id,
        created_at: record.created_at,
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    use super::*;
    use crate::policy::Access;

    const SHORT: Duration = Duration::from_millis(50);

    fn open_policy() -> AgentPolicy {
        AgentPolicy {
            enabled: true,
            chats: Access::All,
            channels: Access::All,
            propose: true,
            ..AgentPolicy::default()
        }
    }

    fn hub_with(policy: AgentPolicy) -> (Arc<AgentHub>, UnboundedReceiver<UiEvent>, Arc<Store>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let (sender, receiver) = unbounded_channel();
        (
            AgentHub::new(store.clone(), policy, sender),
            receiver,
            store,
        )
    }

    fn propose(hub: &AgentHub, conversation_id: &str, text: &str) -> Proposal {
        hub.add_proposal(NewProposal {
            conversation_id: conversation_id.to_owned(),
            text: text.to_owned(),
            ..NewProposal::default()
        })
        .unwrap()
    }

    fn react(hub: &AgentHub, conversation_id: &str, message_id: &str) -> ReactionProposal {
        hub.add_reaction_proposal(NewReactionProposal {
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            emoji: "\u{1F44D}".to_owned(),
            ..NewReactionProposal::default()
        })
        .unwrap()
    }

    #[test]
    fn a_new_proposal_supersedes_the_pending_one() {
        let (hub, mut ui, store) = hub_with(open_policy());
        let first = propose(&hub, "c1", "one");
        let second = propose(&hub, "c1", "two");
        assert_eq!(hub.pending_proposal("c1"), Some(second.clone()));
        let pending = store.pending_agent_proposals().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, second.id);
        assert_eq!(
            ui.try_recv().unwrap(),
            UiEvent::ProposalChanged(first.clone())
        );
        assert_eq!(ui.try_recv().unwrap(), UiEvent::ProposalChanged(second));
        let state = hub.lock();
        assert!(matches!(
            state.events[0].kind,
            EventKind::ProposalResolved {
                outcome: Outcome::Superseded,
                ..
            }
        ));
    }

    #[test]
    fn resolve_clears_the_proposal_and_notifies_the_ui() {
        let (hub, mut ui, store) = hub_with(open_policy());
        let proposal = propose(&hub, "c1", "hi");
        ui.try_recv().unwrap();
        assert!(hub.resolve(&proposal.id, Outcome::Sent));
        assert!(!hub.resolve(&proposal.id, Outcome::Sent));
        assert!(hub.pending_proposal("c1").is_none());
        assert!(store.pending_agent_proposals().unwrap().is_empty());
        assert_eq!(
            ui.try_recv().unwrap(),
            UiEvent::ProposalRemoved {
                conversation_id: "c1".to_owned()
            }
        );
    }

    #[test]
    fn user_sent_and_own_messages_make_the_proposal_stale() {
        let (hub, mut ui, _store) = hub_with(open_policy());
        let stale = propose(&hub, "c1", "hi");
        ui.try_recv().unwrap();
        hub.user_sent("c1");
        assert!(hub.pending_proposal("c1").is_none());
        assert_eq!(ui.try_recv().unwrap(), UiEvent::ProposalStale(stale));
        propose(&hub, "c2", "yo");
        hub.message_arrived("c2", "m1", false, Utc::now(), None, "other");
        assert!(hub.pending_proposal("c2").is_some());
        hub.message_arrived("c2", "m2", true, Utc::now(), None, "mine");
        assert!(hub.pending_proposal("c2").is_none());
    }

    #[test]
    fn restore_pending_reloads_unresolved_proposals() {
        let (hub, _ui, store) = hub_with(open_policy());
        let proposal = propose(&hub, "c1", "hi");
        let (sender, mut receiver) = unbounded_channel();
        let restored = AgentHub::new(store, open_policy(), sender);
        restored.restore_pending().unwrap();
        assert_eq!(restored.pending_proposal("c1"), Some(proposal.clone()));
        assert_eq!(
            receiver.try_recv().unwrap(),
            UiEvent::ProposalChanged(proposal)
        );
    }

    #[test]
    fn captions_update_partials_and_clear() {
        let (hub, _ui, _store) = hub_with(open_policy());
        hub.push_caption("c1", "1", "Ana", "Hel");
        hub.push_caption("c1", "1", "Ana", "Hello");
        hub.push_caption("c1", "2", "Bo", "Hi");
        let lines = hub.live_captions("c1");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Hello");
        assert!(hub.live_captions("c2").is_empty());
        hub.clear_captions("c1");
        assert!(hub.live_captions("c1").is_empty());
    }

    #[tokio::test]
    async fn waiting_without_a_cursor_only_sees_new_events() {
        let (hub, _ui, _store) = hub_with(open_policy());
        hub.user_typing("c1");
        let (events, next) = hub.wait_events(None, SHORT).await;
        assert!(events.is_empty());
        assert_eq!(next, 1);
        hub.user_typing("c1");
        let (events, next) = hub.wait_events(Some(next), SHORT).await;
        assert_eq!(events.len(), 1);
        assert_eq!(next, 2);
    }

    #[tokio::test]
    async fn a_waiter_wakes_when_an_event_arrives() {
        let (hub, _ui, _store) = hub_with(open_policy());
        let waiter = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.wait_events(None, Duration::from_secs(5)).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        hub.message_arrived(
            "c1",
            "m1",
            false,
            Utc::now(),
            Some("Ada".to_owned()),
            "hello",
        );
        let (events, next) = waiter.await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(next, events[0].seq);
    }

    #[tokio::test]
    async fn events_are_filtered_by_the_policy_at_read_time() {
        let policy = AgentPolicy {
            chats: Access::Selected(["c1".to_owned()].into()),
            channels: Access::None,
            ..open_policy()
        };
        let (hub, _ui, _store) = hub_with(policy);
        hub.user_typing("c1");
        hub.user_typing("c2");
        let (events, next) = hub.wait_events(Some(0), SHORT).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind.conversation_id(), "c1");
        assert_eq!(next, 2);
        hub.set_policy(AgentPolicy::default());
        assert!(hub.wait_events(Some(0), SHORT).await.0.is_empty());
    }

    #[test]
    fn the_ring_buffer_drops_the_oldest_events() {
        let (hub, _ui, _store) = hub_with(open_policy());
        for _ in 0..EVENT_CAPACITY + 5 {
            hub.user_typing("c1");
        }
        let state = hub.lock();
        assert_eq!(state.events.len(), EVENT_CAPACITY);
        assert_eq!(state.events[0].seq, 6);
        assert_eq!(state.last_seq, EVENT_CAPACITY as u64 + 5);
    }

    #[test]
    fn reaction_proposals_live_beside_the_message_proposal() {
        let (hub, mut ui, _store) = hub_with(open_policy());
        let proposal = propose(&hub, "c1", "hi");
        let first = react(&hub, "c1", "m1");
        let second = react(&hub, "c1", "m2");
        assert_eq!(hub.pending_proposal("c1"), Some(proposal.clone()));
        assert_eq!(hub.pending_reactions("c1").len(), 2);
        assert_eq!(ui.try_recv().unwrap(), UiEvent::ProposalChanged(proposal));
        assert_eq!(
            ui.try_recv().unwrap(),
            UiEvent::ReactionProposalChanged(first)
        );
        assert_eq!(
            ui.try_recv().unwrap(),
            UiEvent::ReactionProposalChanged(second)
        );
        assert!(hub.lock().events.is_empty());
    }

    #[test]
    fn a_second_reaction_on_the_same_message_replaces_the_first_as_superseded() {
        let (hub, _ui, store) = hub_with(open_policy());
        react(&hub, "c1", "m1");
        let second = react(&hub, "c1", "m1");
        assert_eq!(hub.pending_reactions("c1"), [second.clone()]);
        assert_eq!(store.pending_agent_reactions().unwrap().len(), 1);
        assert!(matches!(
            hub.lock().events[0].kind,
            EventKind::ReactionResolved {
                outcome: ReactionOutcome::Superseded,
                ..
            }
        ));
    }

    #[test]
    fn resolving_a_reaction_leaves_the_message_proposal_alone() {
        let (hub, mut ui, store) = hub_with(open_policy());
        let proposal = propose(&hub, "c1", "hi");
        let reaction = react(&hub, "c1", "m1");
        while ui.try_recv().is_ok() {}
        assert!(hub.resolve_reaction(&reaction.id, ReactionOutcome::Accepted));
        assert!(!hub.resolve_reaction(&reaction.id, ReactionOutcome::Accepted));
        assert_eq!(hub.pending_proposal("c1"), Some(proposal));
        assert!(store.pending_agent_reactions().unwrap().is_empty());
        assert_eq!(
            ui.try_recv().unwrap(),
            UiEvent::ReactionProposalRemoved {
                conversation_id: "c1".to_owned(),
                message_id: "m1".to_owned()
            }
        );
        assert!(matches!(
            hub.lock().events[0].kind,
            EventKind::ReactionResolved {
                outcome: ReactionOutcome::Accepted,
                ..
            }
        ));
    }

    #[test]
    fn restore_pending_reloads_reaction_proposals() {
        let (hub, _ui, store) = hub_with(open_policy());
        let reaction = react(&hub, "c1", "m1");
        let (sender, mut receiver) = unbounded_channel();
        let restored = AgentHub::new(store, open_policy(), sender);
        restored.restore_pending().unwrap();
        assert_eq!(restored.pending_reactions("c1"), [reaction.clone()]);
        assert_eq!(
            receiver.try_recv().unwrap(),
            UiEvent::ReactionProposalChanged(reaction)
        );
    }

    fn propose_with_image(hub: &AgentHub, conversation_id: &str) -> Proposal {
        hub.add_proposal(NewProposal {
            conversation_id: conversation_id.to_owned(),
            text: "look".to_owned(),
            images: vec![ProposalImage {
                name: "image-1.png".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: Arc::new(vec![1, 2, 3]),
            }],
            ..NewProposal::default()
        })
        .unwrap()
    }

    #[test]
    fn proposal_images_are_deleted_when_the_proposal_ends() {
        let (hub, _ui, store) = hub_with(open_policy());
        let count = |proposal: &Proposal| store.agent_proposal_image_count(&proposal.id).unwrap();

        let resolved = propose_with_image(&hub, "c1");
        assert_eq!(count(&resolved), 1);
        hub.resolve(&resolved.id, Outcome::Sent);
        assert_eq!(count(&resolved), 0);

        let superseded = propose_with_image(&hub, "c2");
        let current = propose_with_image(&hub, "c2");
        assert_eq!(count(&superseded), 0);
        assert_eq!(count(&current), 1);

        hub.user_sent("c2");
        assert_eq!(count(&current), 0);
    }

    #[test]
    fn the_restore_sweep_drops_images_of_proposals_that_are_not_pending() {
        let (hub, _ui, store) = hub_with(open_policy());
        let kept = propose_with_image(&hub, "c1");
        store
            .insert_agent_proposal(&AgentProposalRecord {
                id: "old".to_owned(),
                conversation_id: "c3".to_owned(),
                reply_to_message_id: None,
                text: "x".to_owned(),
                mentions: Vec::new(),
                images: vec![AttachmentImage {
                    name: "i".to_owned(),
                    format: "image/png".to_owned(),
                    bytes: vec![9],
                    width: None,
                    height: None,
                }],
                agent: None,
                session_id: None,
                created_at: now_millis(),
                state: "sent".to_owned(),
                resolved_at: None,
            })
            .unwrap();
        assert_eq!(store.agent_proposal_image_count("old").unwrap(), 1);
        let (sender, _receiver) = unbounded_channel();
        AgentHub::new(store.clone(), open_policy(), sender)
            .restore_pending()
            .unwrap();
        assert_eq!(store.agent_proposal_image_count(&kept.id).unwrap(), 1);
        assert_eq!(store.agent_proposal_image_count("old").unwrap(), 0);
    }
}
