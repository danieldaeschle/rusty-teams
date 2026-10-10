use serde_json::{Map, Value, json};
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::{Error, Result};
use crate::messages::ensure_success;
use crate::pins::{CsaTransport, DEFAULT_REGION, SessionTransport};
use crate::translate::language_code;

const SKYPE_SCOPE: &str = "user_impersonation";
const CLIENT_VERSION: &str = "1415/26091712213";
const CLIENT_TYPE: &str = "TeamsV2Web";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranslationBehavior {
    #[default]
    Ask,
    Auto,
    Never,
}

impl TranslationBehavior {
    pub fn code(self) -> &'static str {
        match self {
            TranslationBehavior::Ask => "Ask",
            TranslationBehavior::Auto => "Yes",
            TranslationBehavior::Never => "No",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "Yes" => TranslationBehavior::Auto,
            "No" => TranslationBehavior::Never,
            _ => TranslationBehavior::Ask,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LanguageSettings {
    pub display_locale: Option<String>,
    pub target_locale: Option<String>,
    pub behavior: TranslationBehavior,
    pub authoring_locales: Vec<String>,
    pub preferences: Map<String, Value>,
}

impl LanguageSettings {
    pub fn never_translate_codes(&self) -> Vec<String> {
        let mut codes: Vec<String> = Vec::new();
        for locale in &self.authoring_locales {
            let code = language_code(locale);
            if !code.is_empty() && !codes.contains(&code) {
                codes.push(code);
            }
        }
        codes
    }

    pub fn with_never_translate(&self, code: &str) -> Vec<String> {
        let mut locales = self.authoring_locales.clone();
        if !self
            .never_translate_codes()
            .iter()
            .any(|known| known == code)
        {
            locales.push(code.to_owned());
        }
        locales
    }

    pub fn without_never_translate(&self, code: &str) -> Vec<String> {
        self.authoring_locales
            .iter()
            .filter(|locale| language_code(locale) != code)
            .cloned()
            .collect()
    }
}

impl LanguageSettings {
    /// The shape of the account answer, so a stored copy parses back with `parse_language_settings`.
    pub fn to_account_value(&self) -> Value {
        let locale =
            |locale: &Option<String>| locale.as_ref().map(|locale| json!({"locale": locale}));
        json!({"userLanguageAccountSettings": {"value": {
            "defaultDisplayLanguage": locale(&self.display_locale),
            "defaultTranslationLanguage": locale(&self.target_locale),
            "authoringLanguages": authoring_patch(&self.authoring_locales)["authoringLanguages"],
            "translationPreferences": self.preferences,
        }}})
    }
}

pub fn parse_language_settings(body: &Value) -> Result<LanguageSettings> {
    let value = body
        .pointer("/userLanguageAccountSettings/value")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::UnexpectedAnswer("no language settings".into()))?;
    let locale = |pointer: &str| {
        value
            .get(pointer)
            .and_then(|entry| entry.get("locale"))
            .and_then(Value::as_str)
            .filter(|locale| !locale.is_empty())
            .map(str::to_owned)
    };
    let preferences = value
        .get("translationPreferences")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(LanguageSettings {
        display_locale: locale("defaultDisplayLanguage"),
        target_locale: locale("defaultTranslationLanguage"),
        behavior: preferences
            .get("translationBehavior")
            .and_then(Value::as_str)
            .map_or_else(TranslationBehavior::default, TranslationBehavior::from_code),
        authoring_locales: value
            .get("authoringLanguages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("locale")?.as_str().map(str::to_owned))
            .collect(),
        preferences,
    })
}

pub fn read_body() -> Value {
    json!({"userLanguageAccountSettings": true})
}

pub fn behavior_patch(settings: &LanguageSettings, behavior: TranslationBehavior) -> Value {
    let mut preferences = settings.preferences.clone();
    preferences.insert("translationBehavior".into(), json!(behavior.code()));
    json!({"translationPreferences": preferences})
}

pub fn target_patch(locale: &str) -> Value {
    json!({"defaultTranslationLanguage": {"locale": locale}})
}

pub fn authoring_patch(locales: &[String]) -> Value {
    json!({
        "authoringLanguages": locales
            .iter()
            .map(|locale| json!({"locale": locale}))
            .collect::<Vec<Value>>()
    })
}

pub struct LanguageSettingsClient<T: CsaTransport = SessionTransport> {
    transport: T,
    base_url: String,
}

impl LanguageSettingsClient<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        let transport = SessionTransport::with_scope(session, Scope::new(SPACES, SKYPE_SCOPE));
        LanguageSettingsClient::with_transport(transport, region)
    }
}

impl<T: CsaTransport> LanguageSettingsClient<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        LanguageSettingsClient {
            transport,
            base_url: format!("https://teams.cloud.microsoft/api/mt/{region}/beta/users"),
        }
    }

    pub async fn read(&self) -> Result<LanguageSettings> {
        let url = format!("{}/useraggregatesettings", self.base_url);
        let answer = self
            .transport
            .send(with_headers(Request::with_body(
                Method::Post,
                url,
                read_body(),
            )))
            .await?;
        ensure_success(&answer)?;
        parse_language_settings(&answer.body)
    }

    pub async fn patch(&self, patch: Value) -> Result<()> {
        let url = format!("{}/languageAccountSettings", self.base_url);
        let answer = self
            .transport
            .send(with_headers(Request::with_body(Method::Patch, url, patch)))
            .await?;
        ensure_success(&answer)
    }
}

fn with_headers(mut request: Request) -> Request {
    request.headers = vec![
        ("x-ms-client-type".to_owned(), CLIENT_TYPE.to_owned()),
        ("x-ms-client-version".to_owned(), CLIENT_VERSION.to_owned()),
        ("Accept".to_owned(), "application/json".to_owned()),
    ];
    request
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

    impl CsaTransport for &Canned {
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

    fn account() -> Value {
        json!({"userLanguageAccountSettings": {"value": {
            "defaultDisplayLanguage": {"locale": "en-US"},
            "defaultTranslationLanguage": {"locale": "en"},
            "authoringLanguages": [{"locale": "de-DE"}, {"locale": "en-DE"}, {"locale": "en-US"}],
            "translationPreferences": {
                "translationBehavior": "Ask",
                "untranslatedLanguages": ["de", "en"],
                "languageOverrides": []
            }
        }}})
    }

    #[test]
    fn parses_the_account_settings() {
        let settings = parse_language_settings(&account()).unwrap();
        assert_eq!(settings.display_locale.as_deref(), Some("en-US"));
        assert_eq!(settings.target_locale.as_deref(), Some("en"));
        assert_eq!(settings.behavior, TranslationBehavior::Ask);
        assert_eq!(settings.never_translate_codes(), vec!["de", "en"]);
        assert_eq!(
            settings.preferences["untranslatedLanguages"],
            json!(["de", "en"])
        );
    }

    #[test]
    fn a_stored_copy_parses_back_to_the_same_settings() {
        let settings = parse_language_settings(&account()).unwrap();
        assert_eq!(
            parse_language_settings(&settings.to_account_value()).unwrap(),
            settings
        );
        let empty = LanguageSettings::default();
        assert_eq!(
            parse_language_settings(&empty.to_account_value()).unwrap(),
            empty
        );
    }

    #[test]
    fn missing_fields_fall_back_to_ask_and_empty_lists() {
        let settings =
            parse_language_settings(&json!({"userLanguageAccountSettings": {"value": {}}}))
                .unwrap();
        assert_eq!(settings, LanguageSettings::default());
        assert!(parse_language_settings(&json!({})).is_err());
    }

    #[test]
    fn behavior_codes_follow_the_account_values() {
        assert_eq!(
            TranslationBehavior::from_code("Yes"),
            TranslationBehavior::Auto
        );
        assert_eq!(
            TranslationBehavior::from_code("No"),
            TranslationBehavior::Never
        );
        assert_eq!(
            TranslationBehavior::from_code("Ask"),
            TranslationBehavior::Ask
        );
        assert_eq!(
            TranslationBehavior::from_code("?"),
            TranslationBehavior::Ask
        );
        assert_eq!(TranslationBehavior::Auto.code(), "Yes");
    }

    #[test]
    fn behavior_patch_keeps_the_other_preferences() {
        let settings = parse_language_settings(&account()).unwrap();
        assert_eq!(
            behavior_patch(&settings, TranslationBehavior::Auto),
            json!({"translationPreferences": {
                "translationBehavior": "Yes",
                "untranslatedLanguages": ["de", "en"],
                "languageOverrides": []
            }})
        );
    }

    #[test]
    fn target_and_authoring_patches_carry_only_their_part() {
        assert_eq!(
            target_patch("fr"),
            json!({"defaultTranslationLanguage": {"locale": "fr"}})
        );
        assert_eq!(
            authoring_patch(&["de-DE".into(), "fr".into()]),
            json!({"authoringLanguages": [{"locale": "de-DE"}, {"locale": "fr"}]})
        );
    }

    #[test]
    fn adding_and_removing_a_never_language_works_on_codes() {
        let settings = parse_language_settings(&account()).unwrap();
        assert_eq!(
            settings.with_never_translate("fr"),
            vec!["de-DE", "en-DE", "en-US", "fr"]
        );
        assert_eq!(
            settings.with_never_translate("de"),
            vec!["de-DE", "en-DE", "en-US"]
        );
        assert_eq!(settings.without_never_translate("en"), vec!["de-DE"]);
    }

    #[tokio::test]
    async fn read_and_patch_use_the_mt_routes_with_the_client_headers() {
        let transport = Canned {
            answers: Mutex::new(vec![(200, account()), (200, Value::Null)].into()),
            requests: Mutex::new(Vec::new()),
        };
        let client = LanguageSettingsClient::with_transport(&transport, "emea");
        let settings = client.read().await.unwrap();
        client
            .patch(behavior_patch(&settings, TranslationBehavior::Never))
            .await
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        let base = "https://teams.cloud.microsoft/api/mt/emea/beta/users";
        assert_eq!(requests[0].method, Method::Post);
        assert_eq!(requests[0].url, format!("{base}/useraggregatesettings"));
        assert_eq!(
            requests[0].body,
            Some(json!({"userLanguageAccountSettings": true}))
        );
        assert_eq!(requests[1].method, Method::Patch);
        assert_eq!(requests[1].url, format!("{base}/languageAccountSettings"));
        let names: Vec<&str> = requests[1]
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["x-ms-client-type", "x-ms-client-version", "Accept"]);
        assert_eq!(
            requests[1].body.as_ref().unwrap()["translationPreferences"]["translationBehavior"],
            json!("No")
        );
    }

    #[tokio::test]
    async fn an_empty_answer_is_not_settings() {
        let transport = Canned {
            answers: Mutex::new(vec![(204, Value::Null)].into()),
            requests: Mutex::new(Vec::new()),
        };
        let outcome = LanguageSettingsClient::with_transport(&transport, "emea")
            .read()
            .await;
        assert!(outcome.is_err());
    }
}
