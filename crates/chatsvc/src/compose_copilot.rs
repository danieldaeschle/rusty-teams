use base64::Engine as _;
use serde_json::{Value, json};
use session::{AugloopExchange, AugloopStage, Request, Scope, Session};
use uuid::Uuid;

use crate::error::{Error, Result};

const AUGLOOP_URL: &str = "wss://augloop.office.com/";
const AUGLOOP_RESOURCE: &str = "https://augloop.office.com/v2";
const AUGLOOP_SCOPE: &str = ".default";
const TOKEN_PLACEHOLDER: &str = "@@augloop-token@@";
const DONE_MARKER: &str = "\"isCompleted\":true";
const ANSWER_TIMEOUT_MS: u64 = 55_000;
const KEEPALIVE_MS: u64 = 30_000;
const RUNTIME_VERSION: &str = "2.37.2676";
const DOCUMENT_ID: &str = "doc";
const INIT_MESSAGE_ID: &str = "c1";
const SEED_MESSAGE_ID: &str = "c2";
const ANNOTATION_TYPE: &str = "AugLoop_TeamsCopilot_TeamsCopilotComposeRewriteStreamingAnnotation";
const SIGNAL_TYPE: &str = "AugLoop_TeamsCopilot_TeamsCopilotComposeRewriteSignal";
const FLIGHTS: &str = "Microsoft.Teams.Augloop.EnableSendSubstrateEvents:true;Microsoft.Teams.Augloop.AllowPartialResults:true;Microsoft.Teams.Augloop.DefaultModelOverride:GPT41_ShortCo_0414;Microsoft.Teams.Augloop.FLIGHT_OMNI_MIGRATION:true;Microsoft.Teams.Augloop.EnableCopilotOrTeamsPremiumLicenseAggregation:true;Microsoft.Teams.Augloop.RingInfo:general;Microsoft.Teams.Augloop.EnableTeamsAuthSvcLicensingChecks:true;_acceptsClaimsChallengeMessages;_acceptsSeedingStatusChangeMessages";

const DEFAULT_TONE: &str = "default";
const PROOFREAD_TONE: &str = "PROOFREAD_SCENARIO";
const MEDIUM_LENGTH: &str = "medium";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjustment {
    Concise,
    Longer,
    Casual,
    Professional,
    Confident,
    Enthusiastic,
}

impl Adjustment {
    pub const ALL: [Adjustment; 6] = [
        Adjustment::Concise,
        Adjustment::Longer,
        Adjustment::Casual,
        Adjustment::Professional,
        Adjustment::Confident,
        Adjustment::Enthusiastic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Adjustment::Concise => "Concise",
            Adjustment::Longer => "Longer",
            Adjustment::Casual => "Casual",
            Adjustment::Professional => "Professional",
            Adjustment::Confident => "Confident",
            Adjustment::Enthusiastic => "Enthusiastic",
        }
    }
}

/// The "Write with Copilot" choices with the tone and length Teams sends for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeAction {
    Rewrite,
    Proofread,
    Adjust(Adjustment),
    Custom(String),
}

impl ComposeAction {
    pub fn tone(&self) -> &'static str {
        match self {
            ComposeAction::Proofread => PROOFREAD_TONE,
            ComposeAction::Adjust(Adjustment::Casual) => "casual",
            ComposeAction::Adjust(Adjustment::Professional) => "professional",
            ComposeAction::Adjust(Adjustment::Confident) => "confident",
            ComposeAction::Adjust(Adjustment::Enthusiastic) => "enthusiastic",
            _ => DEFAULT_TONE,
        }
    }

    pub fn length(&self) -> &'static str {
        match self {
            ComposeAction::Adjust(Adjustment::Concise) => "short",
            ComposeAction::Adjust(Adjustment::Longer) => "long",
            _ => MEDIUM_LENGTH,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ComposeAction::Rewrite => "Rewrite",
            ComposeAction::Proofread => "Proofread",
            ComposeAction::Adjust(adjustment) => adjustment.label(),
            ComposeAction::Custom(prompt) => prompt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeRewrite {
    pub tenant_id: String,
    pub thread_id: String,
    pub user_html: String,
    pub action: ComposeAction,
    pub ui_language: String,
    pub time_zone: String,
}

pub struct ComposeCopilot {
    session: Session,
}

impl ComposeCopilot {
    pub fn new(session: &Session) -> Self {
        ComposeCopilot {
            session: session.clone(),
        }
    }

    pub async fn rewrite(&self, rewrite: &ComposeRewrite) -> Result<String> {
        let request = Request::augloop(AUGLOOP_URL, exchange(rewrite, &Uuid::new_v4().to_string()));
        let response = self
            .session
            .send(request, &Scope::new(AUGLOOP_RESOURCE, AUGLOOP_SCOPE))
            .await?;
        answer_of(&response.body)
    }
}

fn correlation_vector() -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(Uuid::new_v4().as_bytes())
}

fn header(type_name: &str, base_types: &[&str]) -> Value {
    json!({"T_": type_name, "B_": base_types})
}

pub fn exchange(rewrite: &ComposeRewrite, session_id: &str) -> AugloopExchange {
    let vector = correlation_vector();
    let init = json!({
        "protocolVersion": 2,
        "clientMetadata": {
            "appName": "Teams",
            "appPlatform": "Web",
            "uiLanguage": rewrite.ui_language,
            "flights": FLIGHTS,
            "releaseAudienceGroup": "Production",
            "releaseChannel": "general",
            "sessionId": session_id,
            "runtimeVersion": RUNTIME_VERSION,
            "docSessionId": Uuid::new_v4().to_string(),
            "userSystemTimezone": rewrite.time_zone,
        },
        "extensionConfigs": [],
        "returnWorkflowInputTypes": true,
        "enableRemoteExecutionNotification": false,
        "createBlobStorageContainer": false,
        "H_": header("AugLoop_Session_Protocol_SessionInitMessage", &["AugLoop_Session_Protocol_Message"]),
        "cv": format!("{vector}.1"),
        "messageId": INIT_MESSAGE_ID,
    });
    let seed = json!({
        "cv": correlation_vector(),
        "seq": 0,
        "ops": [{
            "parentPath": ["session"],
            "items": [{"id": DOCUMENT_ID, "body": {
                "isReadonly": false,
                "H_": header("AugLoop_Core_Document", &["AugLoop_Core_TileGroup"]),
            }}],
            "H_": add_operation_header(),
        }],
        "groupId": "Seed",
        "groupSize": 1,
        "groupComplete": true,
        "H_": header("AugLoop_Session_Protocol_SyncMessage", &["AugLoop_Session_Protocol_Message"]),
        "messageId": SEED_MESSAGE_ID,
    });
    let activation = json!({
        "annotationType": ANNOTATION_TYPE,
        "token": format!("{ANNOTATION_TYPE}-0"),
        "ignoreExistingAnnotations": false,
        "returnAnnotationDoesNotExist": true,
        "sendApologies": false,
        "H_": header("AugLoop_Session_Protocol_AnnotationActivationMessage", &["AugLoop_Session_Protocol_Message"]),
        "cv": format!("{vector}.2"),
        "messageId": "c3",
    });
    let token = json!({
        "authToken": TOKEN_PLACEHOLDER,
        "version": 1,
        "H_": header("AugLoop_Session_Protocol_TokenProvisionMessage", &["AugLoop_Session_Protocol_Message"]),
        "cv": format!("{vector}.3"),
        "messageId": "c4",
    });
    AugloopExchange {
        stages: vec![
            AugloopStage {
                send: vec![init.to_string()],
                await_message_id: Some(INIT_MESSAGE_ID.to_owned()),
            },
            AugloopStage {
                send: vec![seed.to_string(), activation.to_string(), token.to_string()],
                await_message_id: Some(SEED_MESSAGE_ID.to_owned()),
            },
            AugloopStage {
                send: vec![signal(rewrite).to_string()],
                await_message_id: None,
            },
        ],
        placeholder: TOKEN_PLACEHOLDER.to_owned(),
        done_marker: DONE_MARKER.to_owned(),
        timeout_ms: ANSWER_TIMEOUT_MS,
        keepalive_ms: KEEPALIVE_MS,
    }
}

fn add_operation_header() -> Value {
    header(
        "AugLoop_Core_AddOperation",
        &["AugLoop_Core_OperationWithSiblingContext", "AugLoop_Core_Operation"],
    )
}

pub fn signal(rewrite: &ComposeRewrite) -> Value {
    let mut body = json!({
        "userText": rewrite.user_html,
        "tone": rewrite.action.tone(),
        "length": rewrite.action.length(),
        "requestContext": {
            "threadId": rewrite.thread_id,
            "contextType": "Chat",
            "tenantId": rewrite.tenant_id,
        },
        "streamResponse": true,
        "H_": header(SIGNAL_TYPE, &["AugLoop_TeamsCopilot_TeamsCopilotBaseSignal", "AugLoop_Signals_Signal"]),
    });
    if let ComposeAction::Custom(prompt) = &rewrite.action {
        body["customPrompt"] = json!(prompt);
    }
    json!({
        "cv": correlation_vector(),
        "seq": 1,
        "ops": [{
            "parentPath": [
                "session",
                DOCUMENT_ID,
                format!("teams_compose_copilot_streaming_{}", Uuid::new_v4()),
                Uuid::new_v4().to_string(),
            ],
            "items": [{"id": Uuid::new_v4().to_string(), "body": body, "contextId": "C1"}],
            "H_": add_operation_header(),
        }],
        "H_": header("AugLoop_Session_Protocol_SyncMessage", &["AugLoop_Session_Protocol_Message"]),
        "messageId": "c5",
    })
}

fn annotation_bodies(received: &Value) -> impl Iterator<Item = &Value> {
    received
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["annotationType"] == ANNOTATION_TYPE)
        .flat_map(|message| message["ops"].as_array().into_iter().flatten())
        .flat_map(|operation| operation["items"].as_array().into_iter().flatten())
        .map(|item| &item["body"])
}

pub fn answer_of(received: &Value) -> Result<String> {
    let completed = annotation_bodies(received).find(|body| body["isCompleted"] == true);
    let Some(body) = completed else {
        return Err(Error::UnexpectedAnswer(failure_reason(received)));
    };
    if body["status"].as_i64().unwrap_or(0) != 0 {
        return Err(Error::UnexpectedAnswer(format!("Copilot status {}", body["status"])));
    }
    body["answer"]
        .as_str()
        .map(str::trim)
        .filter(|answer| !answer.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Error::UnexpectedAnswer("Copilot gave no answer".to_owned()))
}

fn failure_reason(received: &Value) -> String {
    received
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|message| message["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "Copilot did not answer".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite() -> ComposeRewrite {
        ComposeRewrite {
            tenant_id: "tenant-1".into(),
            thread_id: "19:chat@thread.v2".into(),
            user_html: "<p>hallo</p>".into(),
            action: ComposeAction::Rewrite,
            ui_language: "en-gb".into(),
            time_zone: "Europe/Berlin".into(),
        }
    }

    #[test]
    fn stages_init_then_seed_with_token_then_signal() {
        let exchange = exchange(&rewrite(), "session-1");
        assert_eq!(exchange.stages.len(), 3);
        assert_eq!(exchange.stages[0].await_message_id.as_deref(), Some("c1"));
        let init: Value = serde_json::from_str(&exchange.stages[0].send[0]).unwrap();
        assert_eq!(init["clientMetadata"]["sessionId"], "session-1");
        let token: Value = serde_json::from_str(&exchange.stages[1].send[2]).unwrap();
        assert_eq!(token["authToken"], TOKEN_PLACEHOLDER);
        let signal: Value = serde_json::from_str(&exchange.stages[2].send[0]).unwrap();
        let body = &signal["ops"][0]["items"][0]["body"];
        assert_eq!(body["userText"], "<p>hallo</p>");
        assert_eq!(body["requestContext"]["threadId"], "19:chat@thread.v2");
        assert_eq!((body["tone"].as_str(), body["length"].as_str()), (Some("default"), Some("medium")));
        assert!(body.get("customPrompt").is_none());
    }

    #[test]
    fn actions_send_the_tone_and_length_teams_sends() {
        let sent = |action: ComposeAction| {
            let body = signal(&ComposeRewrite { action, ..rewrite() })["ops"][0]["items"][0]["body"].clone();
            (body["tone"].as_str().unwrap().to_owned(), body["length"].as_str().unwrap().to_owned(), body.get("customPrompt").cloned())
        };
        assert_eq!(sent(ComposeAction::Proofread), ("PROOFREAD_SCENARIO".into(), "medium".into(), None));
        assert_eq!(sent(ComposeAction::Adjust(Adjustment::Concise)), ("default".into(), "short".into(), None));
        assert_eq!(sent(ComposeAction::Adjust(Adjustment::Longer)), ("default".into(), "long".into(), None));
        assert_eq!(sent(ComposeAction::Adjust(Adjustment::Confident)), ("confident".into(), "medium".into(), None));
        assert_eq!(
            sent(ComposeAction::Custom("as a haiku".into())),
            ("default".into(), "medium".into(), Some(json!("as a haiku")))
        );
    }

    #[test]
    fn the_completed_annotation_carries_the_answer() {
        let part = |body: Value| json!({"annotationType": ANNOTATION_TYPE, "ops": [{"items": [{"body": body}]}]});
        let received = json!([
            {"messageId": "c1"},
            part(json!({"answerPart": "<p>Hallo", "isCompleted": false})),
            part(json!({"status": 0, "answerPart": "", "answer": "<p>Hallo zusammen</p>", "isCompleted": true})),
        ]);
        assert_eq!(answer_of(&received).unwrap(), "<p>Hallo zusammen</p>");
    }

    #[test]
    fn a_missing_answer_reports_the_socket_error() {
        let received = json!([{"messageId": "c1", "error": "Unauthorized"}]);
        assert_eq!(
            answer_of(&received).unwrap_err().to_string(),
            "unexpected service answer: Unauthorized"
        );
    }
}
