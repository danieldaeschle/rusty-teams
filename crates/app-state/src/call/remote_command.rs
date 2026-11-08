use gpui_kit::*;

use super::join::JoinMode;
use crate::app_state::AppState;

pub const JOIN_FLAG: &str = "--join";
pub const HANG_UP_FLAG: &str = "--hang-up";
const JOIN_COMMAND: &str = "join ";
const HANG_UP_COMMAND: &str = "hang-up";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommand {
    Join(String),
    HangUp,
}

impl RemoteCommand {
    pub fn encode(&self) -> String {
        match self {
            RemoteCommand::Join(link) => format!("{JOIN_COMMAND}{link}"),
            RemoteCommand::HangUp => HANG_UP_COMMAND.to_owned(),
        }
    }

    pub fn decode(text: &str) -> Option<RemoteCommand> {
        if text == HANG_UP_COMMAND {
            return Some(RemoteCommand::HangUp);
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
        ] {
            assert_eq!(RemoteCommand::decode(&command.encode()), Some(command));
        }
        assert_eq!(RemoteCommand::decode("join  "), None);
        assert_eq!(RemoteCommand::decode("reboot"), None);
    }
}
