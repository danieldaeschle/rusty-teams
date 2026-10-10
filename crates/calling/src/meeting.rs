use serde_json::Value;
use session::{Request, Session};

use crate::error::Result;
use crate::relay::ic3_scope;
use crate::signaling::{CHATSVC_REGION, MeetingTarget};

const LIVE_STATE_PREFIX: &str = "awareness_conversationLiveState";
const MEETING_PROPERTY: &str = "meeting";
const ACTIVE_STATUS: &str = "Active";
const MILLISECOND_EPOCH_FLOOR: i64 = 1_000_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveMeeting {
    pub thread_id: String,
    pub conversation_url: Option<String>,
    pub expiration: Option<i64>,
    pub organizer_id: Option<String>,
    pub tenant_id: Option<String>,
    pub meeting_code: Option<String>,
    pub passcode: Option<String>,
}

impl LiveMeeting {
    pub fn is_running(&self, now_unix: i64) -> bool {
        self.expiration.is_none_or(|expires| expires > now_unix)
    }

    pub fn target(&self) -> Option<MeetingTarget> {
        let meeting_data = self.meeting_code.as_ref().map(|code| {
            serde_json::json!({"meetingCode": code, "passcode": self.passcode})
        });
        Some(MeetingTarget {
            thread_id: self.thread_id.clone(),
            tenant_id: self.tenant_id.clone()?,
            organizer_id: self.organizer_id.clone()?,
            meeting_data,
        })
    }
}

fn as_object(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => serde_json::from_str(text).ok(),
        Value::Object(_) => Some(value.clone()),
        _ => None,
    }
}

fn expiration_seconds(value: &Value) -> Option<i64> {
    let raw = value.as_i64().or_else(|| value.as_str()?.parse().ok())?;
    Some(if raw >= MILLISECOND_EPOCH_FLOOR { raw / 1000 } else { raw })
}

fn property_text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().filter(|text| !text.is_empty()).map(str::to_owned)
}

fn thread_properties(body: &Value) -> Value {
    let mut merged = serde_json::Map::new();
    for key in ["properties", "threadProperties"] {
        if let Some(properties) = body.get(key).and_then(Value::as_object) {
            merged.extend(properties.clone());
        }
    }
    Value::Object(merged)
}

pub fn live_meeting(thread_id: &str, properties: &Value, now_unix: i64) -> Option<LiveMeeting> {
    let meeting = properties.get(MEETING_PROPERTY).and_then(as_object);
    let entries = properties.as_object()?;
    entries
        .iter()
        .filter(|(key, _)| key.starts_with(LIVE_STATE_PREFIX))
        .filter_map(|(_, value)| as_object(value))
        .filter(|state| state["status"].as_str() == Some(ACTIVE_STATUS))
        .filter_map(|state| {
            let expiration = expiration_seconds(&state["expiration"]);
            if expiration.is_some_and(|expires| expires <= now_unix) {
                return None;
            }
            let info = &state["meetingInfo"];
            let fallback = meeting.as_ref();
            Some(LiveMeeting {
                thread_id: thread_id.to_owned(),
                conversation_url: property_text(&state, "conversationUrl"),
                expiration,
                organizer_id: property_text(info, "organizerId")
                    .or_else(|| fallback.and_then(|meeting| property_text(meeting, "organizerId"))),
                tenant_id: property_text(info, "tenantId")
                    .or_else(|| fallback.and_then(|meeting| property_text(meeting, "tenantId"))),
                meeting_code: property_text(&state["meetingData"], "meetingCode"),
                passcode: property_text(&state["meetingData"], "passcode"),
            })
        })
        .next()
}

pub async fn fetch_live_meeting(session: &Session, thread_id: &str) -> Result<Option<LiveMeeting>> {
    let url = format!(
        "https://teams.cloud.microsoft/api/chatsvc/{CHATSVC_REGION}/v1/users/ME/conversations/{}?view=msnp24Equivalent",
        thread_id.replace(':', "%3A").replace('@', "%40")
    );
    let response = session.send(Request::get(url), &ic3_scope()).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    Ok(live_meeting(thread_id, &thread_properties(&response.body), now))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOW: i64 = 1_791_600_000;

    fn live_state(status: &str, expiration: i64) -> String {
        json!({
            "conversationUrl": "https://conv/1",
            "conversationId": "1",
            "status": status,
            "expiration": expiration,
            "conversationType": "scheduledMeeting",
            "meetingInfo": {"organizerId": "org-1", "tenantId": "tenant-1"},
            "meetingData": {"meetingCode": "123", "passcode": "pw"},
        })
        .to_string()
    }

    #[test]
    fn an_active_unexpired_state_is_a_running_meeting() {
        let properties = json!({"awareness_conversationLiveState:0": live_state("Active", NOW + 600)});
        let meeting = live_meeting("19:m@thread.v2", &properties, NOW).unwrap();
        assert_eq!(meeting.organizer_id.as_deref(), Some("org-1"));
        assert_eq!(meeting.conversation_url.as_deref(), Some("https://conv/1"));
        let target = meeting.target().unwrap();
        assert_eq!(target.thread_id, "19:m@thread.v2");
        assert_eq!(target.tenant_id, "tenant-1");
        assert_eq!(target.meeting_data.unwrap()["meetingCode"], "123");
    }

    #[test]
    fn no_state_expired_or_ended_means_no_join_button() {
        assert!(live_meeting("t", &json!({}), NOW).is_none());
        assert!(live_meeting("t", &json!({"topic": "x"}), NOW).is_none());
        let expired = json!({"awareness_conversationLiveState:0": live_state("Active", NOW - 1)});
        assert!(live_meeting("t", &expired, NOW).is_none());
        let ended = json!({"awareness_conversationLiveState:0": live_state("Ended", NOW + 600)});
        assert!(live_meeting("t", &ended, NOW).is_none());
    }

    #[test]
    fn a_known_meeting_stops_running_when_it_expires() {
        let properties = json!({"awareness_conversationLiveState:0": live_state("Active", NOW + 600)});
        let meeting = live_meeting("t", &properties, NOW).unwrap();
        assert!(meeting.is_running(NOW + 599));
        assert!(!meeting.is_running(NOW + 600));
    }

    #[test]
    fn a_second_parallel_state_still_counts() {
        let properties = json!({
            "awareness_conversationLiveState:0": live_state("Ended", NOW + 600),
            "awareness_conversationLiveState:1": live_state("Active", NOW + 600),
        });
        assert!(live_meeting("t", &properties, NOW).is_some());
    }

    #[test]
    fn expiration_in_milliseconds_is_understood() {
        let properties = json!({"awareness_conversationLiveState": live_state("Active", (NOW + 600) * 1000)});
        assert!(live_meeting("t", &properties, NOW).is_some());
        let past = json!({"awareness_conversationLiveState": live_state("Active", (NOW - 600) * 1000)});
        assert!(live_meeting("t", &past, NOW).is_none());
    }

    #[test]
    fn meeting_info_falls_back_to_the_meeting_property() {
        let mut state: Value = serde_json::from_str(&live_state("Active", NOW + 600)).unwrap();
        state.as_object_mut().unwrap().remove("meetingInfo");
        let properties = json!({
            "awareness_conversationLiveState:0": state.to_string(),
            "meeting": json!({"organizerId": "org-2", "tenantId": "tenant-2"}).to_string(),
        });
        let meeting = live_meeting("t", &properties, NOW).unwrap();
        assert_eq!(meeting.organizer_id.as_deref(), Some("org-2"));
        assert_eq!(meeting.tenant_id.as_deref(), Some("tenant-2"));
    }

    #[test]
    fn without_organizer_or_tenant_there_is_no_join_target() {
        let state = json!({"status": "Active", "expiration": NOW + 600});
        let properties = json!({"awareness_conversationLiveState:0": state.to_string()});
        let meeting = live_meeting("t", &properties, NOW).unwrap();
        assert!(meeting.target().is_none());
    }

    #[test]
    fn thread_and_conversation_properties_merge() {
        let body = json!({
            "properties": {"a": 1},
            "threadProperties": {"awareness_conversationLiveState:0": live_state("Active", NOW + 600)},
        });
        let merged = thread_properties(&body);
        assert!(merged.get("a").is_some());
        assert!(live_meeting("t", &merged, NOW).is_some());
    }
}
