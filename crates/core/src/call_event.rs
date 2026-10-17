use serde::{Deserialize, Serialize};
use serde_json::Value;
use store::MessageRecord;

const STARTED_TYPE: &str = "#microsoft.graph.callStartedEventMessageDetail";
const ENDED_TYPE: &str = "#microsoft.graph.callEndedEventMessageDetail";
const RECORDING_TYPE: &str = "#microsoft.graph.callRecordingEventMessageDetail";
const TRANSCRIPT_TYPE: &str = "#microsoft.graph.callTranscriptEventMessageDetail";
const UNRESOLVED_NAME_PREFIX: &str = "orgid:";
const RECORDING_SUFFIX: &str = "-Meeting Recording";
const GRAPH_SHARES: &str = "https://graph.microsoft.com/v1.0/shares/";
const THUMBNAIL_PATH: &str = "/driveItem/thumbnails/0/large/content";
const SHAREPOINT_HOST: &str = ".sharepoint.com/";
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallPhase {
    Started,
    Ended,
    RecordingStarted,
    RecordingStopped,
    RecordingSaved,
    TranscriptStarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallType {
    Call,
    Meeting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallEvent {
    pub phase: CallPhase,
    pub call_type: CallType,
    pub call_id: String,
    pub duration_seconds: Option<u64>,
    pub initiator_id: Option<String>,
    pub initiator_name: Option<String>,
    pub participant_ids: Vec<String>,
    #[serde(default)]
    pub participant_names: Vec<String>,
    #[serde(default)]
    pub recording: Option<Recording>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recording {
    pub name: Option<String>,
    pub url: Option<String>,
    pub organizer_id: Option<String>,
}

impl Recording {
    pub fn title(&self) -> Option<String> {
        let name = self.name.as_deref()?;
        let stem = name.strip_suffix(".mp4").unwrap_or(name);
        let stem = stem.strip_suffix(RECORDING_SUFFIX).unwrap_or(stem);
        let title = match stem.rsplit_once('-') {
            Some((title, stamp)) if is_recording_stamp(stamp) => title,
            _ => stem,
        };
        Some(title.trim().to_owned()).filter(|title| !title.is_empty())
    }

    pub fn thumbnail_url(&self) -> Option<String> {
        let url = self
            .url
            .as_deref()
            .filter(|url| url.contains(SHAREPOINT_HOST))?;
        Some(format!(
            "{GRAPH_SHARES}{}{THUMBNAIL_PATH}",
            chatsvc::transcripts::share_id(url)
        ))
    }
}

fn is_recording_stamp(stamp: &str) -> bool {
    let digits = stamp.strip_suffix("UTC").unwrap_or(stamp);
    digits.split_once('_').is_some_and(|(date, time)| {
        date.len() == 8
            && time.len() == 6
            && date
                .bytes()
                .chain(time.bytes())
                .all(|byte| byte.is_ascii_digit())
    })
}

fn recording_phase(detail: &Value) -> Option<CallPhase> {
    match detail.get("callRecordingStatus").and_then(Value::as_str)? {
        "initial" => Some(CallPhase::RecordingStarted),
        "chunkFinished" => Some(CallPhase::RecordingStopped),
        "success" => Some(CallPhase::RecordingSaved),
        _ => None,
    }
}

impl CallEvent {
    pub fn from_graph(detail: &Value) -> Option<CallEvent> {
        let phase = match detail.get("@odata.type")?.as_str()? {
            STARTED_TYPE => CallPhase::Started,
            ENDED_TYPE => CallPhase::Ended,
            RECORDING_TYPE => recording_phase(detail)?,
            TRANSCRIPT_TYPE => CallPhase::TranscriptStarted,
            _ => return None,
        };
        let participants: Vec<(String, Option<String>)> = detail
            .get("callParticipants")
            .and_then(Value::as_array)
            .map(|participants| {
                participants
                    .iter()
                    .filter_map(|participant| identity(participant.get("participant")?))
                    .collect()
            })
            .unwrap_or_default();
        let recording =
            (detail.get("@odata.type")?.as_str()? == RECORDING_TYPE).then(|| Recording {
                name: text_at(detail, "/callRecordingDisplayName"),
                url: text_at(detail, "/callRecordingUrl"),
                organizer_id: text_at(detail, "/meetingOrganizer/user/id"),
            });
        let call_type = match detail.get("callEventType").and_then(Value::as_str) {
            Some("meeting") => CallType::Meeting,
            _ => CallType::Call,
        };
        Some(CallEvent {
            phase,
            call_type,
            call_id: detail.get("callId")?.as_str()?.to_owned(),
            duration_seconds: detail
                .get("callDuration")
                .or_else(|| detail.get("callRecordingDuration"))
                .and_then(Value::as_str)
                .and_then(parse_duration_seconds),
            initiator_id: text_at(detail, "/initiator/user/id"),
            initiator_name: person_name(detail.pointer("/initiator/user")),
            participant_names: participants
                .iter()
                .map(|(_, name)| name.clone().unwrap_or_default())
                .collect(),
            participant_ids: participants.into_iter().map(|(id, _)| id).collect(),
            recording,
        })
    }

    pub fn from_json(json: &str) -> Option<CallEvent> {
        serde_json::from_str(json).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn initiated_by(&self, user_id: Option<&str>) -> bool {
        user_id.is_some() && self.initiator_id.as_deref() == user_id
    }

    pub fn missed(&self, my_user_id: Option<&str>) -> bool {
        my_user_id.is_some()
            && self.phase == CallPhase::Ended
            && self.call_type == CallType::Call
            && self.participant_ids.len() < 2
            && !self.initiated_by(my_user_id)
    }

    pub fn is_recording(&self) -> bool {
        matches!(
            self.phase,
            CallPhase::RecordingStarted
                | CallPhase::RecordingStopped
                | CallPhase::RecordingSaved
                | CallPhase::TranscriptStarted
        )
    }

    pub fn title(&self, missed: bool) -> &'static str {
        match (self.phase, self.call_type) {
            _ if missed => "Missed call",
            (CallPhase::Started, CallType::Call) => "Call started",
            (CallPhase::Ended, CallType::Call) => "Call ended",
            (CallPhase::Started, CallType::Meeting) => "Meeting started",
            (CallPhase::Ended, CallType::Meeting) => "Meeting ended",
            (CallPhase::RecordingStarted, _) => "Recording has started",
            (CallPhase::RecordingStopped, _) => "Recording stopped",
            (CallPhase::RecordingSaved, _) => "Recording is ready",
            (CallPhase::TranscriptStarted, _) => "Transcription has started",
        }
    }

    pub fn plain_text(&self) -> String {
        let title = self.title(false);
        match self.duration_seconds {
            Some(seconds) if !self.is_recording() => {
                format!("{title} {}", format_duration(seconds))
            }
            _ => title.to_owned(),
        }
    }
}

pub fn call_event(record: &MessageRecord) -> Option<CallEvent> {
    CallEvent::from_json(record.call_event_json.as_deref()?)
}

fn identity(identity_set: &Value) -> Option<(String, Option<String>)> {
    identity_set
        .as_object()?
        .values()
        .find_map(|identity| Some((text_at(identity, "/id")?, person_name(Some(identity)))))
}

fn person_name(identity: Option<&Value>) -> Option<String> {
    text_at(identity?, "/displayName").filter(|name| !name.starts_with(UNRESOLVED_NAME_PREFIX))
}

fn text_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)?
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

pub fn parse_duration_seconds(text: &str) -> Option<u64> {
    let rest = text.strip_prefix('P')?;
    let (date_part, time_part) = match rest.split_once('T') {
        Some((date_part, time_part)) => (date_part, time_part),
        None => (rest, ""),
    };
    if date_part.is_empty() && time_part.is_empty() {
        return None;
    }
    let days = sum_components(date_part, &[('D', SECONDS_PER_DAY)])?;
    let time = sum_components(
        time_part,
        &[('H', SECONDS_PER_HOUR), ('M', SECONDS_PER_MINUTE), ('S', 1)],
    )?;
    Some(days + time)
}

fn sum_components(part: &str, units: &[(char, u64)]) -> Option<u64> {
    let mut total = 0;
    let mut number = String::new();
    for character in part.chars() {
        if character.is_ascii_digit() || character == '.' {
            number.push(character);
            continue;
        }
        let (_, factor) = units.iter().find(|(unit, _)| *unit == character)?;
        let value: f64 = number.parse().ok()?;
        total += (value * *factor as f64) as u64;
        number.clear();
    }
    number.is_empty().then_some(total)
}

pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    let remainder = seconds % SECONDS_PER_MINUTE;
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 || (hours > 0 && remainder > 0) {
        parts.push(format!("{minutes}m"));
    }
    if remainder > 0 || (hours == 0 && minutes == 0) {
        parts.push(format!("{remainder}s"));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/tests/fixtures/call_events/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn meeting_chat_events() -> Vec<CallEvent> {
        fixture("meeting_chat_recording.json")["value"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .map(|message| CallEvent::from_graph(&message["eventDetail"]).unwrap())
            .collect()
    }

    #[test]
    fn recorded_meeting_yields_every_phase_in_order() {
        let phases: Vec<CallPhase> = meeting_chat_events()
            .iter()
            .map(|event| event.phase)
            .collect();
        assert_eq!(
            phases,
            [
                CallPhase::Started,
                CallPhase::RecordingStarted,
                CallPhase::RecordingStarted,
                CallPhase::TranscriptStarted,
                CallPhase::RecordingStopped,
                CallPhase::Ended,
                CallPhase::RecordingStopped,
                CallPhase::RecordingSaved,
            ]
        );
        let call_ids: std::collections::HashSet<String> = meeting_chat_events()
            .into_iter()
            .map(|event| event.call_id)
            .collect();
        assert_eq!(call_ids.len(), 1);
    }

    #[test]
    fn saved_recording_keeps_link_title_duration_and_owner() {
        let saved = meeting_chat_events().pop().unwrap();
        let recording = saved.recording.as_ref().unwrap();
        assert_eq!(saved.duration_seconds, Some(814));
        assert_eq!(recording.title().as_deref(), Some("Weekly team meeting"));
        assert!(recording.url.as_deref().unwrap().contains("/:v:/"));
        assert_eq!(
            recording.organizer_id.as_deref(),
            Some("00000000-0000-4000-8000-000000000002")
        );
        assert_eq!(saved.plain_text(), "Recording is ready");
        let thumbnail = recording.thumbnail_url().unwrap();
        assert!(thumbnail.starts_with("https://graph.microsoft.com/v1.0/shares/u!"));
        assert!(thumbnail.ends_with("/driveItem/thumbnails/0/large/content"));
    }

    #[test]
    fn started_recording_has_no_link_and_names_its_initiator_by_id() {
        let started = &meeting_chat_events()[1];
        assert_eq!(started.recording.as_ref().unwrap().url, None);
        assert_eq!(started.recording.as_ref().unwrap().thumbnail_url(), None);
        assert_eq!(started.duration_seconds, Some(0));
        assert!(started.initiator_id.is_some());
        assert_eq!(started.initiator_name, None);
    }

    #[test]
    fn ended_meeting_keeps_participant_names_beside_their_ids() {
        let ended = meeting_chat_events()
            .into_iter()
            .find(|event| event.phase == CallPhase::Ended)
            .unwrap();
        assert_eq!(ended.call_type, CallType::Meeting);
        assert_eq!(ended.duration_seconds, Some(988));
        assert_eq!(ended.participant_ids.len(), ended.participant_names.len());
        assert_eq!(ended.participant_names[0], "");
        assert_eq!(ended.participant_names[1], "Person 1");
        assert_eq!(ended.recording, None);
    }

    #[test]
    fn channel_meet_now_root_is_a_started_meeting() {
        let thread = fixture("channel_adhoc_meeting.json");
        let root = CallEvent::from_graph(&thread["eventDetail"]).unwrap();
        let ended = CallEvent::from_graph(&thread["replies"][0]["eventDetail"]).unwrap();
        assert_eq!(root.phase, CallPhase::Started);
        assert_eq!(root.call_type, CallType::Meeting);
        assert_eq!(ended.call_id, root.call_id);
        assert_eq!(ended.duration_seconds, Some(11_975));
        assert_eq!(ended.initiator_name, None);
    }

    #[test]
    fn unknown_recording_status_is_not_an_event() {
        let detail = serde_json::json!({
            "@odata.type": RECORDING_TYPE,
            "callId": "c",
            "callRecordingStatus": "failure",
        });
        assert_eq!(CallEvent::from_graph(&detail), None);
    }

    #[test]
    fn events_stored_before_recordings_still_load() {
        let stored = r#"{"phase":"Ended","call_type":"Call","call_id":"c","duration_seconds":5,"initiator_id":"u","initiator_name":null,"participant_ids":["u"]}"#;
        let event = CallEvent::from_json(stored).unwrap();
        assert!(event.participant_names.is_empty());
        assert_eq!(event.recording, None);
    }

    #[test]
    fn recording_title_drops_the_stamp_only_when_it_is_one() {
        let title = |name: &str| {
            Recording {
                name: Some(name.to_owned()),
                url: None,
                organizer_id: None,
            }
            .title()
        };
        assert_eq!(
            title("Sync-20261012_140227UTC-Meeting Recording.mp4").as_deref(),
            Some("Sync")
        );
        assert_eq!(
            title("Q3-plan-Meeting Recording.mp4").as_deref(),
            Some("Q3-plan")
        );
        assert_eq!(title("demo.mp4").as_deref(), Some("demo"));
    }
}
