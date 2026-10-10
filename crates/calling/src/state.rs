use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    LocalHangup,
    Cancelled,
    RemoteEnded,
    Dropped,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallState {
    Idle,
    Connecting,
    Connected { since: Instant },
    Reconnecting { since: Instant },
    Ended { reason: EndReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallSignal {
    Dial,
    MediaConnected,
    MediaDisconnected,
    MediaFailed,
    ReconnectTimedOut,
    LocalLeave,
    RemoteEnd,
    Error(String),
}

impl CallState {
    pub fn next(&self, signal: CallSignal, now: Instant) -> CallState {
        use CallSignal::*;
        match (self, signal) {
            (CallState::Idle | CallState::Ended { .. }, Dial) => CallState::Connecting,
            (CallState::Ended { .. }, _) | (CallState::Idle, _) => self.clone(),
            (CallState::Connecting, MediaConnected) => CallState::Connected { since: now },
            (CallState::Reconnecting { since }, MediaConnected) => CallState::Connected { since: *since },
            (CallState::Connected { since }, MediaDisconnected) => CallState::Reconnecting { since: *since },
            (CallState::Connecting, LocalLeave) => ended(EndReason::Cancelled),
            (_, LocalLeave) => ended(EndReason::LocalHangup),
            (_, RemoteEnd) => ended(EndReason::RemoteEnded),
            (CallState::Reconnecting { .. }, ReconnectTimedOut | MediaFailed) => ended(EndReason::Dropped),
            (CallState::Connected { .. }, MediaFailed) => ended(EndReason::Dropped),
            (CallState::Connecting, MediaFailed) => ended(EndReason::Failed("connection failed".into())),
            (_, Error(message)) => ended(EndReason::Failed(message)),
            (_, _) => self.clone(),
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, CallState::Idle | CallState::Ended { .. })
    }

    pub fn started_at(&self) -> Option<Instant> {
        match self {
            CallState::Connected { since } | CallState::Reconnecting { since } => Some(*since),
            _ => None,
        }
    }
}

fn ended(reason: EndReason) -> CallState {
    CallState::Ended { reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(signals: &[CallSignal]) -> CallState {
        let start = Instant::now();
        signals
            .iter()
            .cloned()
            .fold(CallState::Idle, |state, signal| state.next(signal, start))
    }

    #[test]
    fn dial_connects_and_hangs_up() {
        let state = run(&[CallSignal::Dial, CallSignal::MediaConnected]);
        assert!(matches!(state, CallState::Connected { .. }));
        assert_eq!(
            run(&[CallSignal::Dial, CallSignal::MediaConnected, CallSignal::LocalLeave]),
            ended(EndReason::LocalHangup)
        );
    }

    #[test]
    fn leaving_while_connecting_is_a_cancel() {
        assert_eq!(run(&[CallSignal::Dial, CallSignal::LocalLeave]), ended(EndReason::Cancelled));
    }

    #[test]
    fn reconnecting_keeps_the_call_start_and_recovers() {
        let start = Instant::now();
        let connected = CallState::Idle
            .next(CallSignal::Dial, start)
            .next(CallSignal::MediaConnected, start);
        let later = start + std::time::Duration::from_secs(9);
        let reconnecting = connected.next(CallSignal::MediaDisconnected, later);
        assert_eq!(reconnecting, CallState::Reconnecting { since: start });
        assert_eq!(
            reconnecting.next(CallSignal::MediaConnected, later),
            CallState::Connected { since: start }
        );
    }

    #[test]
    fn reconnect_timeout_and_media_failure_drop_the_call() {
        let reconnecting = [CallSignal::Dial, CallSignal::MediaConnected, CallSignal::MediaDisconnected];
        let timed_out = [reconnecting.as_slice(), &[CallSignal::ReconnectTimedOut]].concat();
        assert_eq!(run(&timed_out), ended(EndReason::Dropped));
        let failed = [reconnecting.as_slice(), &[CallSignal::MediaFailed]].concat();
        assert_eq!(run(&failed), ended(EndReason::Dropped));
    }

    #[test]
    fn failing_before_connected_reports_a_failure() {
        assert_eq!(
            run(&[CallSignal::Dial, CallSignal::MediaFailed]),
            ended(EndReason::Failed("connection failed".into()))
        );
        assert_eq!(
            run(&[CallSignal::Dial, CallSignal::Error("boom".into())]),
            ended(EndReason::Failed("boom".into()))
        );
    }

    #[test]
    fn remote_end_wins_in_every_active_state() {
        assert_eq!(run(&[CallSignal::Dial, CallSignal::RemoteEnd]), ended(EndReason::RemoteEnded));
        assert_eq!(
            run(&[CallSignal::Dial, CallSignal::MediaConnected, CallSignal::RemoteEnd]),
            ended(EndReason::RemoteEnded)
        );
    }

    #[test]
    fn ended_is_final_until_the_next_dial() {
        let state = run(&[CallSignal::Dial, CallSignal::LocalLeave, CallSignal::MediaConnected, CallSignal::RemoteEnd]);
        assert_eq!(state, ended(EndReason::Cancelled));
        assert_eq!(state.next(CallSignal::Dial, Instant::now()), CallState::Connecting);
    }

    #[test]
    fn idle_ignores_everything_but_dial() {
        assert_eq!(run(&[CallSignal::MediaConnected, CallSignal::LocalLeave]), CallState::Idle);
    }
}
