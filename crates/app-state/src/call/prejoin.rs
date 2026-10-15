use calling::{CallCommand, CallSpec, MeetingTarget, MuteCommand};
use gpui_kit::Context;

use super::demo::{DemoCall, demo_guests};
use super::model::CallModel;
use crate::app_state::AppState;

const OPTIONS_META_KEY: &str = "prejoin_options";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioChoice {
    #[default]
    Computer,
    NoAudio,
}

impl AudioChoice {
    pub fn label(self) -> &'static str {
        match self {
            AudioChoice::Computer => "Computer audio",
            AudioChoice::NoAudio => "Don't use audio",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinOptions {
    pub camera_on: bool,
    pub mic_on: bool,
    pub audio: AudioChoice,
}

impl Default for JoinOptions {
    fn default() -> Self {
        JoinOptions {
            camera_on: false,
            mic_on: true,
            audio: AudioChoice::Computer,
        }
    }
}

impl JoinOptions {
    pub fn to_meta(&self) -> String {
        format!(
            "camera={}\nmic={}",
            u8::from(self.camera_on),
            u8::from(self.mic_on)
        )
    }

    pub fn from_meta(text: &str) -> Self {
        let mut options = JoinOptions::default();
        for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
            match key {
                "camera" => options.camera_on = value == "1",
                "mic" => options.mic_on = value == "1",
                _ => {}
            }
        }
        options
    }

    pub fn commands_after_join(&self) -> Vec<CallCommand> {
        if self.audio == AudioChoice::NoAudio {
            return vec![CallCommand::DisableAudio];
        }
        if self.mic_on {
            vec![CallCommand::Mute(MuteCommand::Unmute)]
        } else {
            Vec::new()
        }
    }
}

pub fn load_join_options(store: &store::Store) -> JoinOptions {
    store
        .meta(OPTIONS_META_KEY)
        .ok()
        .flatten()
        .map(|text| JoinOptions::from_meta(&text))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DemoMeeting {
    pub guests: usize,
    pub lobby: bool,
    pub organizer: bool,
}

#[derive(Debug, Clone)]
pub struct MeetingJoin {
    pub target: MeetingTarget,
    pub title: String,
    pub organizer: bool,
    pub conversation_id: Option<String>,
    pub demo: DemoMeeting,
}

#[derive(Debug, Clone)]
pub struct PendingJoin {
    pub id: u64,
    pub join: MeetingJoin,
}

impl AppState {
    pub(super) fn stage_meeting_join(&mut self, join: MeetingJoin, cx: &mut Context<Self>) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        self.prejoin_count += 1;
        self.prejoin = Some(PendingJoin {
            id: self.prejoin_count,
            join,
        });
        self.new_chat = false;
        cx.notify();
    }

    pub fn confirm_prejoin(&mut self, options: JoinOptions, cx: &mut Context<Self>) {
        let Some(pending) = self.prejoin.take() else {
            return;
        };
        let _ = self.store.set_meta(OPTIONS_META_KEY, &options.to_meta());
        self.join_options = options.clone();
        self.start_meeting(pending.join, Some(&options), cx);
    }

    pub fn cancel_prejoin(&mut self, cx: &mut Context<Self>) {
        if self.prejoin.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn start_meeting(
        &mut self,
        join: MeetingJoin,
        options: Option<&JoinOptions>,
        cx: &mut Context<Self>,
    ) {
        if self.call.is_some() {
            self.show_call(true, cx);
            return;
        }
        let demo = DemoCall::Meeting {
            guests: demo_guests(join.demo.guests),
            lobby: join.demo.lobby,
            organizer: join.demo.organizer,
        };
        let with_video = options.is_some_and(|options| options.camera_on);
        let Some(handle) =
            self.place_call(CallSpec::Meeting(join.target.clone()), demo, with_video, cx)
        else {
            return;
        };
        let model = CallModel::meeting(&join.title, join.organizer);
        self.begin_call(handle, model, join.conversation_id, cx);
        if let Some(call) = self.call.as_mut() {
            call.meeting_target = Some(join.target);
            call.model.audio_off =
                options.is_some_and(|options| options.audio == AudioChoice::NoAudio);
        }
        for command in options
            .map(JoinOptions::commands_after_join)
            .unwrap_or_default()
        {
            self.send_call_command(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_survive_a_round_trip_through_the_store_text() {
        let options = JoinOptions {
            camera_on: true,
            mic_on: false,
            audio: AudioChoice::Computer,
        };
        assert_eq!(JoinOptions::from_meta(&options.to_meta()), options);
    }

    #[test]
    fn empty_or_garbage_text_gives_the_defaults() {
        assert_eq!(JoinOptions::from_meta(""), JoinOptions::default());
        assert_eq!(JoinOptions::from_meta("junk\nmic"), JoinOptions::default());
    }

    #[test]
    fn computer_audio_unmutes_only_when_the_mic_is_on() {
        let default = JoinOptions::default();
        assert_eq!(
            default.commands_after_join(),
            vec![CallCommand::Mute(MuteCommand::Unmute)]
        );
        let muted = JoinOptions {
            mic_on: false,
            ..default
        };
        assert_eq!(muted.commands_after_join(), Vec::<CallCommand>::new());
    }

    #[test]
    fn without_audio_only_the_audio_is_switched_off() {
        let options = JoinOptions {
            audio: AudioChoice::NoAudio,
            ..JoinOptions::default()
        };
        assert_eq!(
            options.commands_after_join(),
            vec![CallCommand::DisableAudio]
        );
    }
}
