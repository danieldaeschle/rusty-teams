use calling::{CallCommand, MuteCommand};
use gpui_kit::*;

use super::join::JoinMode;
use crate::app_state::AppState;

pub const JOIN_FLAG: &str = "--join";
pub const HANG_UP_FLAG: &str = "--hang-up";
pub const ADMIT_ALL_FLAG: &str = "--admit-all";
pub const UNMUTE_FLAG: &str = "--unmute";
const JOIN_COMMAND: &str = "join ";
const HANG_UP_COMMAND: &str = "hang-up";
const ADMIT_ALL_COMMAND: &str = "admit-all";
const UNMUTE_COMMAND: &str = "unmute";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommand {
    Join(String),
    HangUp,
    AdmitAll,
    Unmute,
}

impl RemoteCommand {
    pub fn encode(&self) -> String {
        match self {
            RemoteCommand::Join(link) => format!("{JOIN_COMMAND}{link}"),
            RemoteCommand::HangUp => HANG_UP_COMMAND.to_owned(),
            RemoteCommand::AdmitAll => ADMIT_ALL_COMMAND.to_owned(),
            RemoteCommand::Unmute => UNMUTE_COMMAND.to_owned(),
        }
    }

    pub fn decode(text: &str) -> Option<RemoteCommand> {
        match text {
            HANG_UP_COMMAND => return Some(RemoteCommand::HangUp),
            ADMIT_ALL_COMMAND => return Some(RemoteCommand::AdmitAll),
            UNMUTE_COMMAND => return Some(RemoteCommand::Unmute),
            _ => {}
        }
        let link = text.strip_prefix(JOIN_COMMAND)?.trim();
        (!link.is_empty()).then(|| RemoteCommand::Join(link.to_owned()))
    }
}

impl AppState {
    /// Commands a second `teams` process forwards, used by the call end-to-end test.
    pub fn run_remote_command(&mut self, text: &str, cx: &mut Context<Self>) {
        match RemoteCommand::decode(text) {
            Some(RemoteCommand::Join(link)) => {
                self.own_engine_next_call = true;
                self.join_link(&link, JoinMode::Direct, cx);
                self.own_engine_next_call = false;
            }
            Some(RemoteCommand::HangUp) => self.leave_call(cx),
            Some(RemoteCommand::AdmitAll) => self.admit_all_call_guests(cx),
            Some(RemoteCommand::Unmute) => self.send_call_command(CallCommand::Mute(MuteCommand::Unmute)),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteCommand;

    #[test]
    fn commands_survive_the_trip_through_the_pipe() {
        for command in [
            RemoteCommand::Join("https://teams.microsoft.com/l/meetup-join/x".to_owned()),
            RemoteCommand::HangUp,
            RemoteCommand::AdmitAll,
            RemoteCommand::Unmute,
        ] {
            assert_eq!(RemoteCommand::decode(&command.encode()), Some(command));
        }
        assert_eq!(RemoteCommand::decode("join  "), None);
        assert_eq!(RemoteCommand::decode("reboot"), None);
    }
}
