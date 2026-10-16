use serde::{Deserialize, Serialize};
use serde_json::Value;
use store::MessageRecord;

const STARTED_TYPE: &str = "#microsoft.graph.callStartedEventMessageDetail";
const ENDED_TYPE: &str = "#microsoft.graph.callEndedEventMessageDetail";
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallPhase {
    Started,
    Ended,
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
}

impl CallEvent {
    pub fn from_graph(detail: &Value) -> Option<CallEvent> {
        let phase = match detail.get("@odata.type")?.as_str()? {
            STARTED_TYPE => CallPhase::Started,
            ENDED_TYPE => CallPhase::Ended,
            _ => return None,
        };
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
                .and_then(Value::as_str)
                .and_then(parse_duration_seconds),
            initiator_id: text_at(detail, "/initiator/user/id"),
            initiator_name: text_at(detail, "/initiator/user/displayName"),
            participant_ids: detail
                .get("callParticipants")
                .and_then(Value::as_array)
                .map(|participants| {
                    participants
                        .iter()
                        .filter_map(|participant| identity_id(participant.get("participant")?))
                        .collect()
                })
                .unwrap_or_default(),
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

    pub fn title(&self, missed: bool) -> &'static str {
        match (self.phase, self.call_type) {
            _ if missed => "Missed call",
            (CallPhase::Started, CallType::Call) => "Call started",
            (CallPhase::Ended, CallType::Call) => "Call ended",
            (CallPhase::Started, CallType::Meeting) => "Meeting started",
            (CallPhase::Ended, CallType::Meeting) => "Meeting ended",
        }
    }

    pub fn plain_text(&self) -> String {
        let title = self.title(false);
        match self.duration_seconds {
            Some(seconds) => format!("{title} {}", format_duration(seconds)),
            None => title.to_owned(),
        }
    }
}

pub fn call_event(record: &MessageRecord) -> Option<CallEvent> {
    CallEvent::from_json(record.call_event_json.as_deref()?)
}

fn identity_id(identity_set: &Value) -> Option<String> {
    identity_set
        .as_object()?
        .values()
        .find_map(|identity| text_at(identity, "/id"))
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
