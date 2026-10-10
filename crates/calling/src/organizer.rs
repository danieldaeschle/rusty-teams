use serde_json::{Value, json};

use crate::trouter_events::CallbackLinks;

const SPOTLIGHT: &str = "spotlight";
const AUDIO: &str = "audio";
const SPECIFIED: &str = "specified";
const EVERYONE: &str = "all";

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
}
