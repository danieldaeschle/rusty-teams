use serde_json::{Value, json};

use crate::roster::{AttendeeLimits, MeetingRole};
use crate::signaling::{Callee, MEETING_MESSAGE_ID, MeetingTarget};
use crate::trouter_events::{CallbackLinks, find_key};

const SPOTLIGHT: &str = "spotlight";
const AUDIO: &str = "audio";
const SPECIFIED: &str = "specified";
const EVERYONE: &str = "all";
const PRESENTER: &str = "presenter";
const DISABLE_AUDIO: &str = "DisableAudio";
const DISABLE_VIDEO: &str = "DisableVideo";
const UNRESTRICTED: &str = "UnRestricted";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeetingCapability {
    pub locked: bool,
    pub attendee_mic_off: bool,
    pub attendee_camera_off: bool,
}

impl MeetingCapability {
    pub fn apply(&mut self, body: &Value) -> bool {
        let Some(capability) = find_key(body, "meetingCapability") else {
            return false;
        };
        let before = *self;
        if let Some(locked) = capability["lockMeeting"].as_bool() {
            self.locked = locked;
        }
        if let Some(restrictions) = capability["attendeeRestrictions"].as_str() {
            self.attendee_mic_off = restrictions.contains(DISABLE_AUDIO);
            self.attendee_camera_off = restrictions.contains(DISABLE_VIDEO);
        }
        *self != before
    }
}

pub fn restrictions_value(mic_off: bool, camera_off: bool) -> String {
    let flags: Vec<&str> = [(mic_off, DISABLE_AUDIO), (camera_off, DISABLE_VIDEO)]
        .into_iter()
        .filter_map(|(on, flag)| on.then_some(flag))
        .collect();
    if flags.is_empty() {
        UNRESTRICTED.to_owned()
    } else {
        flags.join(",")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modality {
    Mic,
    Camera,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateStep {
    Publish(&'static str),
    Remove(String),
}

impl Modality {
    fn blocked_type(self) -> &'static str {
        match self {
            Modality::Mic => "attendeeAudioRestricted",
            Modality::Camera => "attendeeVideoRestricted",
        }
    }

    fn released_type(self) -> &'static str {
        match self {
            Modality::Mic => "attendeeModalitiesUnrestricted",
            Modality::Camera => "attendeeVideoUnrestricted",
        }
    }

    fn attendees_off(self, capability: MeetingCapability) -> bool {
        match self {
            Modality::Mic => capability.attendee_mic_off,
            Modality::Camera => capability.attendee_camera_off,
        }
    }

    fn states(self, limits: &AttendeeLimits) -> (Option<&str>, Option<&str>) {
        let (blocked, released) = match self {
            Modality::Mic => (&limits.mic_blocked, &limits.mic_released),
            Modality::Camera => (&limits.camera_blocked, &limits.camera_released),
        };
        (
            blocked.as_ref().map(|state| state.state_id.as_str()),
            released.as_ref().map(|state| state.state_id.as_str()),
        )
    }
}

pub fn modality_allowed(modality: Modality, capability: MeetingCapability, limits: &AttendeeLimits) -> bool {
    let (blocked, released) = modality.states(limits);
    if blocked.is_some() {
        return false;
    }
    !modality.attendees_off(capability) || released.is_some()
}

pub fn modality_steps(modality: Modality, allowed: bool, capability: MeetingCapability, limits: &AttendeeLimits) -> Vec<StateStep> {
    let (blocked, released) = modality.states(limits);
    let attendees_off = modality.attendees_off(capability);
    let mut steps = Vec::new();
    if allowed {
        if let Some(state_id) = blocked {
            steps.push(StateStep::Remove(state_id.to_owned()));
        }
        if attendees_off && released.is_none() {
            steps.push(StateStep::Publish(modality.released_type()));
        }
    } else {
        if let Some(state_id) = released {
            steps.push(StateStep::Remove(state_id.to_owned()));
        }
        if !attendees_off && blocked.is_none() {
            steps.push(StateStep::Publish(modality.blocked_type()));
        }
    }
    steps
}

#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub mri: &'a str,
    pub display_name: &'a str,
}

impl Target<'_> {
    fn wire(&self) -> Value {
        json!({"id": self.mri, "displayName": self.display_name})
    }
}

pub fn admit_body(from: Value, target: Target<'_>, callbacks: &CallbackLinks, cause_id: &str) -> Value {
    json!({
        "participants": {"from": from, "to": [target.wire()]},
        "links": {
            "admitFailure": callbacks.conversation("admitParticipantFailure"),
            "admitSuccess": callbacks.conversation("admitParticipantSuccess"),
        },
        "debugContent": {"causeId": cause_id},
    })
}

pub fn admit_all_body(from: Value, callbacks: &CallbackLinks, operation_id: &str) -> Value {
    json!({
        "participants": {"from": from},
        "links": {"admitAllStatus": callbacks.conversation("admitAllStatus")},
        "operationId": operation_id,
        "debugContent": {"causeId": operation_id},
    })
}

pub fn remove_participant_body(from: Value, target: Target<'_>, callbacks: &CallbackLinks) -> Value {
    json!({
        "participants": {"from": from, "to": [target.wire()]},
        "links": {
            "removeParticipantSuccess": callbacks.conversation("removeParticipantSuccess"),
            "removeParticipantFailure": callbacks.conversation("removeParticipantFailure"),
        },
    })
}

pub fn mute_participants_body(from: Value, mris: &[String]) -> Value {
    mute_body(from, SPECIFIED, mris)
}

pub fn mute_everyone_body(from: Value, others: &[String]) -> Value {
    mute_body(from, EVERYONE, others)
}

fn mute_body(from: Value, scope: &str, mris: &[String]) -> Value {
    json!({
        "from": from,
        "scope": scope,
        "muteParticipants": mris.iter().map(|mri| json!({"id": mri})).collect::<Vec<_>>(),
        "mediaTypes": [AUDIO],
    })
}

pub fn spotlight_body(from: Value, sequence_number: u32, mri: &str, own_mri: &str) -> Value {
    let mut body = json!({
        "from": from,
        "publishedState": {
            "stateType": SPOTLIGHT,
            "level": "user",
            "content": {},
            "sequenceNumber": sequence_number,
        },
    });
    if mri != own_mri {
        body["scope"] = json!(SPECIFIED);
        body["to"] = json!([{"id": mri}]);
    }
    body
}

pub fn lock_body(from: Value, locked: bool, sequence_number: u32) -> Value {
    json!({"from": from, "meetingSettings": {"lockMeeting": locked}, "sequenceNumber": sequence_number})
}

pub fn role_body(from: Value, mri: &str, role: MeetingRole) -> Value {
    json!({
        "from": from,
        "scope": SPECIFIED,
        "to": [{"id": mri, "meetingRole": role.wire()}],
    })
}

pub fn attendee_state_body(from: Value, state_type: &str, sequence_number: u32, mri: &str) -> Value {
    json!({
        "from": from,
        "publishedState": {
            "stateType": state_type,
            "level": "user",
            "content": {},
            "sequenceNumber": sequence_number,
        },
        "scope": SPECIFIED,
        "to": [{"id": mri}],
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitee {
    pub callee: Callee,
    pub participant_id: String,
}

pub fn group_chat_for(thread_id: &str, meeting: bool) -> Value {
    json!({"threadId": thread_id, "messageId": if meeting { json!(MEETING_MESSAGE_ID) } else { Value::Null }})
}

pub fn add_link_name(has_thread: bool) -> &'static str {
    if has_thread { "addParticipantAndModality" } else { "addParticipant" }
}

pub fn add_participants_body(
    from: Value,
    invitees: &[Invitee],
    group_chat: Value,
    meeting: Option<&MeetingTarget>,
    callbacks: &CallbackLinks,
) -> Value {
    let to: Vec<Value> = invitees
        .iter()
        .map(|invitee| {
            let mut person = json!({
                "id": invitee.callee.mri,
                "displayName": invitee.callee.display_name,
                "participantId": invitee.participant_id,
            });
            if meeting.is_some() {
                person["invitationPreferences"] = json!({"requestedMeetingRoles": [PRESENTER]});
            }
            person
        })
        .collect();
    let invitation_data = match meeting {
        Some(meeting) => json!({"invitationData": {"meetingInfo": {"organizerId": meeting.organizer_id, "tenantId": meeting.tenant_id}}}),
        None => json!({}),
    };
    json!({
        "disableUnmute": false,
        "participants": {"from": from, "to": to},
        "participantInvitationData": invitation_data,
        "replacementDetails": null,
        "groupContext": null,
        "groupChat": group_chat,
        "links": {
            "addParticipantSuccess": callbacks.conversation("addParticipantSuccess"),
            "addParticipantFailure": callbacks.conversation("addParticipantFailure"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn callbacks() -> CallbackLinks {
        CallbackLinks::new("https://trouter.example/v4/f/x/", "call-1")
    }

    fn from() -> Value {
        json!({"id": "8:orgid:me"})
    }

    fn ana() -> Target<'static> {
        Target { mri: "8:orgid:ana", display_name: "Ana" }
    }

    #[test]
    fn admitting_one_names_the_person_and_both_callbacks() {
        let body = admit_body(from(), ana(), &callbacks(), "cause-1");
        assert_eq!(body["participants"]["from"]["id"], "8:orgid:me");
        assert_eq!(body["participants"]["to"], json!([{"id": "8:orgid:ana", "displayName": "Ana"}]));
        assert!(body["links"]["admitSuccess"].as_str().unwrap().contains("/conversation/admitParticipantSuccess/"));
        assert!(body["links"]["admitFailure"].as_str().unwrap().contains("/conversation/admitParticipantFailure/"));
        assert_eq!(body["debugContent"]["causeId"], "cause-1");
    }

    #[test]
    fn admitting_everyone_has_no_recipients() {
        let body = admit_all_body(from(), &callbacks(), "op-1");
        assert!(body["participants"].get("to").is_none());
        assert_eq!(body["operationId"], "op-1");
        assert!(body["links"]["admitAllStatus"].as_str().unwrap().contains("/conversation/admitAllStatus/"));
    }

    #[test]
    fn denying_and_removing_use_the_remove_body() {
        let body = remove_participant_body(from(), ana(), &callbacks());
        assert_eq!(body["participants"]["to"][0]["id"], "8:orgid:ana");
        assert!(body["links"]["removeParticipantSuccess"].as_str().unwrap().contains("/conversation/removeParticipantSuccess/"));
        assert!(body["links"]["removeParticipantFailure"].as_str().unwrap().contains("/conversation/removeParticipantFailure/"));
    }

    #[test]
    fn muting_one_or_all_only_changes_the_scope() {
        let one = mute_participants_body(from(), &["8:orgid:ana".to_owned()]);
        assert_eq!(one["scope"], "specified");
        assert_eq!(one["muteParticipants"], json!([{"id": "8:orgid:ana"}]));
        assert_eq!(one["mediaTypes"], json!(["audio"]));
        let everyone = mute_everyone_body(from(), &["8:orgid:ana".to_owned(), "8:orgid:bo".to_owned()]);
        assert_eq!(everyone["scope"], "all");
        assert_eq!(everyone["muteParticipants"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn spotlighting_someone_else_is_targeted_and_self_is_not() {
        let other = spotlight_body(from(), 4, "8:orgid:ana", "8:orgid:me");
        assert_eq!(other["publishedState"]["stateType"], "spotlight");
        assert_eq!(other["publishedState"]["level"], "user");
        assert_eq!(other["publishedState"]["sequenceNumber"], 4);
        assert_eq!(other["scope"], "specified");
        assert_eq!(other["to"], json!([{"id": "8:orgid:ana"}]));
        let own = spotlight_body(from(), 5, "8:orgid:me", "8:orgid:me");
        assert!(own.get("scope").is_none());
        assert!(own.get("to").is_none());
    }

    fn capability(mic_off: bool, camera_off: bool) -> MeetingCapability {
        MeetingCapability { locked: false, attendee_mic_off: mic_off, attendee_camera_off: camera_off }
    }

    fn limits_with(mic_blocked: bool, mic_released: bool) -> AttendeeLimits {
        let state = |id: &str| crate::roster::PublishedState { state_id: id.to_owned(), rank: 1 };
        AttendeeLimits {
            mic_blocked: mic_blocked.then(|| state("blocked-1")),
            mic_released: mic_released.then(|| state("released-1")),
            ..AttendeeLimits::default()
        }
    }

    #[test]
    fn locking_names_the_setting_and_the_sequence_number() {
        let body = lock_body(from(), true, 1);
        assert_eq!(body["meetingSettings"], json!({"lockMeeting": true}));
        assert_eq!(body["sequenceNumber"], 1);
        assert_eq!(body["from"]["id"], "8:orgid:me");
        assert_eq!(lock_body(from(), false, 2)["meetingSettings"]["lockMeeting"], false);
    }

    #[test]
    fn a_role_change_targets_one_person_with_the_wire_role() {
        let body = role_body(from(), "8:orgid:ana", MeetingRole::Presenter);
        assert_eq!(body["scope"], "specified");
        assert_eq!(body["to"], json!([{"id": "8:orgid:ana", "meetingRole": "presenter"}]));
        assert_eq!(role_body(from(), "8:orgid:ana", MeetingRole::Attendee)["to"][0]["meetingRole"], "attendee");
    }

    #[test]
    fn restricting_one_attendee_publishes_a_user_state_for_that_person() {
        let body = attendee_state_body(from(), "attendeeAudioRestricted", 7, "8:orgid:ana");
        assert_eq!(body["publishedState"]["stateType"], "attendeeAudioRestricted");
        assert_eq!(body["publishedState"]["level"], "user");
        assert_eq!(body["publishedState"]["sequenceNumber"], 7);
        assert_eq!(body["scope"], "specified");
        assert_eq!(body["to"], json!([{"id": "8:orgid:ana"}]));
    }

    #[test]
    fn inviting_into_a_meeting_names_the_organizer_and_asks_for_the_presenter_role() {
        let meeting = MeetingTarget { thread_id: "19:meeting_x@thread.v2".into(), tenant_id: "tenant-1".into(), organizer_id: "organizer-1".into(), meeting_data: None };
        let invitees = [Invitee { callee: Callee { mri: "8:orgid:ana".into(), display_name: "Ana".into() }, participant_id: "p-1".into() }];
        let body = add_participants_body(from(), &invitees, group_chat_for(&meeting.thread_id, true), Some(&meeting), &callbacks());
        assert_eq!(body["participants"]["to"][0]["id"], "8:orgid:ana");
        assert_eq!(body["participants"]["to"][0]["participantId"], "p-1");
        assert_eq!(body["participants"]["to"][0]["invitationPreferences"]["requestedMeetingRoles"], json!(["presenter"]));
        assert_eq!(body["participantInvitationData"]["invitationData"]["meetingInfo"], json!({"organizerId": "organizer-1", "tenantId": "tenant-1"}));
        assert_eq!(body["groupChat"], json!({"threadId": "19:meeting_x@thread.v2", "messageId": "0"}));
        assert!(body["links"]["addParticipantSuccess"].as_str().unwrap().contains("/conversation/addParticipantSuccess/"));
        assert!(body["links"]["addParticipantFailure"].as_str().unwrap().contains("/conversation/addParticipantFailure/"));
    }

    #[test]
    fn inviting_into_a_group_call_has_no_meeting_data_and_a_null_message() {
        let invitees = [Invitee { callee: Callee { mri: "8:orgid:bo".into(), display_name: "Bo".into() }, participant_id: "p-2".into() }];
        let body = add_participants_body(from(), &invitees, group_chat_for("19:group@thread.v2", false), None, &callbacks());
        assert_eq!(body["participantInvitationData"], json!({}));
        assert!(body["participants"]["to"][0].get("invitationPreferences").is_none());
        assert_eq!(body["groupChat"], json!({"threadId": "19:group@thread.v2", "messageId": null}));
        assert_eq!(add_link_name(true), "addParticipantAndModality");
        assert_eq!(add_link_name(false), "addParticipant");
    }

    #[test]
    fn the_restriction_option_combines_both_flags() {
        assert_eq!(restrictions_value(false, false), "UnRestricted");
        assert_eq!(restrictions_value(true, false), "DisableAudio");
        assert_eq!(restrictions_value(false, true), "DisableVideo");
        assert_eq!(restrictions_value(true, true), "DisableAudio,DisableVideo");
    }

    #[test]
    fn the_capability_follows_a_conversation_update() {
        let mut state = MeetingCapability::default();
        let update = json!({"meetingDetails": {"meetingCapability": {"lockMeeting": true, "attendeeRestrictions": "DisableAudio,DisableVideo"}}});
        assert!(state.apply(&update));
        assert_eq!(state, MeetingCapability { locked: true, attendee_mic_off: true, attendee_camera_off: true });
        assert!(!state.apply(&update));
        assert!(!state.apply(&json!({"other": 1})));
        assert!(state.apply(&json!({"meetingCapability": {"lockMeeting": false, "attendeeRestrictions": "UnRestricted"}})));
        assert_eq!(state, MeetingCapability::default());
    }

    #[test]
    fn a_mic_is_allowed_unless_blocked_or_the_meeting_blocks_everyone() {
        let none = AttendeeLimits::default();
        assert!(modality_allowed(Modality::Mic, capability(false, false), &none));
        assert!(!modality_allowed(Modality::Mic, capability(true, false), &none));
        assert!(modality_allowed(Modality::Mic, capability(true, false), &limits_with(false, true)));
        assert!(!modality_allowed(Modality::Mic, capability(false, false), &limits_with(true, false)));
        assert!(modality_allowed(Modality::Camera, capability(true, false), &none));
    }

    #[test]
    fn disabling_one_mic_publishes_a_restriction_or_drops_the_release() {
        let open = capability(false, false);
        assert_eq!(modality_steps(Modality::Mic, false, open, &AttendeeLimits::default()), vec![StateStep::Publish("attendeeAudioRestricted")]);
        let closed = capability(true, false);
        assert_eq!(modality_steps(Modality::Mic, false, closed, &limits_with(false, true)), vec![StateStep::Remove("released-1".into())]);
        assert!(modality_steps(Modality::Mic, false, closed, &AttendeeLimits::default()).is_empty());
        assert!(modality_steps(Modality::Mic, false, open, &limits_with(true, false)).is_empty());
    }

    #[test]
    fn allowing_one_mic_removes_the_restriction_or_publishes_a_release() {
        let open = capability(false, false);
        assert_eq!(modality_steps(Modality::Mic, true, open, &limits_with(true, false)), vec![StateStep::Remove("blocked-1".into())]);
        let closed = capability(true, false);
        assert_eq!(modality_steps(Modality::Mic, true, closed, &AttendeeLimits::default()), vec![StateStep::Publish("attendeeModalitiesUnrestricted")]);
        assert_eq!(modality_steps(Modality::Camera, true, capability(false, true), &AttendeeLimits::default()), vec![StateStep::Publish("attendeeVideoUnrestricted")]);
        assert!(modality_steps(Modality::Mic, true, open, &AttendeeLimits::default()).is_empty());
    }
}
