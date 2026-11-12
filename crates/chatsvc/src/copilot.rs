use serde_json::{Value, json};
use session::{NestedApp, Request, Scope, Session, SignalrExchange};
use uuid::Uuid;

use crate::error::{Error, Result};

const CHATHUB_URL: &str = "https://substrate.office.com/m365Copilot/Chathub";
const SYDNEY_RESOURCE: &str = "https://substrate.office.com/sydney";
const SYDNEY_SCOPE: &str = "sydney.readwrite";
const COPILOT_APP: NestedApp = NestedApp {
    client_id: "c0ab8ce9-e9a0-42e7-b064-33d422df41f1",
    redirect_uri: "brk-multihub://m365copilotapp.svc.cloud.microsoft",
    broker_redirect_uri: "https://teams.cloud.microsoft/v2/authv2",
};
const HOST_PARAMETERS: &str = "source=%22officeweb%22&product=Office&agentHost=Bizchat.FullScreen&licenseType=Premium&isEdu=false&agent=work&scenario=officeweb";
const HANDSHAKE: &str = r#"{"protocol":"json","version":1}"#;
const INVOCATION: u8 = 4;
const COMPLETION: u8 = 2;
const CLOSE: u8 = 7;
const ANSWER_TIMEOUT_MS: u64 = 55_000;
const SOURCE: &str = "teamshub";
const CITATION_OPEN: char = '\u{e200}';
const CITATION_CLOSE: char = '\u{e201}';
const INLINE_TAGS: [&str; 6] = ["Person", "TeamsMessage", "File", "Event", "Email", "Chat"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotAsk {
    pub user_id: String,
    pub tenant_id: String,
    pub chat_id: String,
    pub prompt: String,
    pub locale: String,
    pub utc_offset_hours: i32,
}

pub struct Copilot {
    session: Session,
}

impl Copilot {
    pub fn new(session: &Session) -> Self {
        Copilot {
            session: session.clone(),
        }
    }

    pub async fn ask(&self, ask: &CopilotAsk) -> Result<String> {
        let session_id = Uuid::new_v4().to_string();
        let request_id = Uuid::new_v4().simple().to_string();
        let request = Request::signalr(
            chathub_url(ask, &session_id, &request_id),
            SignalrExchange {
                handshake: HANDSHAKE.to_owned(),
                invocations: vec![invocation(ask, &session_id, &request_id).to_string()],
                done_types: vec![COMPLETION, CLOSE],
                timeout_ms: ANSWER_TIMEOUT_MS,
            },
        );
        let response = self
            .session
            .send(
                request,
                &Scope::new(SYDNEY_RESOURCE, SYDNEY_SCOPE).for_nested_app(COPILOT_APP),
            )
            .await?;
        answer_of(&response.body)
    }
}

fn chathub_url(ask: &CopilotAsk, session_id: &str, request_id: &str) -> String {
    let route = format!("{}@{}", ask.user_id, ask.tenant_id);
    format!(
        "{CHATHUB_URL}/{route}?chatsessionid={request_id}&XRoutingParameterSessionKey={user}&clientrequestid={request_id}&X-SessionId={session_id}&ConversationId={conversation}&{HOST_PARAMETERS}",
        user = ask.user_id,
        conversation = Uuid::new_v4(),
    )
}

pub fn invocation(ask: &CopilotAsk, session_id: &str, request_id: &str) -> Value {
    json!({
        "type": INVOCATION,
        "invocationId": "0",
        "target": "chat",
        "arguments": [{
            "source": SOURCE,
            "clientCorrelationId": request_id,
            "sessionId": session_id,
            "optionsSets": ["enterprise_flux_work", "rich_responses"],
            "streamingMode": "ConciseWithPadding",
            "allowedMessageTypes": ["Chat", "Disengaged", "Progress", "EndOfRequest"],
            "requestId": request_id,
            "traceId": request_id,
            "isStartOfSession": true,
            "clientInfo": {
                "clientPlatform": "web",
                "clientAppName": "Teams",
                "clientEntrypoint": "ChatPanel",
                "clientSessionId": session_id,
                "ProductCategory": "Chat",
                "clientAppType": "Web",
                "productEntryPoint": "ChatPanel",
            },
            "message": {
                "author": "user",
                "inputMethod": "Keyboard",
                "text": ask.prompt,
                "requestId": request_id,
                "locale": ask.locale,
                "locationInfo": {"timeZoneOffset": ask.utc_offset_hours},
                "messageType": "Chat",
                "experienceType": "Default",
                "messageAnnotations": [{
                    "id": ask.chat_id,
                    "text": ask.chat_id,
                    "messageAnnotationAppContext": "Teams",
                    "messageAnnotationSource": "ClientAnnotated",
                    "messageAnnotationMetadata": {"@type": "Chat", "annotationType": "Chat"},
                    "messageAnnotationType": "Chat",
                }],
            },
        }],
    })
}

pub fn answer_of(received: &Value) -> Result<String> {
    let messages = received.as_array().map(Vec::as_slice).unwrap_or_default();
    let completion = messages
        .iter()
        .find(|message| message["type"].as_u64() == Some(u64::from(COMPLETION)))
        .ok_or_else(|| Error::UnexpectedAnswer(close_reason(messages)))?;
    let item = &completion["item"];
    let answer = item["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["author"] == "bot")
        .filter(|message| message["messageType"].is_null() || message["messageType"] == "Chat")
        .filter_map(|message| message["text"].as_str())
        .next_back()
        .map(clean_answer)
        .filter(|text| !text.is_empty());
    answer.ok_or_else(|| {
        let reason = item["result"]["message"]
            .as_str()
            .or_else(|| item["result"]["value"].as_str())
            .unwrap_or("Copilot gave no answer");
        Error::UnexpectedAnswer(reason.to_owned())
    })
}

fn close_reason(messages: &[Value]) -> String {
    messages
        .iter()
        .find_map(|message| message["error"].as_str())
        .unwrap_or("Copilot did not answer")
        .to_owned()
}

pub fn clean_answer(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut in_citation = false;
    for character in text.chars() {
        match character {
            CITATION_OPEN => in_citation = true,
            CITATION_CLOSE => in_citation = false,
            _ if !in_citation => cleaned.push(character),
            _ => {}
        }
    }
    for tag in INLINE_TAGS {
        cleaned = cleaned
            .replace(&format!("<{tag}>"), "")
            .replace(&format!("</{tag}>"), "");
    }
    cleaned.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask() -> CopilotAsk {
        CopilotAsk {
            user_id: "user-1".into(),
            tenant_id: "tenant-1".into(),
            chat_id: "19:chat@thread.v2".into(),
            prompt: "Summarize this chat".into(),
            locale: "en-us".into(),
            utc_offset_hours: 1,
        }
    }

    #[test]
    fn the_chat_rides_along_as_a_message_annotation() {
        let body = invocation(&ask(), "session-1", "request-1");
        let message = &body["arguments"][0]["message"];
        assert_eq!(body["target"], "chat");
        assert_eq!(message["text"], "Summarize this chat");
        assert_eq!(message["messageAnnotations"][0]["id"], "19:chat@thread.v2");
        assert_eq!(
            message["messageAnnotations"][0]["messageAnnotationType"],
            "Chat"
        );
    }

    #[test]
    fn the_url_routes_to_the_user_in_the_tenant() {
        let url = chathub_url(&ask(), "session-1", "request-1");
        assert!(
            url.starts_with("https://substrate.office.com/m365Copilot/Chathub/user-1@tenant-1?")
        );
        assert!(url.contains("X-SessionId=session-1"));
    }

    #[test]
    fn the_last_bot_chat_message_is_the_answer() {
        let received = json!([
            {"type": 1, "target": "update", "arguments": [{"messages": [{"text": "Digging in", "author": "bot", "messageType": "Progress"}]}]},
            {"type": 2, "invocationId": "0", "item": {"messages": [
                {"text": "Summarize this chat", "author": "user"},
                {"text": "Working", "author": "bot", "messageType": "Progress"},
                {"text": "### Chat summary\n\n- <Person>Ada</Person> shipped it. \u{e200}cite\u{e202}turn1search1\u{e201}", "author": "bot"}
            ], "result": {"value": "Success"}}}
        ]);
        assert_eq!(
            answer_of(&received).unwrap(),
            "### Chat summary\n\n- Ada shipped it."
        );
    }

    #[test]
    fn a_refusal_carries_the_service_reason() {
        let received = json!([
            {"type": 2, "item": {"messages": [{"text": "Hi", "author": "user"}], "result": {"value": "Throttled", "message": "Too many requests"}}}
        ]);
        assert_eq!(
            answer_of(&received).unwrap_err().to_string(),
            "unexpected service answer: Too many requests"
        );
    }

    #[test]
    fn a_closed_hub_reports_its_error() {
        let received = json!([{"type": 7, "error": "Unauthorized"}]);
        assert_eq!(
            answer_of(&received).unwrap_err().to_string(),
            "unexpected service answer: Unauthorized"
        );
    }
}
