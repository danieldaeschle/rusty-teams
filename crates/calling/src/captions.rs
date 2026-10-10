use serde_json::{Value, json};

use crate::roster::CaptionBot;
use crate::trouter_events::CallbackLinks;

pub const CAPTIONS_DATA_ID: u8 = 3;
pub const RECORDING_BOT_MRI: &str = "28:bdd75849-e0a6-4cce-8fc1-d7c0d4da43e5";
pub const RECORDER_RESOURCE: &str = "4580fd1d-e5a3-4f56-9ad1-aab0e3bf8f76";
pub const RECORDER_SCOPE: &str = "access_recorder_service";
pub const INITIATOR_TOKEN_PLACEHOLDER: &str = "@@recorder-token@@";
pub const COMMAND_SCRIPT: &str = include_str!("../assets/caption_command.js");
const BOT_MODE: &str = "RecordingAndTranscription";
const COMMAND_MODE: &str = "transcription";
const CLOSED_CAPTIONS: &str = "closedCaptions";
const RECOGNITION_RESULTS: &str = "recognitionResults";
const TEXT_TRACKS: &str = "textTracks";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionEntry {
    pub id: String,
    pub user_id: String,
    pub display_name: String,
    pub text: String,
    pub is_final: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptionState {
    Off,
    Starting,
    On,
    Failed(String),
}

pub fn parse_caption_message(payload: &[u8]) -> Vec<CaptionEntry> {
    let Ok(message) = serde_json::from_slice::<Value>(payload) else {
        return Vec::new();
    };
    [RECOGNITION_RESULTS, TEXT_TRACKS]
        .into_iter()
        .filter_map(|key| message[key].as_array())
        .flatten()
        .filter_map(entry_from)
        .collect()
}

fn entry_from(item: &Value) -> Option<CaptionEntry> {
    let text = item["text"].as_str().or_else(|| item["rawText"].as_str())?.trim();
    if text.is_empty() {
        return None;
    }
    let user_id = item["userId"].as_str().unwrap_or_default().to_owned();
    let id = item["utteranceId"].as_str().or_else(|| item["id"].as_str()).map_or_else(|| user_id.clone(), str::to_owned);
    Some(CaptionEntry {
        id,
        user_id,
        display_name: item["displayName"].as_str().unwrap_or_default().to_owned(),
        text: text.to_owned(),
        is_final: item["isFinal"].as_bool().unwrap_or(false),
    })
}

pub fn add_bot_body(from: Value, thread_id: &str, call_id: &str, organizer_name: &str, callbacks: &CallbackLinks) -> Value {
    json!({
        "disableUnmute": false,
        "participants": {
            "from": from,
            "to": [{"id": RECORDING_BOT_MRI, "participantId": uuid::Uuid::new_v4().to_string()}],
        },
        "participantInvitationData": {"botData": {
            "meetingTitle": "",
            "clientInfo": "Teams-R4",
            "callId": call_id,
            "threadId": thread_id,
            "recorderFeatures": {
                "enablePPTSharing": true,
                "intermediateLiveCaptions": false,
                "actionItemsEnabled": false,
                "enableEmailAndMeetingLanguageModel": true,
                "ceoSummit": false,
                "useUnmixedAudio": true,
                "enableTranscriptMeetingChaptering": false,
            },
            "mode": BOT_MODE,
            "iCalUid": null,
            "consumerType": "Teams",
            "spokenLanguage": "",
            "initiatorUserToken": INITIATOR_TOKEN_PLACEHOLDER,
            "exchangeId": null,
            "meetingOrganizer": organizer_name,
        }},
        "replacementDetails": null,
        "groupContext": null,
        "groupChat": {"threadId": thread_id, "messageId": null},
        "links": {
            "addParticipantSuccess": callbacks.conversation("addParticipantSuccess"),
            "addParticipantFailure": callbacks.conversation("addParticipantFailure"),
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotAction {
    Start,
    Stop,
}

impl BotAction {
    fn name(self) -> &'static str {
        match self {
            BotAction::Start => "start",
            BotAction::Stop => "stop",
        }
    }
}

/// The skype token is added inside the page, so the body travels without it.
pub fn command_body(action: BotAction, own_mri: &str, participant_leg_id: &str, timestamp: &str) -> Value {
    json!({
        "timestamp": timestamp,
        "participantMri": own_mri,
        "participantLegId": participant_leg_id,
        "action": action.name(),
        "mode": COMMAND_MODE,
        "processingModes": [CLOSED_CAPTIONS],
    })
}

pub fn endpoint_metadata(captions: bool) -> Value {
    json!({"holographicCapabilities": 3, "transcriptionPrefs": {"closedCaptions": captions}})
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionStep {
    AddBot,
    Start,
    Stop,
    ReportOn,
    Wait,
}

#[derive(Default)]
pub struct CaptionFlow {
    wanted: bool,
    bot_requested: bool,
    started: bool,
    reported: bool,
}

impl CaptionFlow {
    pub fn want(&mut self, wanted: bool) {
        self.wanted = wanted;
        if !wanted {
            self.reported = false;
        }
    }

    pub fn wanted(&self) -> bool {
        self.wanted
    }

    pub fn on(&self) -> bool {
        self.reported
    }

    pub fn text_arrived(&mut self) -> bool {
        self.wanted && self.started && !std::mem::replace(&mut self.reported, true)
    }

    pub fn failed(&mut self) {
        *self = CaptionFlow::default();
    }

    pub fn next(&mut self, bot: Option<&CaptionBot>) -> CaptionStep {
        if !self.wanted {
            return if std::mem::take(&mut self.started) { CaptionStep::Stop } else { CaptionStep::Wait };
        }
        let Some(bot) = bot else {
            return if std::mem::replace(&mut self.bot_requested, true) { CaptionStep::Wait } else { CaptionStep::AddBot };
        };
        if bot.command_url.is_some() && !std::mem::replace(&mut self.started, true) {
            return CaptionStep::Start;
        }
        if self.started && bot.active && !std::mem::replace(&mut self.reported, true) {
            return CaptionStep::ReportOn;
        }
        CaptionStep::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bot(command_url: bool, active: bool) -> CaptionBot {
        CaptionBot { mri: RECORDING_BOT_MRI.into(), command_url: command_url.then(|| "https://x/v2/oncommand/1".into()), active }
    }

    #[test]
    fn recognition_results_become_entries_with_speaker_and_finality() {
        let message = json!({"recognitionResults": [
            {"id": "u1", "text": " Hello there ", "isFinal": false, "userId": "8:orgid:ana", "displayName": "Ana"},
            {"id": "u2", "text": "", "userId": "8:orgid:bo"},
            {"utteranceId": "u3", "rawText": "raw", "isFinal": true, "userId": "8:orgid:bo"},
        ]});
        let entries = parse_caption_message(message.to_string().as_bytes());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], CaptionEntry { id: "u1".into(), user_id: "8:orgid:ana".into(), display_name: "Ana".into(), text: "Hello there".into(), is_final: false });
        assert_eq!(entries[1].id, "u3");
        assert!(entries[1].is_final);
        assert_eq!(entries[1].display_name, "");
    }

    #[test]
    fn text_tracks_count_and_other_messages_and_bad_json_do_not() {
        let tracks = json!({"textTracks": [{"id": "t", "text": "from a track", "userId": "x", "isFinal": true}]});
        assert_eq!(parse_caption_message(tracks.to_string().as_bytes()).len(), 1);
        assert!(parse_caption_message(br#"{"events": [{"type": "x"}]}"#).is_empty());
        assert!(parse_caption_message(b"not json").is_empty());
        assert!(parse_caption_message(&[]).is_empty());
    }

    #[test]
    fn the_bot_invitation_carries_the_meeting_and_a_token_placeholder() {
        let callbacks = CallbackLinks::new("https://trouter.example/v4/f/x/", "call-1");
        let body = add_bot_body(json!({"id": "8:orgid:me"}), "19:meeting_x@thread.v2", "call-1", "Me", &callbacks);
        let bot_data = &body["participantInvitationData"]["botData"];
        assert_eq!(body["participants"]["to"][0]["id"], RECORDING_BOT_MRI);
        assert_eq!(bot_data["mode"], "RecordingAndTranscription");
        assert_eq!(bot_data["threadId"], "19:meeting_x@thread.v2");
        assert_eq!(bot_data["callId"], "call-1");
        assert_eq!(bot_data["initiatorUserToken"], INITIATOR_TOKEN_PLACEHOLDER);
        assert_eq!(body["groupChat"]["threadId"], "19:meeting_x@thread.v2");
        assert!(body["links"]["addParticipantFailure"].as_str().unwrap().contains("/conversation/addParticipantFailure/"));
    }

    #[test]
    fn start_and_stop_commands_differ_only_in_the_action() {
        let start = command_body(BotAction::Start, "8:orgid:me", "leg-1", "2026-10-10T08:44:12.705Z");
        let stop = command_body(BotAction::Stop, "8:orgid:me", "leg-1", "2026-10-10T08:44:12.705Z");
        assert_eq!(start["action"], "start");
        assert_eq!(stop["action"], "stop");
        assert_eq!(start["mode"], "transcription");
        assert_eq!(start["processingModes"], json!(["closedCaptions"]));
        assert_eq!(start["participantLegId"], "leg-1");
        assert!(start.get("participantSkypeToken").is_none());
        assert_eq!(start["timestamp"], stop["timestamp"]);
    }

    #[test]
    fn the_endpoint_metadata_switches_the_caption_preference() {
        assert_eq!(endpoint_metadata(true)["transcriptionPrefs"]["closedCaptions"], true);
        assert_eq!(endpoint_metadata(false)["transcriptionPrefs"]["closedCaptions"], false);
        assert_eq!(endpoint_metadata(true)["holographicCapabilities"], 3);
    }

    #[test]
    fn the_flow_adds_the_bot_once_then_starts_then_reports_then_stops() {
        let mut flow = CaptionFlow::default();
        assert_eq!(flow.next(None), CaptionStep::Wait);
        flow.want(true);
        assert_eq!(flow.next(None), CaptionStep::AddBot);
        assert_eq!(flow.next(None), CaptionStep::Wait);
        assert_eq!(flow.next(Some(&bot(false, false))), CaptionStep::Wait);
        assert_eq!(flow.next(Some(&bot(true, false))), CaptionStep::Start);
        assert_eq!(flow.next(Some(&bot(true, false))), CaptionStep::Wait);
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::ReportOn);
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::Wait);
        flow.want(false);
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::Stop);
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::Wait);
    }

    #[test]
    fn turning_captions_on_again_reuses_the_bot_in_the_roster() {
        let mut flow = CaptionFlow::default();
        flow.want(true);
        assert_eq!(flow.next(Some(&bot(true, false))), CaptionStep::Start);
        flow.want(false);
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::Stop);
        flow.want(true);
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::Start);
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::ReportOn);
    }

    #[test]
    fn caption_text_proves_the_captions_are_on_even_before_the_roster_says_so() {
        let mut flow = CaptionFlow::default();
        assert!(!flow.text_arrived());
        flow.want(true);
        assert_eq!(flow.next(Some(&bot(true, false))), CaptionStep::Start);
        assert!(flow.text_arrived());
        assert!(!flow.text_arrived());
        assert_eq!(flow.next(Some(&bot(true, true))), CaptionStep::Wait);
        flow.want(false);
        assert!(!flow.text_arrived());
    }

    #[test]
    fn a_failed_invitation_allows_another_try() {
        let mut flow = CaptionFlow::default();
        flow.want(true);
        assert_eq!(flow.next(None), CaptionStep::AddBot);
        flow.failed();
        assert!(!flow.wanted());
        flow.want(true);
        assert_eq!(flow.next(None), CaptionStep::AddBot);
    }
}
