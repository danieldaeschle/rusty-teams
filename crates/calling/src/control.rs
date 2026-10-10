use std::future::Future;
use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::background::BackgroundChoice;
use crate::breakout::BreakoutMove;
use crate::captions::{CaptionEntry, CaptionState};
use crate::devices::{DeviceChoice, DeviceLists};
use crate::error::{Error, Result};
use crate::hold::HoldState;
use crate::mute::MuteCommand;
use crate::reaction::Reaction;
use crate::roster::RosterEntry;
use crate::camera::CameraDevice;
use crate::screen::ShareSource;
use crate::signaling::{Callee, MeetingTarget};
use crate::state::CallState;
use crate::video_frame::VideoHub;
use crate::whiteboard::ContentShare;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallCommand {
    Mute(MuteCommand),
    SelectInput(DeviceChoice),
    SelectOutput(DeviceChoice),
    RefreshDevices,
    SetCamera(bool),
    SelectCamera(DeviceChoice),
    StartShare(ShareSource),
    StopShare,
    SetShareSound(bool),
    RefreshShareSources,
    SetHand(bool),
    LowerHand { mri: String },
    LowerAllHands,
    Admit { mri: String },
    AdmitAll,
    Deny { mri: String },
    MuteParticipant { mri: String },
    MuteAll,
    Spotlight { mri: String },
    StopSpotlight { mri: String },
    RemoveParticipant { mri: String },
    SetCaptions(bool),
    SetBackground(BackgroundChoice),
    SendReaction(Reaction),
    SetRecording { on: bool, title: String },
    ConsentToRecording,
    Hold(bool),
    Transfer { target: Callee, replaces: Option<String> },
    OpenWhiteboard { title: String },
    Hangup,
    EndMeeting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Calling,
    Ringing,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallUpdate {
    State(CallState),
    Muted(bool),
    Levels { local: f32, remote: f32 },
    Devices(DeviceLists),
    Selected { input: DeviceChoice, output: DeviceChoice },
    ListenOnly(bool),
    Stats { inbound_packets: u64, outbound_packets: u64 },
    Progress(Progress),
    Roster(Vec<RosterEntry>),
    Speakers(Vec<String>),
    Lobby(bool),
    OwnIdentity { mri: String },
    VideoReady,
    ScreenShare(Option<String>),
    Camera(bool),
    Cameras(Vec<CameraDevice>),
    LocalShare(Option<String>),
    ShareSources(Vec<ShareSource>),
    ShareSound(bool),
    Reaction { mri: String, reaction: Reaction },
    Captions(CaptionState),
    Caption(CaptionEntry),
    BlurTiming(f32),
    Recording(bool),
    ConsentRequired(bool),
    Hold(HoldState),
    Whiteboard(Option<ContentShare>),
    WhiteboardUrl(String),
    BreakoutMove(BreakoutMove),
    BreakoutRoom { main: MeetingTarget },
    ReplacementLink(String),
    Organizer { action: String, outcome: std::result::Result<u16, String> },
    MeetingChat(String),
    Notice(String),
}

pub struct CallHandle {
    pub commands: UnboundedSender<CallCommand>,
    pub updates: UnboundedReceiver<CallUpdate>,
    pub video: Arc<VideoHub>,
}

pub struct CallControl {
    commands: UnboundedReceiver<CallCommand>,
    updates: UnboundedSender<CallUpdate>,
    deferred: Vec<CallCommand>,
    pub video: Arc<VideoHub>,
}

pub fn call_channel() -> (CallHandle, CallControl) {
    let (command_sender, command_receiver) = unbounded_channel();
    let (update_sender, update_receiver) = unbounded_channel();
    let video = VideoHub::new(update_sender.clone());
    (
        CallHandle {
            commands: command_sender,
            updates: update_receiver,
            video: video.clone(),
        },
        CallControl {
            commands: command_receiver,
            updates: update_sender,
            deferred: Vec::new(),
            video,
        },
    )
}

impl CallControl {
    pub fn send(&self, update: CallUpdate) {
        let _ = self.updates.send(update);
    }

    pub fn updates(&self) -> UnboundedSender<CallUpdate> {
        self.updates.clone()
    }

    pub fn try_recv_command(&mut self) -> Option<CallCommand> {
        if !self.deferred.is_empty() {
            return Some(self.deferred.remove(0));
        }
        self.commands.try_recv().ok()
    }

    pub async fn recv(&mut self) -> Option<CallCommand> {
        if !self.deferred.is_empty() {
            return Some(self.deferred.remove(0));
        }
        self.commands.recv().await
    }

    /// Runs a setup step; a hangup while it is pending cancels it, other commands wait for the call loop.
    pub async fn until_hangup<T>(&mut self, step: impl Future<Output = T>) -> Result<T> {
        let CallControl { commands, deferred, .. } = self;
        let hangup = async {
            loop {
                match commands.recv().await {
                    Some(CallCommand::Hangup) | None => return,
                    Some(other) => deferred.push(other),
                }
            }
        };
        tokio::select! {
            output = step => Ok(output),
            _ = hangup => Err(Error::Cancelled),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hangup_cancels_a_pending_step_and_keeps_other_commands() {
        let (handle, mut control) = call_channel();
        handle.commands.send(CallCommand::Mute(MuteCommand::Mute)).unwrap();
        handle.commands.send(CallCommand::Hangup).unwrap();
        let outcome = control.until_hangup(std::future::pending::<()>()).await;
        assert!(matches!(outcome, Err(Error::Cancelled)));
        assert_eq!(control.recv().await, Some(CallCommand::Mute(MuteCommand::Mute)));
    }

    #[tokio::test]
    async fn a_finished_step_returns_its_value() {
        let (_handle, mut control) = call_channel();
        assert_eq!(control.until_hangup(async { 7 }).await.unwrap(), 7);
    }
}
