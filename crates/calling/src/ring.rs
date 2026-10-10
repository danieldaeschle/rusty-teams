use std::time::{Duration, Instant};

use crate::end::EndKind;

pub const RING_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingEnd {
    CallerCancelled,
    AnsweredElsewhere,
    Other,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingState {
    Ringing { since: Instant },
    Accepted,
    Declined,
    Ended(RingEnd),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingSignal {
    Accept,
    Decline,
    Remote(EndKind),
    Tick,
}

impl RingState {
    pub fn start(now: Instant) -> RingState {
        RingState::Ringing { since: now }
    }

    pub fn next(self, signal: RingSignal, now: Instant) -> RingState {
        let RingState::Ringing { since } = self else {
            return self;
        };
        match signal {
            RingSignal::Accept => RingState::Accepted,
            RingSignal::Decline => RingState::Declined,
            RingSignal::Remote(EndKind::AnsweredElsewhere) => RingState::Ended(RingEnd::AnsweredElsewhere),
            RingSignal::Remote(EndKind::Cancelled) => RingState::Ended(RingEnd::CallerCancelled),
            RingSignal::Remote(_) => RingState::Ended(RingEnd::Other),
            RingSignal::Tick if now.saturating_duration_since(since) >= RING_TIMEOUT => {
                RingState::Ended(RingEnd::TimedOut)
            }
            RingSignal::Tick => self,
        }
    }

    pub fn is_ringing(&self) -> bool {
        matches!(self, RingState::Ringing { .. })
    }

    pub fn is_missed(&self) -> bool {
        matches!(self, RingState::Ended(RingEnd::CallerCancelled | RingEnd::TimedOut | RingEnd::Other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn ringing_until_something_happens() {
        let start = Instant::now();
        let state = RingState::start(start);
        assert!(state.is_ringing());
        assert_eq!(state.next(RingSignal::Tick, after(start, 29)), state);
    }

    #[test]
    fn accept_and_decline_settle_the_ring() {
        let start = Instant::now();
        let state = RingState::start(start);
        assert_eq!(state.next(RingSignal::Accept, start), RingState::Accepted);
        assert_eq!(state.next(RingSignal::Decline, start), RingState::Declined);
        assert!(!RingState::Accepted.is_ringing());
    }

    #[test]
    fn thirty_seconds_without_an_answer_time_out() {
        let start = Instant::now();
        let state = RingState::start(start).next(RingSignal::Tick, after(start, 30));
        assert_eq!(state, RingState::Ended(RingEnd::TimedOut));
        assert!(state.is_missed());
    }

    #[test]
    fn the_caller_hanging_up_is_a_missed_call() {
        let start = Instant::now();
        let state = RingState::start(start).next(RingSignal::Remote(EndKind::Cancelled), start);
        assert_eq!(state, RingState::Ended(RingEnd::CallerCancelled));
        assert!(state.is_missed());
    }

    #[test]
    fn answering_elsewhere_stops_the_ring_without_a_missed_call() {
        let start = Instant::now();
        let state = RingState::start(start).next(RingSignal::Remote(EndKind::AnsweredElsewhere), start);
        assert_eq!(state, RingState::Ended(RingEnd::AnsweredElsewhere));
        assert!(!state.is_missed());
    }

    #[test]
    fn any_other_remote_end_counts_as_missed() {
        let start = Instant::now();
        let state = RingState::start(start).next(RingSignal::Remote(EndKind::Normal), start);
        assert_eq!(state, RingState::Ended(RingEnd::Other));
        assert!(state.is_missed());
    }

    #[test]
    fn a_settled_ring_ignores_later_signals() {
        let start = Instant::now();
        let accepted = RingState::start(start).next(RingSignal::Accept, start);
        assert_eq!(accepted.next(RingSignal::Remote(EndKind::Cancelled), start), RingState::Accepted);
        assert_eq!(accepted.next(RingSignal::Tick, after(start, 60)), RingState::Accepted);
        let declined = RingState::start(start).next(RingSignal::Decline, start);
        assert_eq!(declined.next(RingSignal::Accept, start), RingState::Declined);
        assert!(!declined.is_missed());
    }
}
