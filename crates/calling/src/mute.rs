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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerMuteSync {
    pub muted: bool,
    pub cleared: bool,
    pub silence_without_signal: bool,
}

/// Applies an organizer mute locally. A local unmute while the flag is still set
/// stays cleared until `server_muted` drops, so the same roster update cannot mute again.
pub fn sync_server_mute(server_muted: bool, muted: bool, cleared: bool) -> ServerMuteSync {
    if !server_muted {
        return ServerMuteSync {
            muted,
            cleared: false,
            silence_without_signal: false,
        };
    }
    if cleared || muted {
        return ServerMuteSync {
            muted,
            cleared,
            silence_without_signal: false,
        };
    }
    ServerMuteSync {
        muted: true,
        cleared: false,
        silence_without_signal: true,
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
    fn an_organizer_mute_closes_the_mic_until_the_user_clears_it() {
        let applied = sync_server_mute(true, false, false);
        assert!(applied.silence_without_signal);
        assert!(applied.muted);
        assert!(!applied.cleared);
        let held = sync_server_mute(true, false, true);
        assert!(!held.silence_without_signal);
        assert!(!held.muted);
        assert!(held.cleared);
        let again = sync_server_mute(false, false, true);
        assert!(!again.cleared);
        let fresh = sync_server_mute(true, false, again.cleared);
        assert!(fresh.silence_without_signal);
    }

    #[test]
    fn meetings_start_muted_and_other_calls_do_not() {
        let meeting = MuteEffect::initial(true);
        assert!(meeting.muted && !meeting.track_enabled && meeting.endpoint_is_muted);
        let direct = MuteEffect::initial(false);
        assert!(!direct.muted && direct.track_enabled && !direct.endpoint_is_muted);
    }
}
