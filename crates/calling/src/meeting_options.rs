use serde_json::{Value, json};
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::{Error, Result};
use crate::organizer::restrictions_value;
use crate::signaling::{CHATSVC_REGION, MeetingTarget};
use crate::whiteboard::encoded;

const OPTIONS_SCOPE: &str = "user_impersonation";
const READ_HOST: &str = "https://teams.microsoft.com";
const LIVE_HOST: &str = "https://teams.cloud.microsoft";
const AUTO_ADMITTED: &str = "AutoAdmittedUsers";
const PRESENTER: &str = "PresenterOption";
const RESTRICTIONS: &str = "AttendeeRestrictions";
const MEETING_CHAT: &str = "AllowMeetingChat";
const REACTIONS: &str = "AllowTeamsMeetingReactions";
const AUTO_RECORD: &str = "AutoRecordAndTranscribeMode";
const RECORD_ON: &str = "AutoRecordingAndTranscription";
const RECORD_OFF: &str = "Off";
const LIST: &str = "List";
const BOOLEAN: &str = "Boolean";
const BIT_MASK: &str = "BitMask";

pub const BYPASS_CHOICES: [(&str, &str); 6] = [
    ("Everyone", "Everyone"),
    ("EveryoneInSameAndFederatedCompany", "People in my org and trusted orgs"),
    ("EveryoneInCompany", "People in my org"),
    ("EveryoneInCompanyExcludingGuests", "People in my org, no guests"),
    ("InvitedUsers", "People I invite"),
    ("OrganizerOnly", "Only me"),
];
pub const PRESENTER_CHOICES: [(&str, &str); 3] = [
    ("Everyone", "Everyone"),
    ("EveryoneInCompany", "People in my org"),
    ("OrganizerOnly", "Only me"),
];
pub const CHAT_CHOICES: [(&str, &str); 3] = [
    ("Enabled", "On"),
    ("InMeeting", "In-meeting only"),
    ("Disabled", "Off"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingOptions {
    pub bypass_lobby: String,
    pub presenters: String,
    pub attendee_mic: bool,
    pub attendee_camera: bool,
    pub meeting_chat: String,
    pub reactions: bool,
    pub record_automatically: bool,
    record_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionWrite {
    pub name: &'static str,
    pub kind: &'static str,
    pub value: Value,
}

impl OptionWrite {
    pub fn restrictions(mic_off: bool, camera_off: bool) -> OptionWrite {
        OptionWrite { name: RESTRICTIONS, kind: BIT_MASK, value: json!(restrictions_value(mic_off, camera_off)) }
    }

    fn body(&self) -> Value {
        json!({"options": [{"name": self.name, "type": self.kind, "currentValue": self.value, "isRecommended": null}]})
    }
}

impl MeetingOptions {
    pub fn from_response(body: &Value) -> Option<MeetingOptions> {
        let options = body["options"].as_array()?;
        let current = |name: &str| options.iter().find(|option| option["name"].as_str() == Some(name)).map(|option| &option["currentValue"]);
        let text = |name: &str| current(name).and_then(Value::as_str).map(str::to_owned);
        let flag = |name: &str| current(name).and_then(|value| value.as_bool().or_else(|| value.as_str().map(|text| text.eq_ignore_ascii_case("true"))));
        let restrictions = text(RESTRICTIONS).unwrap_or_default();
        let record_mode = text(AUTO_RECORD).unwrap_or_else(|| RECORD_OFF.to_owned());
        Some(MeetingOptions {
            bypass_lobby: text(AUTO_ADMITTED)?,
            presenters: text(PRESENTER)?,
            attendee_mic: !restrictions.contains("DisableAudio"),
            attendee_camera: !restrictions.contains("DisableVideo"),
            meeting_chat: text(MEETING_CHAT)?,
            reactions: flag(REACTIONS).unwrap_or(true),
            record_automatically: record_mode == RECORD_ON,
            record_mode,
        })
    }

    pub fn changes(&self, edited: &MeetingOptions) -> Vec<OptionWrite> {
        let mut writes = Vec::new();
        let mut list = |name: &'static str, before: &str, after: &str| {
            if before != after {
                writes.push(OptionWrite { name, kind: LIST, value: json!(after) });
            }
        };
        list(AUTO_ADMITTED, &self.bypass_lobby, &edited.bypass_lobby);
        list(PRESENTER, &self.presenters, &edited.presenters);
        list(MEETING_CHAT, &self.meeting_chat, &edited.meeting_chat);
        if self.record_automatically != edited.record_automatically {
            let mode = if edited.record_automatically { RECORD_ON } else { RECORD_OFF };
            writes.push(OptionWrite { name: AUTO_RECORD, kind: LIST, value: json!(mode) });
        }
        if self.reactions != edited.reactions {
            writes.push(OptionWrite { name: REACTIONS, kind: BOOLEAN, value: json!(edited.reactions) });
        }
        if (self.attendee_mic, self.attendee_camera) != (edited.attendee_mic, edited.attendee_camera) {
            writes.push(OptionWrite::restrictions(!edited.attendee_mic, !edited.attendee_camera));
        }
        writes
    }

    pub fn attendees_off(&self) -> (bool, bool) {
        (!self.attendee_mic, !self.attendee_camera)
    }

    pub fn record_mode(&self) -> &str {
        &self.record_mode
    }
}

fn coordinates(target: &MeetingTarget) -> String {
    format!("{}/{}/{}", encoded(&target.tenant_id), encoded(&target.organizer_id), encoded(&target.thread_id))
}

pub fn read_url(target: &MeetingTarget) -> String {
    format!("{READ_HOST}/api/mt/{CHATSVC_REGION}//beta/meetings/v1/options/{}/0?monarch=true", coordinates(target))
}

pub fn write_url(target: &MeetingTarget) -> String {
    format!("{LIVE_HOST}/api/mt/{CHATSVC_REGION}/beta/meetings/v1/options/{}/0/0?monarch=true", coordinates(target))
}

fn with_headers(mut request: Request, caller: &str) -> Request {
    request.headers.extend([
        ("X-Ms-Migration".to_owned(), "True".to_owned()),
        ("x-ms-client-caller".to_owned(), caller.to_owned()),
        ("X-MS-Request-Id".to_owned(), uuid::Uuid::new_v4().to_string()),
    ]);
    request
}

fn scope() -> Scope {
    Scope::new(SPACES, OPTIONS_SCOPE)
}

pub async fn fetch(session: &Session, target: &MeetingTarget) -> Result<MeetingOptions> {
    let request = with_headers(Request::get(read_url(target)), "meeting-options-dialog");
    let response = session.send(request, &scope()).await?;
    MeetingOptions::from_response(&response.body).ok_or_else(|| Error::Signaling("meeting options answer without the expected options".into()))
}

pub fn write_failed(body: &Value) -> bool {
    body["success"].as_bool() == Some(false)
        || body["options"].as_array().is_some_and(|options| options.iter().any(|option| option["success"].as_bool() == Some(false)))
}

pub async fn write(session: &Session, target: &MeetingTarget, change: &OptionWrite) -> Result<()> {
    let mut request = with_headers(Request::with_body(Method::Post, write_url(target), change.body()), "meeting-options-dialog");
    request.headers.push(("x-ms-test-user".to_owned(), "False".to_owned()));
    let response = session.send(request, &scope()).await?;
    if write_failed(&response.body) {
        return Err(Error::Signaling(format!("the service refused {}", change.name)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> MeetingTarget {
        MeetingTarget {
            thread_id: "19:meeting_abc@thread.v2".into(),
            tenant_id: "00000000-0000-0000-0000-000000000001".into(),
            organizer_id: "00000000-0000-0000-0000-000000000002".into(),
            meeting_data: None,
        }
    }

    fn option(name: &str, kind: &str, value: Value) -> Value {
        json!({"name": name, "type": kind, "currentValue": value, "category": "x", "isDisabled": false})
    }

    fn response() -> Value {
        json!({"options": [
            option("AutoAdmittedUsers", "List", json!("EveryoneInCompany")),
            option("PresenterOption", "List", json!("Everyone")),
            option("AttendeeRestrictions", "BitMask", json!("DisableAudio")),
            option("AllowMeetingChat", "List", json!("InMeeting")),
            option("AllowTeamsMeetingReactions", "Boolean", json!(true)),
            option("AutoRecordAndTranscribeMode", "List", json!("AutoTranscriptionOnly")),
        ], "meetingDetails": {"title": "Standup"}})
    }

    #[test]
    fn the_read_url_keeps_the_double_slash_and_the_write_url_adds_a_zero() {
        assert_eq!(
            read_url(&target()),
            "https://teams.microsoft.com/api/mt/emea//beta/meetings/v1/options/00000000-0000-0000-0000-000000000001/00000000-0000-0000-0000-000000000002/19%3Ameeting_abc%40thread.v2/0?monarch=true"
        );
        assert_eq!(
            write_url(&target()),
            "https://teams.cloud.microsoft/api/mt/emea/beta/meetings/v1/options/00000000-0000-0000-0000-000000000001/00000000-0000-0000-0000-000000000002/19%3Ameeting_abc%40thread.v2/0/0?monarch=true"
        );
    }

    #[test]
    fn the_read_answer_becomes_the_dialog_fields() {
        let options = MeetingOptions::from_response(&response()).unwrap();
        assert_eq!(options.bypass_lobby, "EveryoneInCompany");
        assert_eq!(options.presenters, "Everyone");
        assert!(!options.attendee_mic);
        assert!(options.attendee_camera);
        assert_eq!(options.meeting_chat, "InMeeting");
        assert!(options.reactions);
        assert!(!options.record_automatically);
        assert_eq!(options.record_mode(), "AutoTranscriptionOnly");
        assert!(MeetingOptions::from_response(&json!({"options": []})).is_none());
    }

    #[test]
    fn only_changed_options_are_written() {
        let before = MeetingOptions::from_response(&response()).unwrap();
        assert!(before.changes(&before.clone()).is_empty());
        let edited = MeetingOptions {
            bypass_lobby: "OrganizerOnly".into(),
            meeting_chat: "Disabled".into(),
            record_automatically: true,
            reactions: false,
            attendee_mic: true,
            attendee_camera: false,
            ..before.clone()
        };
        let writes = before.changes(&edited);
        let names: Vec<&str> = writes.iter().map(|write| write.name).collect();
        assert_eq!(names, ["AutoAdmittedUsers", "AllowMeetingChat", "AutoRecordAndTranscribeMode", "AllowTeamsMeetingReactions", "AttendeeRestrictions"]);
        assert_eq!(writes[2].value, json!("AutoRecordingAndTranscription"));
        assert_eq!(writes[4].value, json!("DisableVideo"));
    }

    #[test]
    fn a_write_sends_one_option_in_the_captured_shape() {
        let write = OptionWrite { name: "AllowMeetingChat", kind: "List", value: json!("Disabled") };
        assert_eq!(
            write.body(),
            json!({"options": [{"name": "AllowMeetingChat", "type": "List", "currentValue": "Disabled", "isRecommended": null}]})
        );
        assert_eq!(OptionWrite::restrictions(true, true).body()["options"][0]["currentValue"], "DisableAudio,DisableVideo");
        assert_eq!(OptionWrite::restrictions(false, false).value, json!("UnRestricted"));
    }

    #[test]
    fn a_refused_write_is_told_apart_from_an_accepted_one() {
        assert!(!write_failed(&json!({"success": true, "options": [{"success": true}]})));
        assert!(write_failed(&json!({"success": false})));
        assert!(write_failed(&json!({"success": true, "options": [{"success": false}]})));
    }
}
