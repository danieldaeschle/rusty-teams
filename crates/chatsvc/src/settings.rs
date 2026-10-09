use serde_json::{Map, Value, json};
use session::{Method, Request, Session};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, ensure_success};
use crate::pins::DEFAULT_REGION;

const SETTINGS_KEY: &str = "simpleCollabUserSettings";
const SETTINGS_ID: &str = "simpleCollabUserSettingsId";
const MUTED_FIELD: &str = "hasUserEnabledMutedChatSection";
const MEETING_FIELD: &str = "hasUserEnabledMeetingChatSection";
const MUTED_DEFAULT: bool = true;
const MEETING_DEFAULT: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSection {
    Muted,
    Meeting,
}

impl ChatSection {
    fn field(self) -> &'static str {
        match self {
            ChatSection::Muted => MUTED_FIELD,
            ChatSection::Meeting => MEETING_FIELD,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChatSectionSettings {
    pub muted: Option<bool>,
    pub meeting: Option<bool>,
}

impl ChatSectionSettings {
    pub fn enabled(&self, section: ChatSection) -> bool {
        match section {
            ChatSection::Muted => self.muted.unwrap_or(MUTED_DEFAULT),
            ChatSection::Meeting => self.meeting.unwrap_or(MEETING_DEFAULT),
        }
    }

    pub fn set(&mut self, section: ChatSection, enabled: bool) {
        match section {
            ChatSection::Muted => self.muted = Some(enabled),
            ChatSection::Meeting => self.meeting = Some(enabled),
        }
    }
}

pub struct UserSettings<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    base_url: String,
}

impl UserSettings<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        UserSettings::with_transport(SessionMessageTransport::new(session), region)
    }
}

impl<T: MessageTransport> UserSettings<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        UserSettings {
            transport,
            base_url: format!(
                "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/properties"
            ),
        }
    }

    pub async fn chat_sections(&self) -> Result<ChatSectionSettings> {
        Ok(parse_chat_sections(&self.properties().await?))
    }

    pub async fn set_chat_section(&self, section: ChatSection, enabled: bool) -> Result<()> {
        let properties = self.properties().await?;
        let merged = merge_chat_section(&properties, section, enabled);
        let url = format!("{}?name={SETTINGS_KEY}", self.base_url);
        let body = json!({ SETTINGS_KEY: merged });
        let answer = self
            .transport
            .send(Request::with_body(Method::Put, url, body))
            .await?;
        ensure_success(&answer)
    }

    async fn properties(&self) -> Result<Value> {
        let answer = self.transport.send(Request::get(&self.base_url)).await?;
        ensure_success(&answer)?;
        if answer.body.is_object() {
            Ok(answer.body)
        } else {
            Err(Error::UnexpectedAnswer("no user properties".into()))
        }
    }
}

fn stored_settings(properties: &Value) -> Map<String, Value> {
    properties
        .get(SETTINGS_KEY)
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

pub fn parse_chat_sections(properties: &Value) -> ChatSectionSettings {
    let settings = stored_settings(properties);
    ChatSectionSettings {
        muted: settings.get(MUTED_FIELD).and_then(Value::as_bool),
        meeting: settings.get(MEETING_FIELD).and_then(Value::as_bool),
    }
}

pub fn merge_chat_section(properties: &Value, section: ChatSection, enabled: bool) -> String {
    let mut settings = stored_settings(properties);
    settings
        .entry("id")
        .or_insert_with(|| Value::String(SETTINGS_ID.to_owned()));
    settings.insert(section.field().to_owned(), Value::Bool(enabled));
    Value::Object(settings).to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    struct Canned {
        answers: Mutex<VecDeque<(u16, Value)>>,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            let (status, body) = self.answers.lock().unwrap().pop_front().expect("exhausted");
            Ok(ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    fn properties(settings: &str) -> Value {
        json!({"other": "kept", SETTINGS_KEY: settings})
    }

    #[test]
    fn unset_fields_use_the_teams_defaults() {
        let parsed = parse_chat_sections(&json!({}));
        assert_eq!(parsed, ChatSectionSettings::default());
        assert!(parsed.enabled(ChatSection::Muted));
        assert!(!parsed.enabled(ChatSection::Meeting));
    }

    #[test]
    fn reads_both_flags_from_the_json_string() {
        let parsed = parse_chat_sections(&properties(
            r#"{"id":"simpleCollabUserSettingsId","hasUserEnabledMutedChatSection":false,"hasUserEnabledMeetingChatSection":true}"#,
        ));
        assert!(!parsed.enabled(ChatSection::Muted));
        assert!(parsed.enabled(ChatSection::Meeting));
    }

    #[test]
    fn merge_keeps_every_unknown_field_and_the_other_flag() {
        let existing = properties(
            r#"{"id":"simpleCollabUserSettingsId","hasUserEnabledMeetingChatSection":true,"futureSetting":{"nested":[1,2]},"count":3}"#,
        );
        let merged = merge_chat_section(&existing, ChatSection::Muted, false);
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "simpleCollabUserSettingsId",
                "hasUserEnabledMeetingChatSection": true,
                "hasUserEnabledMutedChatSection": false,
                "futureSetting": {"nested": [1, 2]},
                "count": 3,
            })
        );
        assert_eq!(
            parse_chat_sections(&json!({SETTINGS_KEY: merged})),
            ChatSectionSettings {
                muted: Some(false),
                meeting: Some(true)
            }
        );
    }

    #[test]
    fn merge_without_stored_settings_creates_the_object_with_its_id() {
        let merged = merge_chat_section(&json!({}), ChatSection::Meeting, true);
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(
            value,
            json!({"id": SETTINGS_ID, "hasUserEnabledMeetingChatSection": true})
        );
    }

    #[tokio::test]
    async fn setting_a_section_reads_then_puts_the_merged_object_as_text() {
        let transport = Canned {
            answers: Mutex::new(
                vec![(200, properties(r#"{"keep":1}"#)), (200, Value::Null)].into(),
            ),
            requests: Mutex::new(Vec::new()),
        };
        UserSettings::with_transport(&transport, "emea")
            .set_chat_section(ChatSection::Muted, false)
            .await
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        let base = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/properties";
        assert_eq!(requests[0].method, Method::Get);
        assert_eq!(requests[0].url, base);
        assert_eq!(requests[1].method, Method::Put);
        assert_eq!(
            requests[1].url,
            format!("{base}?name=simpleCollabUserSettings")
        );
        let sent = requests[1].body.as_ref().unwrap()["simpleCollabUserSettings"]
            .as_str()
            .unwrap();
        let value: Value = serde_json::from_str(sent).unwrap();
        assert_eq!(value["keep"], json!(1));
        assert_eq!(value["hasUserEnabledMutedChatSection"], json!(false));
    }
}
