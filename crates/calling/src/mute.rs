#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteCommand {
    Mute,
    Unmute,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuteEffect {
    pub muted: bool,
    pub track_enabled: bool,
    pub endpoint_is_muted: bool,
}

impl MuteCommand {
    pub fn target(self, currently_muted: bool) -> bool {
        match self {
            MuteCommand::Mute => true,
            MuteCommand::Unmute => false,
            MuteCommand::Toggle => !currently_muted,
        }
    }
}

impl MuteEffect {
    pub fn for_muted(muted: bool) -> Self {
        MuteEffect {
            muted,
            track_enabled: !muted,
            endpoint_is_muted: muted,
        }
    }

    pub fn initial(is_meeting: bool) -> Self {
        MuteEffect::for_muted(is_meeting)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_map_to_a_target_state() {
        assert!(MuteCommand::Mute.target(false));
        assert!(MuteCommand::Mute.target(true));
        assert!(!MuteCommand::Unmute.target(true));
        assert!(MuteCommand::Toggle.target(false));
        assert!(!MuteCommand::Toggle.target(true));
    }

    #[test]
    fn muting_stops_the_track_and_tells_teams() {
        let muted = MuteEffect::for_muted(true);
        assert!(!muted.track_enabled);
        assert!(muted.endpoint_is_muted);
        let open = MuteEffect::for_muted(false);
        assert!(open.track_enabled);
        assert!(!open.endpoint_is_muted);
    }

    #[test]
    fn meetings_start_muted_and_other_calls_do_not() {
        let meeting = MuteEffect::initial(true);
        assert!(meeting.muted && !meeting.track_enabled && meeting.endpoint_is_muted);
        let direct = MuteEffect::initial(false);
        assert!(!direct.muted && direct.track_enabled && !direct.endpoint_is_muted);
    }
}
