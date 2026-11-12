#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalleeGate {
    callees: usize,
    failed: usize,
    joined: bool,
    media_connected: bool,
}

impl CalleeGate {
    pub fn new(callees: usize) -> Self {
        CalleeGate {
            callees,
            failed: 0,
            joined: false,
            media_connected: false,
        }
    }

    pub fn is_waiting(&self) -> bool {
        self.callees > 0 && !self.joined
    }

    pub fn callees_failed(&mut self, count: usize) -> bool {
        if !self.is_waiting() {
            return false;
        }
        self.failed += count;
        self.failed >= self.callees
    }

    pub fn media_connected(&mut self) -> bool {
        self.media_connected = true;
        !self.is_waiting()
    }

    pub fn callee_joined(&mut self) -> bool {
        let was_waiting = self.is_waiting();
        self.joined = true;
        was_waiting && self.media_connected
    }
}

pub fn roster_has_callee<'a>(
    participant_mris: impl IntoIterator<Item = &'a str>,
    own_mri: &str,
) -> bool {
    participant_mris.into_iter().any(|mri| mri != own_mri)
}

/// The Echo bot hangs up after playing the recording back; Teams then ends the test call.
#[derive(Debug, Default)]
pub struct EchoWatch {
    seen: bool,
}

impl EchoWatch {
    pub fn bot_left<'a>(&mut self, participant_mris: impl IntoIterator<Item = &'a str>) -> bool {
        if participant_mris
            .into_iter()
            .any(|mri| mri == crate::signaling::ECHO_BOT_MRI)
        {
            self.seen = true;
            return false;
        }
        self.seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roster_with_only_ourselves_has_no_callee() {
        assert!(!roster_has_callee(["8:orgid:me"], "8:orgid:me"));
        assert!(!roster_has_callee([], "8:orgid:me"));
    }

    #[test]
    fn a_roster_with_someone_else_has_the_callee() {
        assert!(roster_has_callee(
            ["8:orgid:me", "8:orgid:them"],
            "8:orgid:me"
        ));
    }

    #[test]
    fn media_waits_for_the_callee_when_required() {
        let mut gate = CalleeGate::new(1);
        assert!(!gate.media_connected());
        assert!(gate.is_waiting());
        assert!(gate.callee_joined());
        assert!(!gate.is_waiting());
        assert!(gate.media_connected());
    }

    #[test]
    fn a_callee_before_media_does_not_fire_early() {
        let mut gate = CalleeGate::new(1);
        assert!(!gate.callee_joined());
        assert!(gate.media_connected());
    }

    #[test]
    fn the_call_fails_only_when_every_callee_failed() {
        let mut gate = CalleeGate::new(3);
        assert!(!gate.callees_failed(1));
        assert!(gate.callees_failed(2));
        let mut joined = CalleeGate::new(1);
        joined.callee_joined();
        assert!(!joined.callees_failed(1));
        assert!(!CalleeGate::new(0).callees_failed(1));
    }

    #[test]
    fn an_unrequired_gate_never_waits() {
        let mut gate = CalleeGate::new(0);
        assert!(!gate.is_waiting());
        assert!(gate.media_connected());
        assert!(!gate.callee_joined());
    }

    #[test]
    fn the_echo_call_ends_only_after_the_bot_was_there_and_left() {
        let me = "8:orgid:me";
        let bot = crate::signaling::ECHO_BOT_MRI;
        let mut watch = EchoWatch::default();
        assert!(!watch.bot_left([me]));
        assert!(!watch.bot_left([me, bot]));
        assert!(!watch.bot_left([me, bot]));
        assert!(watch.bot_left([me]));
    }
}
