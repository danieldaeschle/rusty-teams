use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::signaling::{MeetingTarget, Participant, chat_thread_id, endpoint_state, subscribe_request};
use crate::trouter_events::CallbackLinks;

const RESURRECT: &str = "resurrect";

/// Conversation links are required: without them epconv answers 400 with an empty body.
pub fn resolve_body(from: &Participant, callbacks: &CallbackLinks, meeting_data: &Value) -> Value {
    json!({
        "conversationRequest": subscribe_request(callbacks),
        "participants": {"from": from.wire()},
        "groupChat": null,
        "meetingInfo": null,
        "meetingData": meeting_data,
        "meetingPreferences": {"shouldResurrect": RESURRECT},
        "endpointState": endpoint_state(),
    })
}

fn answer_text(body: &Value, pointer: &str) -> Option<String> {
    body.pointer(pointer)?
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

pub fn target_from_answer(body: &Value, meeting_data: &Value) -> Result<MeetingTarget> {
    let not_found = || Error::Signaling("meeting not found".into());
    Ok(MeetingTarget {
        thread_id: chat_thread_id(body).ok_or_else(not_found)?,
        tenant_id: answer_text(body, "/meetingInfo/tenantId")
            .or_else(|| answer_text(body, "/tenantId"))
            .ok_or_else(not_found)?,
        organizer_id: answer_text(body, "/meetingInfo/organizerId").ok_or_else(not_found)?,
        meeting_data: Some(meeting_data.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting_data() -> Value {
        json!({"meetingCode": "234567890123", "passcode": "pw", "meetingUrl": "https://teams.microsoft.com/meet/234567890123?p=pw"})
    }

    fn from() -> Participant {
        Participant {
            mri: "8:orgid:me".into(),
            display_name: "Me".into(),
            endpoint_id: "endpoint".into(),
            participant_id: "participant".into(),
            language_id: "en-gb".into(),
        }
    }

    #[test]
    fn the_resolve_body_is_a_roster_subscription_by_meeting_data() {
        let callbacks = CallbackLinks::new("https://trouter.example/callAgent", "call-1");
        let body = resolve_body(&from(), &callbacks, &meeting_data());
        assert_eq!(body["meetingData"], meeting_data());
        assert_eq!(body["groupChat"], Value::Null);
        assert_eq!(body["meetingInfo"], Value::Null);
        assert_eq!(body["participants"]["from"]["id"], "8:orgid:me");
        assert_eq!(body["meetingPreferences"]["shouldResurrect"], "resurrect");
        let links = body["conversationRequest"]["links"].as_object().unwrap();
        assert!(links.contains_key("conversationUpdate") && links.contains_key("conversationEnd"));
        assert!(body["conversationRequest"]["roster"]["rosterUpdate"].as_str().unwrap().contains("call-1"));
        assert!(body.get("callInvitation").is_none());
    }

    #[test]
    fn an_answer_with_thread_tenant_and_organizer_gives_a_target() {
        let body = json!({
            "activeModalities": {"groupChat": {"threadId": "19:meeting_x@thread.v2", "messageId": "0"}},
            "meetingInfo": {"tenantId": "tenant-1", "organizerId": "organizer-1"},
        });
        let target = target_from_answer(&body, &meeting_data()).unwrap();
        assert_eq!(target.thread_id, "19:meeting_x@thread.v2");
        assert_eq!(target.tenant_id, "tenant-1");
        assert_eq!(target.organizer_id, "organizer-1");
        assert_eq!(target.meeting_data, Some(meeting_data()));
    }

    #[test]
    fn the_top_level_tenant_fills_a_missing_meeting_info_tenant() {
        let body = json!({
            "activeModalities": {"groupChat": {"threadId": "19:x"}},
            "tenantId": "tenant-2",
            "meetingInfo": {"organizerId": "organizer-1"},
        });
        assert_eq!(target_from_answer(&body, &meeting_data()).unwrap().tenant_id, "tenant-2");
    }

    #[test]
    fn an_answer_without_a_thread_or_organizer_is_not_found() {
        let no_thread = json!({"meetingInfo": {"tenantId": "t", "organizerId": "o"}});
        assert!(target_from_answer(&no_thread, &meeting_data()).is_err());
        let no_organizer = json!({"activeModalities": {"groupChat": {"threadId": "19:x"}}, "meetingInfo": {"tenantId": "t"}});
        assert!(target_from_answer(&no_organizer, &meeting_data()).is_err());
    }
}
