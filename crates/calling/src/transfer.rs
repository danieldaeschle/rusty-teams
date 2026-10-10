use serde_json::{Value, json};

use crate::signaling::Participant;
use crate::trouter_events::CallbackLinks;

#[derive(Debug, Clone, Copy)]
pub struct TransferTarget<'a> {
    pub mri: &'a str,
    pub replaces: Option<&'a str>,
}

fn transferor(from: &Participant) -> Value {
    json!({
        "details": {
            "id": from.mri,
            "endpointId": from.endpoint_id,
            "participantId": from.participant_id,
            "languageId": from.language_id,
        },
        "authorizationToken": null,
    })
}

pub fn transfer_body(from: &Participant, target: TransferTarget<'_>, callbacks: &CallbackLinks) -> Value {
    let replacement = target.replaces.map(|replaces| json!({"replaces": replaces}));
    json!({
        "callTransfer": {
            "target": {"id": target.mri},
            "transferor": transferor(from),
            "links": {
                "transferAcceptance": callbacks.call("transferAcceptance"),
                "transferCompletion": callbacks.call("transferCompletion"),
            },
            "replacementDetails": replacement,
            "disableForwardingAndUnanswered": false,
            "transferContext": null,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferEvent {
    Accepted,
    Completed { code: i64, sub_code: i64 },
}

impl TransferEvent {
    pub fn succeeded(&self) -> bool {
        matches!(self, TransferEvent::Completed { code, .. } if *code == 0 || (200..300).contains(code))
    }
}

pub fn completion(body: &Value) -> TransferEvent {
    let completed = &body["transferCompletion"];
    TransferEvent::Completed {
        code: completed["code"].as_i64().unwrap_or_default(),
        sub_code: completed["subCode"].as_i64().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Participant {
        Participant {
            mri: "8:orgid:me".into(),
            display_name: "Me".into(),
            endpoint_id: "ep-1".into(),
            participant_id: "part-1".into(),
            language_id: "en-gb".into(),
        }
    }

    fn callbacks() -> CallbackLinks {
        CallbackLinks::new("https://trouter.example/v4/f/x/", "call-1")
    }

    #[test]
    fn a_blind_transfer_names_the_target_and_the_transferor() {
        let body = transfer_body(&me(), TransferTarget { mri: "8:orgid:ana", replaces: None }, &callbacks());
        let transfer = &body["callTransfer"];
        assert_eq!(transfer["target"], json!({"id": "8:orgid:ana"}));
        assert_eq!(transfer["transferor"]["details"], json!({"id": "8:orgid:me", "endpointId": "ep-1", "participantId": "part-1", "languageId": "en-gb"}));
        assert!(transfer["transferor"]["authorizationToken"].is_null());
        assert!(transfer["replacementDetails"].is_null());
        assert!(transfer["links"]["transferAcceptance"].as_str().unwrap().contains("/call/transferAcceptance/"));
        assert!(transfer["links"]["transferCompletion"].as_str().unwrap().contains("/call/transferCompletion/"));
        assert_eq!(transfer["disableForwardingAndUnanswered"], false);
    }

    #[test]
    fn a_consulted_transfer_replaces_the_consult_call() {
        let body = transfer_body(&me(), TransferTarget { mri: "8:orgid:ana", replaces: Some("https://cc.skype.com/replacement") }, &callbacks());
        assert_eq!(body["callTransfer"]["replacementDetails"], json!({"replaces": "https://cc.skype.com/replacement"}));
    }

    #[test]
    fn the_completion_callback_tells_success_from_failure() {
        let done = completion(&json!({"transferCompletion": {"code": 0, "subCode": 0, "resultCategories": ["Success"]}}));
        assert!(done.succeeded());
        let failed = completion(&json!({"transferCompletion": {"code": 603, "subCode": 10603}}));
        assert_eq!(failed, TransferEvent::Completed { code: 603, sub_code: 10603 });
        assert!(!failed.succeeded());
        assert!(!TransferEvent::Accepted.succeeded());
    }
}
