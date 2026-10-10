use std::time::{Duration, Instant};

use calling::{
    CallCommand, CallState, CallUpdate, DeviceChoice, DeviceLists, EndReason, SpeakingDetector,
};
use tokio::sync::mpsc::UnboundedSender;

pub const TEST_CALL_TITLE: &str = "Test call";

pub struct ActiveCall {
    pub id: u64,
    pub model: CallModel,
    pub commands: UnboundedSender<CallCommand>,
    pub viewing: bool,
}

#[derive(Debug, Clone)]
pub struct CallModel {
    pub title: String,
    pub state: CallState,
    pub muted: bool,
    pub listen_only: bool,
    pub input: DeviceChoice,
    pub output: DeviceChoice,
    pub devices: DeviceLists,
    pub local_speaking: bool,
    pub remote_speaking: bool,
    pub connected_since: Option<Instant>,
    local_detector: SpeakingDetector,
    remote_detector: SpeakingDetector,
}

impl CallModel {
    pub fn new(title: &str) -> Self {
        CallModel {
            title: title.to_owned(),
            state: CallState::Connecting,
            muted: false,
            listen_only: false,
            input: DeviceChoice::SystemDefault,
            output: DeviceChoice::SystemDefault,
            devices: DeviceLists::default(),
            local_speaking: false,
            remote_speaking: false,
            connected_since: None,
            local_detector: SpeakingDetector::default(),
            remote_detector: SpeakingDetector::default(),
        }
    }

    pub fn apply(&mut self, update: CallUpdate) {
        match update {
            CallUpdate::State(state) => {
                if let Some(since) = state.started_at() {
                    self.connected_since = Some(since);
                }
                if !matches!(state, CallState::Connected { .. }) {
                    self.silence();
                }
                self.state = state;
            }
            CallUpdate::Muted(muted) => {
                self.muted = muted;
                if muted {
                    self.local_speaking = false;
                    self.local_detector.reset();
                }
            }
            CallUpdate::Levels { local, remote } => {
                self.local_speaking = !self.muted && self.local_detector.update(local);
                self.remote_speaking = self.remote_detector.update(remote);
            }
            CallUpdate::Devices(devices) => self.devices = devices,
            CallUpdate::Selected { input, output } => {
                self.input = input;
                self.output = output;
            }
            CallUpdate::ListenOnly(listen_only) => self.listen_only = listen_only,
            CallUpdate::Stats { .. } => {}
        }
    }

    fn silence(&mut self) {
        self.local_speaking = false;
        self.remote_speaking = false;
        self.local_detector.reset();
        self.remote_detector.reset();
    }

    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.connected_since
            .map(|since| now.saturating_duration_since(since))
    }

    pub fn is_connecting(&self) -> bool {
        matches!(self.state, CallState::Connecting)
    }

    pub fn is_reconnecting(&self) -> bool {
        matches!(self.state, CallState::Reconnecting { .. })
    }

    pub fn ended_reason(&self) -> Option<&EndReason> {
        match &self.state {
            CallState::Ended { reason } => Some(reason),
            _ => None,
        }
    }

    pub fn timer_text(&self, now: Instant) -> String {
        self.elapsed(now).map(format_elapsed).unwrap_or_default()
    }

    pub fn can_unmute(&self) -> bool {
        !self.listen_only
    }
}

pub fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub fn ended_notice(reason: &EndReason, elapsed: Option<Duration>) -> String {
    match (reason, elapsed) {
        (EndReason::Cancelled, _) => "Call cancelled".to_owned(),
        (EndReason::Dropped, _) => "Call dropped".to_owned(),
        (EndReason::Failed(message), _) => format!("Test call failed: {message}"),
        (EndReason::LocalHangup | EndReason::RemoteEnded, Some(elapsed)) => {
            format!("Call ended {}", format_elapsed(elapsed))
        }
        (EndReason::LocalHangup | EndReason::RemoteEnded, None) => "Call ended".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected_model() -> CallModel {
        let mut model = CallModel::new(TEST_CALL_TITLE);
        model.apply(CallUpdate::State(CallState::Connected {
            since: Instant::now(),
        }));
        model
    }

    #[test]
    fn elapsed_text_is_minutes_and_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(42)), "00:42");
        assert_eq!(format_elapsed(Duration::from_secs(61)), "01:01");
        assert_eq!(format_elapsed(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn levels_drive_the_speaking_rings() {
        let mut model = connected_model();
        model.apply(CallUpdate::Levels {
            local: 0.0,
            remote: 0.4,
        });
        assert!(model.remote_speaking);
        assert!(!model.local_speaking);
        model.apply(CallUpdate::Levels {
            local: 0.4,
            remote: 0.0,
        });
        assert!(model.local_speaking);
    }

    #[test]
    fn muting_switches_the_local_ring_off_at_once() {
        let mut model = connected_model();
        model.apply(CallUpdate::Levels {
            local: 0.5,
            remote: 0.0,
        });
        assert!(model.local_speaking);
        model.apply(CallUpdate::Muted(true));
        assert!(!model.local_speaking);
        model.apply(CallUpdate::Levels {
            local: 0.5,
            remote: 0.0,
        });
        assert!(!model.local_speaking);
    }

    #[test]
    fn reconnecting_keeps_the_timer_and_clears_the_rings() {
        let mut model = connected_model();
        let since = model.connected_since.unwrap();
        model.apply(CallUpdate::Levels {
            local: 0.5,
            remote: 0.5,
        });
        model.apply(CallUpdate::State(CallState::Reconnecting { since }));
        assert!(model.is_reconnecting());
        assert_eq!(model.connected_since, Some(since));
        assert!(!model.remote_speaking && !model.local_speaking);
    }

    #[test]
    fn notices_for_every_end_reason() {
        let elapsed = Some(Duration::from_secs(42));
        assert_eq!(ended_notice(&EndReason::LocalHangup, elapsed), "Call ended 00:42");
        assert_eq!(ended_notice(&EndReason::RemoteEnded, elapsed), "Call ended 00:42");
        assert_eq!(ended_notice(&EndReason::Cancelled, None), "Call cancelled");
        assert_eq!(ended_notice(&EndReason::Dropped, elapsed), "Call dropped");
        assert_eq!(
            ended_notice(&EndReason::Failed("no route".into()), None),
            "Test call failed: no route"
        );
    }

    #[test]
    fn listen_only_blocks_unmuting() {
        let mut model = connected_model();
        assert!(model.can_unmute());
        model.apply(CallUpdate::ListenOnly(true));
        assert!(!model.can_unmute());
    }
}
