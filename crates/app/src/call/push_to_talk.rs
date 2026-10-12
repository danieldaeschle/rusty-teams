use calling::MuteCommand;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushToTalk {
    held: bool,
}

impl PushToTalk {
    pub fn is_held(self) -> bool {
        self.held
    }

    pub fn press(&mut self, muted: bool, can_unmute: bool) -> Option<MuteCommand> {
        if self.held || !muted || !can_unmute {
            return None;
        }
        self.held = true;
        Some(MuteCommand::Unmute)
    }

    pub fn release(&mut self) -> Option<MuteCommand> {
        std::mem::take(&mut self.held).then_some(MuteCommand::Mute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressing_while_muted_unmutes_and_releasing_mutes_again() {
        let mut talk = PushToTalk::default();
        assert_eq!(talk.press(true, true), Some(MuteCommand::Unmute));
        assert!(talk.is_held());
        assert_eq!(talk.release(), Some(MuteCommand::Mute));
        assert!(!talk.is_held());
    }

    #[test]
    fn key_repeat_sends_one_unmute() {
        let mut talk = PushToTalk::default();
        assert!(talk.press(true, true).is_some());
        assert_eq!(talk.press(false, true), None);
        assert_eq!(talk.press(true, true), None);
    }

    #[test]
    fn pressing_while_unmuted_does_nothing_and_release_stays_quiet() {
        let mut talk = PushToTalk::default();
        assert_eq!(talk.press(false, true), None);
        assert_eq!(talk.release(), None);
    }

    #[test]
    fn listen_only_never_unmutes() {
        let mut talk = PushToTalk::default();
        assert_eq!(talk.press(true, false), None);
        assert_eq!(talk.release(), None);
    }
}
