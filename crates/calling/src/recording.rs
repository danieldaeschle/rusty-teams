use serde_json::{Value, json};

use crate::captions::recorder_features;
use crate::trouter_events::find_key;

const CLOUD_RECORDING: &str = "cloudRecording";
const TRANSCRIPTION: &str = "transcription";
const CONSENT_DETAILS: &str = "recordingConsentDetails";
const ACTIVE: &str = "Active";
const RECORDING_AND_TRANSCRIPT: [&str; 2] = ["recording", "realTimeTranscript"];
const RECORDING_ONLY: [&str; 1] = ["recording"];
const TRANSCRIPT_ONLY: [&str; 1] = ["realTimeTranscript"];
const TRANSCRIPTION_MODE: &str = "transcription";
const RECORDING_MODE_KEY: &str = "recording";
const STORAGE_TYPE: &str = "OnedriveForBusiness";
const STORAGE_LOCATION: &str = "Recordings";
const VIDEO_MODE: &str = "Normal";
const AUDIO_ONLY_MODE: &str = "AudioOnly";
const FILE_SUFFIX: &str = "Meeting Recording";
const CONSENT_STATE: &str = "consentToRecording";
const DENY_STATE: &str = "denyConsentToRecording";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateChange {
    pub recording: Option<bool>,
    pub transcription: Option<bool>,
    pub consent_required: Option<bool>,
}

impl StateChange {
    pub fn is_empty(&self) -> bool {
        self.recording.is_none() && self.transcription.is_none() && self.consent_required.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecordingStatus {
    active: bool,
    transcribing: bool,
    consent_required: bool,
    sequence: u64,
    transcription_sequence: u64,
}

impl RecordingStatus {
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn transcribing(&self) -> bool {
        self.transcribing
    }

    pub fn consent_required(&self) -> bool {
        self.consent_required
    }

    fn newer_state(body: &Value, key: &str, last_sequence: &mut u64) -> Option<bool> {
        let state = find_key(body, key)?;
        let sequence = state["sequenceNumber"].as_u64().unwrap_or_default();
        let current = state["value"]["State"].as_str().map(|name| name == ACTIVE)?;
        (sequence >= *last_sequence).then(|| {
            *last_sequence = sequence;
            current
        })
    }

    pub fn apply(&mut self, body: &Value) -> StateChange {
        let mut change = StateChange::default();
        if let Some(current) = Self::newer_state(body, CLOUD_RECORDING, &mut self.sequence)
            && current != self.active
        {
            self.active = current;
            change.recording = Some(current);
        }
        if let Some(current) = Self::newer_state(body, TRANSCRIPTION, &mut self.transcription_sequence)
            && current != self.transcribing
        {
            self.transcribing = current;
            change.transcription = Some(current);
        }
        if let Some(required) = find_key(body, CONSENT_DETAILS).and_then(|details| details["consentActivelyRequired"].as_bool())
            && required != self.consent_required
        {
            self.consent_required = required;
            change.consent_required = Some(required);
        }
        change
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingKind {
    #[default]
    Video,
    AudioOnly,
    TranscriptOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopScope {
    #[default]
    Both,
    RecordingOnly,
    TranscriptionOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingCommand {
    Start(RecordingKind),
    Stop(StopScope),
}

#[derive(Debug, Clone, Default)]
pub struct RecordingPlan {
    pub title: String,
    pub kind: RecordingKind,
    pub scope: StopScope,
}

impl RecordingPlan {
    pub fn start(&self) -> RecordingCommand {
        RecordingCommand::Start(self.kind)
    }

    pub fn stop(&self) -> RecordingCommand {
        RecordingCommand::Stop(self.scope)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StartDetails<'a> {
    pub own_mri: &'a str,
    pub participant_leg_id: &'a str,
    pub timestamp: &'a str,
    pub file_name: &'a str,
    pub meeting_title: &'a str,
    pub organizer_name: &'a str,
    pub correlation_id: &'a str,
}

/// The skype token is added inside the page, so the body travels without it.
pub fn start_body(kind: RecordingKind, details: StartDetails<'_>) -> Value {
    let mut parameters = json!({
        "recordingFeatures": recorder_features(),
        "recordingStorageSettings": [{
            "StorageType": STORAGE_TYPE,
            "StorageLocation": STORAGE_LOCATION,
            "FileName": details.file_name,
        }],
        "correlationId": details.correlation_id,
        "meetingTitle": details.meeting_title,
        "exchangeId": null,
        "meetingOrganizer": details.organizer_name,
        "spokenLanguage": "",
        "type": "start",
    });
    let mut body = json!({
        "timestamp": details.timestamp,
        "participantMri": details.own_mri,
        "participantLegId": details.participant_leg_id,
        "action": "start",
    });
    match kind {
        RecordingKind::Video => parameters["recordingMode"] = json!(VIDEO_MODE),
        RecordingKind::AudioOnly => parameters["recordingMode"] = json!(AUDIO_ONLY_MODE),
        RecordingKind::TranscriptOnly => body["mode"] = json!(TRANSCRIPTION_MODE),
    }
    body["processingModes"] = match kind {
        RecordingKind::TranscriptOnly => json!(TRANSCRIPT_ONLY),
        RecordingKind::Video | RecordingKind::AudioOnly => json!(RECORDING_AND_TRANSCRIPT),
    };
    body["actionParameters"] = parameters;
    body
}

pub fn stop_body(scope: StopScope, own_mri: &str, participant_leg_id: &str, timestamp: &str) -> Value {
    let mut body = json!({
        "timestamp": timestamp,
        "participantMri": own_mri,
        "participantLegId": participant_leg_id,
        "action": "stop",
    });
    match scope {
        StopScope::Both => body["processingModes"] = json!(RECORDING_AND_TRANSCRIPT),
        StopScope::RecordingOnly => {
            body["mode"] = json!(RECORDING_MODE_KEY);
            body["processingModes"] = json!(RECORDING_ONLY);
        }
        StopScope::TranscriptionOnly => {
            body["mode"] = json!(TRANSCRIPTION_MODE);
            body["processingModes"] = json!(TRANSCRIPT_ONLY);
        }
    }
    body
}

pub fn command_body(command: RecordingCommand, details: StartDetails<'_>) -> Value {
    match command {
        RecordingCommand::Start(kind) => start_body(kind, details),
        RecordingCommand::Stop(scope) => stop_body(scope, details.own_mri, details.participant_leg_id, details.timestamp),
    }
}

pub fn file_name(meeting_title: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let title = if meeting_title.trim().is_empty() { "Meeting" } else { meeting_title.trim() };
    let safe: String = title.chars().map(|letter| if letter.is_alphanumeric() || letter == ' ' || letter == '-' { letter } else { '_' }).collect();
    format!("{safe}-{}-{FILE_SUFFIX}", now.format("%Y%m%d_%H%M%S"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    Give,
    Deny,
}

pub fn consent_body(from: Value, sequence_number: u32, consent: Consent) -> Value {
    let state_type = match consent {
        Consent::Give => CONSENT_STATE,
        Consent::Deny => DENY_STATE,
    };
    json!({
        "from": from,
        "publishedState": {
            "stateType": state_type,
            "level": "user",
            "content": "{}",
            "sequenceNumber": sequence_number,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(state: &str, sequence: u64) -> Value {
        json!({"meetingStates": {"cloudRecording": {"value": {"State": state}, "sequenceNumber": sequence}, "transcription": {"value": {"State": state}, "sequenceNumber": sequence}}})
    }

    fn consent(required: bool) -> Value {
        json!({"meetingDetails": {"recordingConsentDetails": {"explicitRecordingConsentEnabled": true, "consentActivelyRequired": required}}})
    }

    #[test]
    fn the_recording_state_follows_the_meeting_state_and_ignores_stale_updates() {
        let mut status = RecordingStatus::default();
        assert_eq!(status.apply(&update("Active", 3)), StateChange { recording: Some(true), transcription: Some(true), consent_required: None });
        assert!(status.active());
        assert!(status.apply(&update("Active", 3)).is_empty());
        assert!(status.apply(&update("Inactive", 2)).is_empty());
        assert!(status.active());
        assert_eq!(status.apply(&update("Inactive", 4)).recording, Some(false));
        assert!(!status.active());
    }

    #[test]
    fn transcription_runs_on_its_own_state_and_sequence() {
        let mut status = RecordingStatus::default();
        let transcript_only = json!({"meetingStates": {"transcription": {"value": {"State": "Active"}, "sequenceNumber": 2}}});
        assert_eq!(status.apply(&transcript_only), StateChange { recording: None, transcription: Some(true), consent_required: None });
        assert!(status.transcribing() && !status.active());
        let stale = json!({"meetingStates": {"transcription": {"value": {"State": "Inactive"}, "sequenceNumber": 1}}});
        assert!(status.apply(&stale).is_empty());
        let stopped = json!({"meetingStates": {"transcription": {"value": {"State": "Inactive"}, "sequenceNumber": 3}}});
        assert_eq!(status.apply(&stopped).transcription, Some(false));
    }

    #[test]
    fn unrelated_updates_change_nothing() {
        let mut status = RecordingStatus::default();
        assert!(status.apply(&json!({"activeModalities": {"groupChat": {"threadId": "19:x"}}})).is_empty());
        assert!(status.apply(&Value::Null).is_empty());
    }

    #[test]
    fn consent_is_required_when_the_meeting_says_so() {
        let mut status = RecordingStatus::default();
        assert!(status.apply(&consent(false)).is_empty());
        assert_eq!(status.apply(&consent(true)), StateChange { recording: None, transcription: None, consent_required: Some(true) });
        assert!(status.consent_required());
        assert!(status.apply(&consent(true)).is_empty());
        assert_eq!(status.apply(&consent(false)).consent_required, Some(false));
    }

    #[test]
    fn one_update_can_carry_both_states() {
        let mut both = update("Active", 1);
        both["meetingDetails"] = consent(true)["meetingDetails"].clone();
        let change = RecordingStatus::default().apply(&both);
        assert_eq!(change, StateChange { recording: Some(true), transcription: Some(true), consent_required: Some(true) });
    }

    fn details() -> StartDetails<'static> {
        StartDetails {
            own_mri: "8:orgid:me",
            participant_leg_id: "leg-1",
            timestamp: "2026-10-10T08:44:12.705Z",
            file_name: "Standup-20261010_084412-Meeting Recording",
            meeting_title: "Standup",
            organizer_name: "Me",
            correlation_id: "corr-1",
        }
    }

    #[test]
    fn the_start_command_records_video_to_onedrive_without_a_mode_key() {
        let body = command_body(RecordingCommand::Start(RecordingKind::Video), details());
        assert_eq!(body["action"], "start");
        assert_eq!(body["processingModes"], json!(["recording", "realTimeTranscript"]));
        assert!(body.get("mode").is_none());
        assert!(body.get("participantSkypeToken").is_none());
        let parameters = &body["actionParameters"];
        assert_eq!(parameters["type"], "start");
        assert_eq!(parameters["recordingMode"], "Normal");
        assert_eq!(parameters["meetingTitle"], "Standup");
        assert_eq!(parameters["recordingStorageSettings"][0]["StorageType"], "OnedriveForBusiness");
        assert_eq!(parameters["recordingStorageSettings"][0]["StorageLocation"], "Recordings");
        assert_eq!(parameters["recordingStorageSettings"][0]["FileName"], "Standup-20261010_084412-Meeting Recording");
        assert_eq!(body["participantLegId"], "leg-1");
    }

    #[test]
    fn audio_only_differs_from_video_in_the_recording_mode_alone() {
        let video = start_body(RecordingKind::Video, details());
        let mut audio = start_body(RecordingKind::AudioOnly, details());
        assert_eq!(audio["actionParameters"]["recordingMode"], "AudioOnly");
        audio["actionParameters"]["recordingMode"] = video["actionParameters"]["recordingMode"].clone();
        assert_eq!(audio, video);
    }

    #[test]
    fn transcription_only_has_a_mode_one_processing_mode_and_no_recording_mode() {
        let body = start_body(RecordingKind::TranscriptOnly, details());
        assert_eq!(body["action"], "start");
        assert_eq!(body["mode"], "transcription");
        assert_eq!(body["processingModes"], json!(["realTimeTranscript"]));
        assert!(body["actionParameters"].get("recordingMode").is_none());
        assert_eq!(body["actionParameters"]["type"], "start");
        assert_eq!(body["actionParameters"]["meetingTitle"], "Standup");
    }

    #[test]
    fn the_three_stop_variants_match_the_capture() {
        let stop = |scope| command_body(RecordingCommand::Stop(scope), details());
        let both = stop(StopScope::Both);
        assert_eq!(both["action"], "stop");
        assert_eq!(both["processingModes"], json!(["recording", "realTimeTranscript"]));
        assert!(both.get("mode").is_none());
        assert!(both.get("actionParameters").is_none());
        assert_eq!(both["timestamp"], "2026-10-10T08:44:12.705Z");
        let recording = stop(StopScope::RecordingOnly);
        assert_eq!(recording["mode"], "recording");
        assert_eq!(recording["processingModes"], json!(["recording"]));
        let transcription = stop(StopScope::TranscriptionOnly);
        assert_eq!(transcription["mode"], "transcription");
        assert_eq!(transcription["processingModes"], json!(["realTimeTranscript"]));
    }

    #[test]
    fn a_plan_names_its_start_and_stop_commands() {
        let plan = RecordingPlan { title: "Standup".into(), kind: RecordingKind::AudioOnly, scope: StopScope::RecordingOnly };
        assert_eq!(plan.start(), RecordingCommand::Start(RecordingKind::AudioOnly));
        assert_eq!(plan.stop(), RecordingCommand::Stop(StopScope::RecordingOnly));
    }

    #[test]
    fn file_names_are_safe_and_stamped() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-10T08:44:12Z").unwrap().with_timezone(&chrono::Utc);
        assert_eq!(file_name("Q3 / Plan: v2", at), "Q3 _ Plan_ v2-20261010_084412-Meeting Recording");
        assert_eq!(file_name("  ", at), "Meeting-20261010_084412-Meeting Recording");
    }

    #[test]
    fn consent_and_denial_publish_a_user_state() {
        let given = consent_body(json!({"id": "8:orgid:me"}), 4, Consent::Give);
        assert_eq!(given["publishedState"]["stateType"], "consentToRecording");
        assert_eq!(given["publishedState"]["level"], "user");
        assert_eq!(given["publishedState"]["content"], "{}");
        assert_eq!(given["publishedState"]["sequenceNumber"], 4);
        let denied = consent_body(json!({"id": "8:orgid:me"}), 5, Consent::Deny);
        assert_eq!(denied["publishedState"]["stateType"], "denyConsentToRecording");
    }
}
