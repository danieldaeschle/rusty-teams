use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingPolicy {
    pub meeting_recording: bool,
    pub meeting_transcription: bool,
    pub call_recording: bool,
    pub call_transcription: bool,
}

impl Default for RecordingPolicy {
    fn default() -> Self {
        RecordingPolicy {
            meeting_recording: true,
            meeting_transcription: false,
            call_recording: true,
            call_transcription: false,
        }
    }
}

pub fn recording_policy_body() -> Value {
    json!({"meetingPolicy": true, "callingPolicy": true})
}

pub fn parse_recording_policy(body: &Value) -> RecordingPolicy {
    let recording = |pointer: &str| {
        body.pointer(pointer)
            .and_then(Value::as_bool)
            .unwrap_or(true)
    };
    let transcription = |pointer: &str| {
        body.pointer(pointer)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    RecordingPolicy {
        meeting_recording: recording("/meetingPolicy/value/allowCloudRecording"),
        meeting_transcription: transcription("/meetingPolicy/value/allowTranscription"),
        call_recording: recording("/callingPolicy/value/allowCloudRecordingForCalls"),
        call_transcription: transcription("/callingPolicy/value/allowTranscriptionForCalling"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_asks_for_both_policies() {
        assert_eq!(
            recording_policy_body(),
            json!({"meetingPolicy": true, "callingPolicy": true})
        );
    }

    #[test]
    fn parses_the_meeting_and_calling_policy() {
        let body = json!({
            "meetingPolicy": {"value": {"allowCloudRecording": false, "allowTranscription": true}},
            "callingPolicy": {"value": {"allowCloudRecordingForCalls": true, "allowTranscriptionForCalling": false}}
        });
        assert_eq!(
            parse_recording_policy(&body),
            RecordingPolicy {
                meeting_recording: false,
                meeting_transcription: true,
                call_recording: true,
                call_transcription: false,
            }
        );
    }

    #[test]
    fn missing_fields_close_transcription_but_keep_recording() {
        assert_eq!(
            parse_recording_policy(&json!({})),
            RecordingPolicy::default()
        );
        let partial = json!({"meetingPolicy": {"value": {"allowTranscription": true}}});
        let policy = parse_recording_policy(&partial);
        assert!(policy.meeting_recording && policy.meeting_transcription);
        assert!(policy.call_recording && !policy.call_transcription);
    }
}
