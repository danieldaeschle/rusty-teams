use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use store::{AgentProposalRecord, Store};
use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedSender;

use crate::policy::AgentPolicy;

const EVENT_CAPACITY: usize = 1000;
const CAPTION_CAPACITY: usize = 2000;
const PENDING_STATE: &str = "pending";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub conversation_id: String,
    pub reply_to_message_id: Option<String>,
    pub text: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
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
    ProposalRemoved { conversation_id: String },
}

#[derive(Default)]
struct HubState {
    policy: AgentPolicy,
    pending: HashMap<String, Proposal>,
    events: VecDeque<Event>,
    last_seq: u64,
    captions: HashMap<String, VecDeque<CaptionLine>>,
}

pub struct AgentHub {
    state: Mutex<HubState>,
    wake: Notify,
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
            store,
            ui,
        })
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

    pub fn restore_pending(&self) -> store::Result<()> {
        let records = self.store.pending_agent_proposals()?;
        let mut state = self.lock();
        for record in records {
            let proposal = proposal_from_record(record);
            let _ = self.ui.send(UiEvent::ProposalChanged(proposal.clone()));
            state
                .pending
                .insert(proposal.conversation_id.clone(), proposal);
        }
        Ok(())
    }

    pub fn add_proposal(
        &self,
        conversation_id: &str,
        text: &str,
        reply_to_message_id: Option<String>,
        agent: Option<String>,
        session_id: Option<String>,
    ) -> store::Result<Proposal> {
        let proposal = Proposal {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: conversation_id.to_owned(),
            reply_to_message_id,
            text: text.to_owned(),
            agent,
            session_id,
            created_at: now_millis(),
        };
        self.store.insert_agent_proposal(&AgentProposalRecord {
            id: proposal.id.clone(),
            conversation_id: proposal.conversation_id.clone(),
            reply_to_message_id: proposal.reply_to_message_id.clone(),
            text: proposal.text.clone(),
            agent: proposal.agent.clone(),
            session_id: proposal.session_id.clone(),
            created_at: proposal.created_at,
            state: PENDING_STATE.to_owned(),
            resolved_at: None,
        })?;
        let mut state = self.lock();
        if let Some(previous) = state
            .pending
            .insert(conversation_id.to_owned(), proposal.clone())
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
        sender: Option<String>,
        preview: &str,
    ) {
        if sender_is_me {
            self.stale_pending(conversation_id);
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

    fn finish(&self, state: &mut HubState, proposal: &Proposal, outcome: Outcome) {
        let _ =
            self.store
                .set_agent_proposal_state(&proposal.id, outcome.state(), Some(now_millis()));
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

fn proposal_from_record(record: AgentProposalRecord) -> Proposal {
    Proposal {
        id: record.id,
        conversation_id: record.conversation_id,
        reply_to_message_id: record.reply_to_message_id,
        text: record.text,
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
        hub.add_proposal(conversation_id, text, None, None, None)
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
        hub.message_arrived("c2", "m1", false, None, "other");
        assert!(hub.pending_proposal("c2").is_some());
        hub.message_arrived("c2", "m2", true, None, "mine");
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
        hub.message_arrived("c1", "m1", false, Some("Ada".to_owned()), "hello");
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
}
