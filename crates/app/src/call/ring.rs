use std::time::Instant;

use calling::{IncomingRing, RingSignal, RingState};
use chrono::{DateTime, Utc};

const ORGID_PREFIX: &str = "8:orgid:";

#[derive(Debug, Clone)]
pub struct RingEntry {
    pub ring: IncomingRing,
    pub state: RingState,
    pub demo: bool,
}

impl RingEntry {
    pub fn caller_user_id(&self) -> Option<String> {
        self.ring.caller.mri.strip_prefix(ORGID_PREFIX).map(str::to_owned)
    }

    pub fn caller_name(&self) -> &str {
        &self.ring.caller.display_name
    }

    pub fn subtitle(&self) -> &'static str {
        match (self.ring.video, self.ring.is_group) {
            (true, _) => "Incoming call",
            (false, true) => "Incoming group audio call",
            (false, false) => "Incoming audio call",
        }
    }

    pub fn accept_label(&self) -> &'static str {
        if self.ring.video { "Accept with audio" } else { "Accept" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissedCall {
    pub caller_mri: String,
    pub caller_name: String,
    pub thread_id: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RingOutcome {
    Missed(MissedCall),
    Gone,
}

#[derive(Default)]
pub struct Rings {
    entries: Vec<RingEntry>,
}

impl Rings {
    pub fn push(&mut self, ring: IncomingRing, demo: bool, now: Instant) {
        if self.entries.iter().any(|entry| entry.ring.ring_id == ring.ring_id) {
            return;
        }
        self.entries.push(RingEntry {
            ring,
            state: RingState::start(now),
            demo,
        });
    }

    pub fn get(&self, ring_id: u64) -> Option<&RingEntry> {
        self.entries.iter().find(|entry| entry.ring.ring_id == ring_id)
    }

    pub fn ringing(&self) -> impl Iterator<Item = &RingEntry> {
        self.entries.iter().filter(|entry| entry.state.is_ringing())
    }

    pub fn is_ringing(&self) -> bool {
        self.ringing().next().is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn signal(&mut self, ring_id: u64, signal: RingSignal, now: Instant) -> Option<RingOutcome> {
        let index = self.entries.iter().position(|entry| entry.ring.ring_id == ring_id)?;
        let next = self.entries[index].state.next(signal, now);
        self.entries[index].state = next;
        self.settle(index)
    }

    pub fn tick(&mut self, now: Instant) -> Vec<(u64, RingOutcome)> {
        let mut outcomes = Vec::new();
        let ids: Vec<u64> = self.entries.iter().map(|entry| entry.ring.ring_id).collect();
        for ring_id in ids {
            if let Some(outcome) = self.signal(ring_id, RingSignal::Tick, now) {
                outcomes.push((ring_id, outcome));
            }
        }
        outcomes
    }

    fn settle(&mut self, index: usize) -> Option<RingOutcome> {
        let entry = &self.entries[index];
        if entry.state.is_ringing() {
            return None;
        }
        let outcome = if entry.state.is_missed() {
            RingOutcome::Missed(MissedCall {
                caller_mri: entry.ring.caller.mri.clone(),
                caller_name: entry.ring.caller.display_name.clone(),
                thread_id: entry.ring.thread_id.clone(),
                at: Utc::now(),
            })
        } else {
            RingOutcome::Gone
        };
        self.entries.remove(index);
        Some(outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use calling::{Caller, EndKind};

    use super::*;

    fn ring(ring_id: u64, video: bool) -> IncomingRing {
        IncomingRing {
            ring_id,
            caller: Caller {
                mri: "8:orgid:caller".into(),
                display_name: "Cara".into(),
            },
            thread_id: Some("19:a_b@unq.gbl.spaces".into()),
            video,
            is_group: false,
            subject: None,
        }
    }

    #[test]
    fn a_ring_stays_until_it_is_settled() {
        let start = Instant::now();
        let mut rings = Rings::default();
        rings.push(ring(1, false), false, start);
        assert!(rings.is_ringing());
        assert!(rings.tick(start + Duration::from_secs(5)).is_empty());
        assert!(rings.get(1).is_some());
    }

    #[test]
    fn thirty_seconds_without_an_answer_make_a_missed_call() {
        let start = Instant::now();
        let mut rings = Rings::default();
        rings.push(ring(1, false), false, start);
        let outcomes = rings.tick(start + Duration::from_secs(30));
        assert_eq!(outcomes.len(), 1);
        let (ring_id, RingOutcome::Missed(missed)) = &outcomes[0] else { panic!("not missed") };
        assert_eq!(*ring_id, 1);
        assert_eq!(missed.caller_name, "Cara");
        assert_eq!(missed.thread_id.as_deref(), Some("19:a_b@unq.gbl.spaces"));
        assert!(rings.is_empty() && !rings.is_ringing());
    }

    #[test]
    fn the_caller_hanging_up_is_missed_but_answering_elsewhere_is_not() {
        let start = Instant::now();
        let mut rings = Rings::default();
        rings.push(ring(1, false), false, start);
        rings.push(ring(2, false), false, start);
        let cancelled = rings.signal(1, RingSignal::Remote(EndKind::Cancelled), start);
        assert!(matches!(cancelled, Some(RingOutcome::Missed(_))));
        let elsewhere = rings.signal(2, RingSignal::Remote(EndKind::AnsweredElsewhere), start);
        assert_eq!(elsewhere, Some(RingOutcome::Gone));
    }

    #[test]
    fn accept_and_decline_leave_no_missed_call() {
        let start = Instant::now();
        let mut rings = Rings::default();
        rings.push(ring(1, false), false, start);
        rings.push(ring(2, false), false, start);
        assert_eq!(rings.signal(1, RingSignal::Accept, start), Some(RingOutcome::Gone));
        assert_eq!(rings.signal(2, RingSignal::Decline, start), Some(RingOutcome::Gone));
        assert!(rings.is_empty());
        assert_eq!(rings.signal(1, RingSignal::Accept, start), None);
    }

    #[test]
    fn a_ring_id_rings_only_once() {
        let start = Instant::now();
        let mut rings = Rings::default();
        rings.push(ring(1, false), false, start);
        rings.push(ring(1, false), false, start);
        assert_eq!(rings.ringing().count(), 1);
    }

    #[test]
    fn video_calls_offer_accept_with_audio() {
        let audio = RingEntry { ring: ring(1, false), state: RingState::start(Instant::now()), demo: false };
        let video = RingEntry { ring: ring(2, true), ..audio.clone() };
        assert_eq!(audio.subtitle(), "Incoming audio call");
        assert_eq!(audio.accept_label(), "Accept");
        assert_eq!(video.subtitle(), "Incoming call");
        assert_eq!(video.accept_label(), "Accept with audio");
        assert_eq!(audio.caller_user_id().as_deref(), Some("caller"));
    }

    #[test]
    fn group_rings_say_so() {
        let mut group = ring(3, false);
        group.is_group = true;
        let entry = RingEntry { ring: group, state: RingState::start(Instant::now()), demo: false };
        assert_eq!(entry.subtitle(), "Incoming group audio call");
    }
}
