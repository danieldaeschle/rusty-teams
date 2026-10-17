use serde_json::{Map, Value};
use session::{Method, Request, Session};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, ensure_success};
use crate::pins::DEFAULT_REGION;

const SETTINGS_KEY: &str = "simpleCollabUserSettings";
const SETTINGS_ID: &str = "simpleCollabUserSettingsId";
const TEAMS_ORDER_KEY: &str = "teamsOrder";
const SKIN_TONE_KEY: &str = "preferredSkinToneIndex";
const APP_HORIZON_KEY: &str = "appConsumptionHorizon";
const APP_HORIZON_APPS: [&str; 3] = ["chat", "channel", "engage_community"];
const MAX_SKIN_TONE: u8 = 5;
const MUTED_FIELD: &str = "hasUserEnabledMutedChatSection";
const MEETING_FIELD: &str = "hasUserEnabledMeetingChatSection";
const ADAPTIVE_RAIL_FIELD: &str = "hasUserEnabledAdaptiveRailForDesktop";
const MUTED_DEFAULT: bool = false;
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
    pub adaptive_rail: Option<bool>,
}

impl ChatSectionSettings {
    pub fn enabled(&self, section: ChatSection) -> bool {
        match section {
            ChatSection::Muted => self.muted.unwrap_or(MUTED_DEFAULT),
            ChatSection::Meeting => self.meeting.unwrap_or(MEETING_DEFAULT),
        }
    }

    pub fn adaptive_rail(&self) -> bool {
        self.adaptive_rail.unwrap_or(false)
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
        self.set_property(SETTINGS_KEY, Value::String(merged)).await
    }

    pub async fn set_skin_tone(&self, tone: u8) -> Result<()> {
        self.set_property(SKIN_TONE_KEY, Value::String(tone.to_string()))
            .await
    }

    /// Teams' "Mark all as read" for channels: one write, the channel entry of `appConsumptionHorizon` moves to now.
    pub async fn mark_all_channels_read(&self, now_ms: i64) -> Result<AppConsumptionHorizon> {
        let properties = self.properties().await?;
        let horizon = AppConsumptionHorizon {
            channel_ms: now_ms,
            ..parse_app_consumption_horizon(&properties)
        };
        self.set_property(APP_HORIZON_KEY, Value::String(horizon.property_value()))
            .await?;
        Ok(horizon)
    }

    pub async fn set_property(&self, name: &str, value: Value) -> Result<()> {
        let url = format!("{}?name={name}", self.base_url);
        let mut body = Map::new();
        body.insert(name.to_owned(), value);
        let answer = self
            .transport
            .send(Request::with_body(Method::Put, url, Value::Object(body)))
            .await?;
        ensure_success(&answer)
    }

    pub async fn properties(&self) -> Result<Value> {
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
        adaptive_rail: settings.get(ADAPTIVE_RAIL_FIELD).and_then(Value::as_bool),
    }
}

pub fn parse_skin_tone(properties: &Value) -> Option<u8> {
    let value = properties.get(SKIN_TONE_KEY)?;
    let tone = match value {
        Value::String(text) => text.parse().ok()?,
        other => u8::try_from(other.as_u64()?).ok()?,
    };
    (tone <= MAX_SKIN_TONE).then_some(tone)
}

pub fn parse_teams_order(properties: &Value) -> Vec<(String, i64)> {
    let entries = match properties.get(TEAMS_ORDER_KEY) {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok(),
        Some(other) => Some(other.clone()),
        None => None,
    };
    let mut order: Vec<(String, i64)> = entries
        .as_ref()
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let team_id = entry.get("teamId")?.as_str()?.to_owned();
                    let position = entry.get("order")?;
                    let position = position
                        .as_i64()
                        .or_else(|| position.as_f64().map(|float| float as i64))
                        .or_else(|| position.as_str()?.parse().ok())?;
                    Some((team_id, position))
                })
                .collect()
        })
        .unwrap_or_default();
    order.sort_by_key(|(_, position)| *position);
    order
}

/// Per app "read everything before" times in ms; Teams treats a conversation of that app as read up to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppConsumptionHorizon {
    pub chat_ms: i64,
    pub channel_ms: i64,
    pub engage_community_ms: i64,
}

impl AppConsumptionHorizon {
    pub fn property_value(&self) -> String {
        let values = [self.chat_ms, self.channel_ms, self.engage_community_ms];
        let mut fields = Map::new();
        for (app, value) in APP_HORIZON_APPS.iter().zip(values) {
            fields.insert((*app).to_owned(), Value::from(value));
        }
        Value::Object(fields).to_string()
    }
}

pub fn parse_app_consumption_horizon(properties: &Value) -> AppConsumptionHorizon {
    let parsed = match properties.get(APP_HORIZON_KEY) {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok(),
        Some(other) => Some(other.clone()),
        None => None,
    };
    let Some(fields) = parsed.as_ref().and_then(Value::as_object) else {
        return AppConsumptionHorizon::default();
    };
    let valid = (fields.contains_key("chat") || fields.contains_key("channel"))
        && fields
            .values()
            .all(|value| value.as_f64().is_some_and(|number| number >= 0.0));
    if !valid {
        return AppConsumptionHorizon::default();
    }
    let read = |app: &str| {
        fields
            .get(app)
            .and_then(Value::as_f64)
            .map_or(0, |number| number as i64)
    };
    AppConsumptionHorizon {
        chat_ms: read("chat"),
        channel_ms: read("channel"),
        engage_community_ms: read("engage_community"),
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

    use serde_json::json;
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
    fn skin_tone_is_read_from_the_string_property() {
        assert_eq!(
            parse_skin_tone(&json!({"preferredSkinToneIndex": "2"})),
            Some(2)
        );
        assert_eq!(
            parse_skin_tone(&json!({"preferredSkinToneIndex": "9"})),
            None
        );
        assert_eq!(parse_skin_tone(&json!({})), None);
    }

    #[test]
    fn unset_fields_use_the_teams_defaults() {
        let parsed = parse_chat_sections(&json!({}));
        assert_eq!(parsed, ChatSectionSettings::default());
        assert!(!parsed.enabled(ChatSection::Muted));
        assert!(!parsed.enabled(ChatSection::Meeting));
        assert!(!parsed.adaptive_rail());
    }

    #[test]
    fn reads_every_flag_from_the_json_string() {
        let parsed = parse_chat_sections(&properties(
            r#"{"id":"simpleCollabUserSettingsId","hasUserEnabledMutedChatSection":false,"hasUserEnabledMeetingChatSection":true,"hasUserEnabledAdaptiveRailForDesktop":true}"#,
        ));
        assert!(!parsed.enabled(ChatSection::Muted));
        assert!(parsed.enabled(ChatSection::Meeting));
        assert!(parsed.adaptive_rail());
    }

    #[test]
    fn teams_order_is_read_from_the_json_string_sorted_by_order() {
        let parsed = parse_teams_order(&json!({
            TEAMS_ORDER_KEY: r#"[{"teamId":"19:b","order":1},{"teamId":"19:a","order":"0"},{"teamId":"19:c"},{"order":2},{"teamId":"19:d","order":2.0}]"#,
        }));
        assert_eq!(
            parsed,
            vec![
                ("19:a".to_owned(), 0),
                ("19:b".to_owned(), 1),
                ("19:d".to_owned(), 2)
            ]
        );
    }

    #[test]
    fn teams_order_is_empty_when_missing_or_malformed() {
        assert!(parse_teams_order(&json!({})).is_empty());
        assert!(parse_teams_order(&json!({TEAMS_ORDER_KEY: "not json"})).is_empty());
        assert!(parse_teams_order(&json!({TEAMS_ORDER_KEY: "{}"})).is_empty());
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
                meeting: Some(true),
                adaptive_rail: None,
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

    #[test]
    fn app_horizon_reads_the_string_and_ignores_invalid_values() {
        let parsed = parse_app_consumption_horizon(&json!({
            "appConsumptionHorizon": "{\"chat\":1791368000000,\"channel\":1791369000000,\"engage_community\":0}"
        }));
        assert_eq!(
            parsed,
            AppConsumptionHorizon {
                chat_ms: 1791368000000,
                channel_ms: 1791369000000,
                engage_community_ms: 0,
            }
        );
        let object = parse_app_consumption_horizon(&json!({"appConsumptionHorizon": {"channel": 5}}));
        assert_eq!(object.channel_ms, 5);
        for broken in [
            json!({}),
            json!({"appConsumptionHorizon": "not json"}),
            json!({"appConsumptionHorizon": "{\"engage_community\":5}"}),
            json!({"appConsumptionHorizon": "{\"channel\":-1}"}),
            json!({"appConsumptionHorizon": "{\"channel\":\"5\"}"}),
        ] {
            assert_eq!(
                parse_app_consumption_horizon(&broken),
                AppConsumptionHorizon::default()
            );
        }
    }

    #[tokio::test]
    async fn mark_all_channels_read_moves_only_the_channel_entry() {
        let stored = json!({
            "appConsumptionHorizon": "{\"chat\":100,\"channel\":200,\"engage_community\":300}"
        });
        let transport = Canned {
            answers: Mutex::new(vec![(200, stored), (200, Value::Null)].into()),
            requests: Mutex::new(Vec::new()),
        };
        let horizon = UserSettings::with_transport(&transport, "emea")
            .mark_all_channels_read(1791368000000)
            .await
            .unwrap();
        assert_eq!(horizon.channel_ms, 1791368000000);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[1].method, Method::Put);
        assert_eq!(
            requests[1].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/properties?name=appConsumptionHorizon"
        );
        let sent = requests[1].body.as_ref().unwrap()["appConsumptionHorizon"]
            .as_str()
            .unwrap();
        let value: Value = serde_json::from_str(sent).unwrap();
        assert_eq!(
            value,
            json!({"chat": 100, "channel": 1791368000000_i64, "engage_community": 300})
        );
    }
}
