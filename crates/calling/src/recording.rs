use serde_json::{Value, json};

use crate::captions::{BotAction, recorder_features};
use crate::trouter_events::find_key;

const CLOUD_RECORDING: &str = "cloudRecording";
const CONSENT_DETAILS: &str = "recordingConsentDetails";
const ACTIVE: &str = "Active";
const PROCESSING_MODES: [&str; 2] = ["recording", "realTimeTranscript"];
const STORAGE_TYPE: &str = "OnedriveForBusiness";
const STORAGE_LOCATION: &str = "Recordings";
const RECORDING_MODE: &str = "Normal";
const FILE_SUFFIX: &str = "Meeting Recording";
const CONSENT_STATE: &str = "consentToRecording";
const DENY_STATE: &str = "denyConsentToRecording";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateChange {
    pub recording: Option<bool>,
    pub consent_required: Option<bool>,
}

impl StateChange {
    pub fn is_empty(&self) -> bool {
        self.recording.is_none() && self.consent_required.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecordingStatus {
    active: bool,
    consent_required: bool,
    sequence: u64,
}

impl RecordingStatus {
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn consent_required(&self) -> bool {
        self.consent_required
    }

    pub fn apply(&mut self, body: &Value) -> StateChange {
        let mut change = StateChange::default();
        if let Some(state) = find_key(body, CLOUD_RECORDING) {
            let sequence = state["sequenceNumber"].as_u64().unwrap_or_default();
            let current = state["value"]["State"].as_str().map(|name| name == ACTIVE);
            if let Some(current) = current
                && sequence >= self.sequence
            {
                self.sequence = sequence;
                if current != self.active {
                    self.active = current;
                    change.recording = Some(current);
                }
            }
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
pub fn start_body(details: StartDetails<'_>) -> Value {
    json!({
        "timestamp": details.timestamp,
        "participantMri": details.own_mri,
        "participantLegId": details.participant_leg_id,
        "action": "start",
        "processingModes": PROCESSING_MODES,
        "actionParameters": {
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
            "recordingMode": RECORDING_MODE,
            "spokenLanguage": "",
            "type": "start",
        },
    })
}

pub fn stop_body(own_mri: &str, participant_leg_id: &str, timestamp: &str) -> Value {
    json!({
        "timestamp": timestamp,
        "participantMri": own_mri,
        "participantLegId": participant_leg_id,
        "action": "stop",
        "processingModes": PROCESSING_MODES,
    })
}

pub fn command_body(action: BotAction, details: StartDetails<'_>) -> Value {
    match action {
        BotAction::Start => start_body(details),
        BotAction::Stop => stop_body(details.own_mri, details.participant_leg_id, details.timestamp),
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
        assert_eq!(status.apply(&update("Active", 3)), StateChange { recording: Some(true), consent_required: None });
        assert!(status.active());
        assert!(status.apply(&update("Active", 3)).is_empty());
        assert!(status.apply(&update("Inactive", 2)).is_empty());
        assert!(status.active());
        assert_eq!(status.apply(&update("Inactive", 4)).recording, Some(false));
        assert!(!status.active());
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
        assert_eq!(status.apply(&consent(true)), StateChange { recording: None, consent_required: Some(true) });
        assert!(status.consent_required());
        assert!(status.apply(&consent(true)).is_empty());
        assert_eq!(status.apply(&consent(false)).consent_required, Some(false));
    }

    #[test]
    fn one_update_can_carry_both_states() {
        let mut both = update("Active", 1);
        both["meetingDetails"] = consent(true)["meetingDetails"].clone();
        let change = RecordingStatus::default().apply(&both);
        assert_eq!(change, StateChange { recording: Some(true), consent_required: Some(true) });
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
    fn the_start_command_records_to_onedrive_without_a_mode_key() {
        let body = command_body(BotAction::Start, details());
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
    fn the_stop_command_is_short() {
        let body = command_body(BotAction::Stop, details());
        assert_eq!(body["action"], "stop");
        assert_eq!(body["processingModes"], json!(["recording", "realTimeTranscript"]));
        assert!(body.get("actionParameters").is_none());
        assert_eq!(body["timestamp"], "2026-10-10T08:44:12.705Z");
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
