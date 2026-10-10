#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPath {
    Pending,
    Direct,
    Mixer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    ApplyAnswer,
    Escalate,
    ApplyOnNewPeer,
    ApplyOnSamePeer,
    AnswerOnNewPeer,
    AnswerOnSamePeer,
    RejectGlare,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaNegotiator {
    path: MediaPath,
    own_offer_pending: bool,
    same_peer_offer_pending: bool,
}

impl Default for MediaNegotiator {
    fn default() -> Self {
        MediaNegotiator {
            path: MediaPath::Pending,
            own_offer_pending: false,
            same_peer_offer_pending: false,
        }
    }
}

impl MediaNegotiator {
    pub fn path(&self) -> MediaPath {
        self.path
    }

    pub fn answered_incoming(&mut self, from_mixer: bool) {
        self.path = if from_mixer { MediaPath::Mixer } else { MediaPath::Direct };
    }

    pub fn on_acceptance(&mut self, from_mixer: bool) -> MediaAction {
        match (self.path, from_mixer) {
            (MediaPath::Pending, _) => {
                self.path = if from_mixer { MediaPath::Mixer } else { MediaPath::Direct };
                MediaAction::ApplyAnswer
            }
            (MediaPath::Direct, true) if !self.own_offer_pending => {
                self.own_offer_pending = true;
                MediaAction::Escalate
            }
            _ => MediaAction::Ignore,
        }
    }

    pub fn begin_same_peer_offer(&mut self) -> bool {
        if self.own_offer_pending || self.same_peer_offer_pending {
            return false;
        }
        self.same_peer_offer_pending = true;
        true
    }

    pub fn same_peer_offer_failed(&mut self) {
        self.same_peer_offer_pending = false;
    }

    pub fn on_media_answer(&mut self) -> MediaAction {
        if std::mem::take(&mut self.same_peer_offer_pending) {
            return MediaAction::ApplyOnSamePeer;
        }
        if !self.own_offer_pending {
            return MediaAction::Ignore;
        }
        self.own_offer_pending = false;
        self.path = MediaPath::Mixer;
        MediaAction::ApplyOnNewPeer
    }

    pub fn on_renegotiation(&mut self, new_offer: bool, escalation: bool) -> MediaAction {
        if self.own_offer_pending || self.same_peer_offer_pending {
            return MediaAction::RejectGlare;
        }
        if escalation {
            self.path = MediaPath::Mixer;
        }
        if new_offer || escalation {
            MediaAction::AnswerOnNewPeer
        } else {
            MediaAction::AnswerOnSamePeer
        }
    }

    pub fn escalation_failed(&mut self) {
        self.own_offer_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_answer_decides_the_path() {
        let mut direct = MediaNegotiator::default();
        assert_eq!(direct.on_acceptance(false), MediaAction::ApplyAnswer);
        assert_eq!(direct.path(), MediaPath::Direct);
        let mut mixer = MediaNegotiator::default();
        assert_eq!(mixer.on_acceptance(true), MediaAction::ApplyAnswer);
        assert_eq!(mixer.path(), MediaPath::Mixer);
    }

    #[test]
    fn a_mixer_answer_during_a_direct_call_escalates_once() {
        let mut negotiator = MediaNegotiator::default();
        negotiator.on_acceptance(false);
        assert_eq!(negotiator.on_acceptance(true), MediaAction::Escalate);
        assert_eq!(negotiator.on_acceptance(true), MediaAction::Ignore);
        assert_eq!(negotiator.on_media_answer(), MediaAction::ApplyOnNewPeer);
        assert_eq!(negotiator.path(), MediaPath::Mixer);
        assert_eq!(negotiator.on_acceptance(true), MediaAction::Ignore);
    }

    #[test]
    fn later_acceptances_on_a_settled_path_are_ignored() {
        let mut mixer = MediaNegotiator::default();
        mixer.on_acceptance(true);
        assert_eq!(mixer.on_acceptance(true), MediaAction::Ignore);
        let mut direct = MediaNegotiator::default();
        direct.on_acceptance(false);
        assert_eq!(direct.on_acceptance(false), MediaAction::Ignore);
    }

    #[test]
    fn a_media_answer_without_our_offer_is_ignored() {
        let mut negotiator = MediaNegotiator::default();
        assert_eq!(negotiator.on_media_answer(), MediaAction::Ignore);
        negotiator.on_acceptance(false);
        assert_eq!(negotiator.on_media_answer(), MediaAction::Ignore);
    }

    #[test]
    fn an_incoming_escalation_gets_a_new_peer_and_moves_to_the_mixer() {
        let mut negotiator = MediaNegotiator::default();
        negotiator.answered_incoming(false);
        assert_eq!(negotiator.path(), MediaPath::Direct);
        assert_eq!(negotiator.on_renegotiation(true, true), MediaAction::AnswerOnNewPeer);
        assert_eq!(negotiator.path(), MediaPath::Mixer);
    }

    #[test]
    fn a_plain_renegotiation_stays_on_the_same_peer() {
        let mut negotiator = MediaNegotiator::default();
        negotiator.answered_incoming(true);
        assert_eq!(negotiator.on_renegotiation(false, false), MediaAction::AnswerOnSamePeer);
        assert_eq!(negotiator.on_renegotiation(true, false), MediaAction::AnswerOnNewPeer);
        assert_eq!(negotiator.path(), MediaPath::Mixer);
    }

    #[test]
    fn an_offer_on_the_same_peer_waits_for_its_answer_and_blocks_other_offers() {
        let mut negotiator = MediaNegotiator::default();
        negotiator.on_acceptance(false);
        assert!(negotiator.begin_same_peer_offer());
        assert!(!negotiator.begin_same_peer_offer());
        assert_eq!(negotiator.on_renegotiation(false, false), MediaAction::RejectGlare);
        assert_eq!(negotiator.on_media_answer(), MediaAction::ApplyOnSamePeer);
        assert_eq!(negotiator.on_media_answer(), MediaAction::Ignore);
        assert!(negotiator.begin_same_peer_offer());
        negotiator.same_peer_offer_failed();
        assert_eq!(negotiator.on_renegotiation(false, false), MediaAction::AnswerOnSamePeer);
    }

    #[test]
    fn two_offers_at_once_are_glare() {
        let mut negotiator = MediaNegotiator::default();
        negotiator.on_acceptance(false);
        negotiator.on_acceptance(true);
        assert_eq!(negotiator.on_renegotiation(true, true), MediaAction::RejectGlare);
        negotiator.escalation_failed();
        assert_eq!(negotiator.on_renegotiation(true, true), MediaAction::AnswerOnNewPeer);
    }
}
