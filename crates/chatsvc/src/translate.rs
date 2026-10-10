use serde_json::{Value, json};
use session::{Method, Request, Session};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, Messages, encode, ensure_success};
use crate::pins::{CsaTransport, DEFAULT_REGION, SessionTransport};

const LIST_VIEW: &str = "msnp24Equivalent";
const USE_ACS_HEADER: &str = "x-ms-use-acs-translation";
const ON_DEMAND_HEADER: &str = "x-ms-on-demand-translation";
const KNOWN_LANGUAGES_HEADER: &str = "x-ms-knownLanguages";
const REQUEST_ID_HEADER: &str = "x-ms-request-id";
pub const MAX_TRANSLATE_BATCH: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslationTrigger {
    OnDemand,
    Automatic { known_languages: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateRequest {
    pub message_id: String,
    pub version: Option<String>,
}

impl TranslateRequest {
    pub fn new(message_id: &str) -> Self {
        TranslateRequest {
            message_id: message_id.to_owned(),
            version: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationStatus {
    Done,
    MessageNotFound,
    VersionNotFound,
    LanguageDetectionFailed,
    NotNeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub message_id: String,
    pub version: Option<String>,
    pub status: TranslationStatus,
    pub content_html: Option<String>,
    pub subject: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLanguage {
    pub message_id: String,
    pub stamp: String,
}

/// Cuts a locale to the code the translator knows: `en-GB` is `en`, `zh-TW` is `zh-cht`, `sr-Latn-RS` is `sr-latn`.
pub fn language_code(locale: &str) -> String {
    let lowered = locale.trim().replace('_', "-").to_lowercase();
    let mut parts = lowered.split('-');
    let language = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    if language == "zh" {
        let traditional = rest
            .iter()
            .any(|part| matches!(*part, "hant" | "hk" | "tw"));
        return if traditional { "zh-cht" } else { "zh-chs" }.to_owned();
    }
    match rest.first() {
        Some(script) if script.len() == 4 => format!("{language}-{script}"),
        _ => language.to_owned(),
    }
}

pub fn translate_headers(trigger: &TranslationTrigger) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Content-Type".to_owned(), "application/json".to_owned()),
        (USE_ACS_HEADER.to_owned(), "true".to_owned()),
        (REQUEST_ID_HEADER.to_owned(), Uuid::new_v4().to_string()),
    ];
    match trigger {
        TranslationTrigger::OnDemand => {
            headers.push((ON_DEMAND_HEADER.to_owned(), "true".to_owned()))
        }
        TranslationTrigger::Automatic { known_languages } => {
            headers.push((KNOWN_LANGUAGES_HEADER.to_owned(), known_languages.join(",")))
        }
    }
    headers
}

pub fn translate_body(messages: &[TranslateRequest]) -> Value {
    Value::Array(
        messages
            .iter()
            .map(|message| match &message.version {
                Some(version) => json!({"id": message.message_id, "version": version}),
                None => json!({"id": message.message_id}),
            })
            .collect(),
    )
}

fn status_for(result_code: &str) -> TranslationStatus {
    match result_code {
        "Success" => TranslationStatus::Done,
        "MessageNotFound" => TranslationStatus::MessageNotFound,
        "VersionNotFound" => TranslationStatus::VersionNotFound,
        "LanguageDetectionFailed" => TranslationStatus::LanguageDetectionFailed,
        "SkipTranslation" | "ContentIsEmpty" => TranslationStatus::NotNeeded,
        _ => TranslationStatus::Failed,
    }
}

pub fn parse_translations(body: &Value) -> Result<Vec<Translation>> {
    let entries = body
        .as_array()
        .ok_or_else(|| Error::UnexpectedAnswer("no translation list".into()))?;
    let text = |entry: &Value, key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let message_id = entry.get("id")?.as_str()?.to_owned();
            let status = status_for(
                entry
                    .get("resultCode")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            let version = match entry.get("version") {
                Some(Value::String(version)) => Some(version.clone()),
                Some(Value::Number(version)) => Some(version.to_string()),
                _ => None,
            };
            Some(Translation {
                message_id,
                version,
                status,
                content_html: text(entry, "translatedContent"),
                subject: text(entry, "translatedSubject"),
                title: text(entry, "translatedTitle"),
            })
        })
        .collect())
}

pub fn parse_languages(body: &Value) -> Result<Vec<Language>> {
    let entries = body
        .as_array()
        .ok_or_else(|| Error::UnexpectedAnswer("no language list".into()))?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(Language {
                code: entry.get("languageTag")?.as_str()?.to_owned(),
                name: entry.get("languageName")?.as_str()?.to_owned(),
            })
        })
        .collect())
}

pub fn parse_message_languages(body: &Value) -> Vec<MessageLanguage> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| {
            Some(MessageLanguage {
                message_id: message.get("id")?.as_str()?.to_owned(),
                stamp: message
                    .pointer("/properties/languageStamp")?
                    .as_str()?
                    .to_owned(),
            })
        })
        .collect()
}

pub struct Translator<T: CsaTransport = SessionTransport> {
    transport: T,
    api_url: String,
}

impl Translator<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        Translator::with_transport(SessionTransport::new(session), region)
    }
}

impl<T: CsaTransport> Translator<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        Translator {
            transport,
            api_url: format!("https://teams.cloud.microsoft/api/csa/{region}/api/v1"),
        }
    }

    pub fn chat_url(&self, chat_id: &str, to_language: &str) -> String {
        format!(
            "{}/chats/{}/messages/languages/{}",
            self.api_url,
            encode(chat_id),
            encode(to_language)
        )
    }

    pub fn channel_url(
        &self,
        team_id: &str,
        channel_id: &str,
        root_id: &str,
        to_language: &str,
    ) -> String {
        format!(
            "{}/teams/{}/channels/{}/{}/messages/languages/{}",
            self.api_url,
            encode(team_id),
            encode(channel_id),
            encode(root_id),
            encode(to_language)
        )
    }

    pub async fn translate_chat(
        &self,
        chat_id: &str,
        to_language: &str,
        messages: &[TranslateRequest],
        trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        let url = self.chat_url(chat_id, to_language);
        self.translate(url, messages, trigger).await
    }

    pub async fn translate_channel(
        &self,
        team_id: &str,
        channel_id: &str,
        root_id: &str,
        to_language: &str,
        messages: &[TranslateRequest],
        trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        let url = self.channel_url(team_id, channel_id, root_id, to_language);
        self.translate(url, messages, trigger).await
    }

    pub async fn languages(&self, ui_locale: &str) -> Result<Vec<Language>> {
        let mut request = Request::get(format!(
            "{}/translator/languages?locale={}",
            self.api_url,
            encode(ui_locale)
        ));
        request.headers = vec![(USE_ACS_HEADER.to_owned(), "true".to_owned())];
        let answer = self.transport.send(request).await?;
        ensure_success(&answer)?;
        parse_languages(&answer.body)
    }

    async fn translate(
        &self,
        url: String,
        messages: &[TranslateRequest],
        trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        let mut request = Request::with_body(Method::Post, url, translate_body(messages));
        request.headers = translate_headers(trigger);
        let answer = self.transport.send(request).await?;
        ensure_success(&answer)?;
        parse_translations(&answer.body)
    }
}

impl<T: MessageTransport> Messages<T> {
    pub async fn list_language_stamps(
        &self,
        conversation: &crate::messages::ConversationRef,
        page_size: usize,
    ) -> Result<Vec<MessageLanguage>> {
        let url = format!(
            "{}/{}/messages?view={LIST_VIEW}&pageSize={page_size}",
            self.base_url,
            encode(&conversation.conversation_id())
        );
        let answer = self.transport.send(Request::get(&url)).await?;
        ensure_success(&answer)?;
        Ok(parse_message_languages(&answer.body))
    }
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

    impl Canned {
        fn new(answers: Vec<(u16, Value)>) -> Self {
            Canned {
                answers: Mutex::new(answers.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
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

    fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
        request
            .headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn locales_map_to_translator_codes() {
        assert_eq!(language_code("en-GB"), "en");
        assert_eq!(language_code("de_DE"), "de");
        assert_eq!(language_code("zh-TW"), "zh-cht");
        assert_eq!(language_code("zh-Hant-HK"), "zh-cht");
        assert_eq!(language_code("zh-CN"), "zh-chs");
        assert_eq!(language_code("zh"), "zh-chs");
        assert_eq!(language_code("sr-Latn-RS"), "sr-latn");
        assert_eq!(language_code("FR"), "fr");
    }

    #[test]
    fn on_demand_requests_force_and_send_no_known_languages() {
        let headers = translate_headers(&TranslationTrigger::OnDemand);
        let find = |name: &str| headers.iter().find(|(key, _)| key == name);
        assert_eq!(find(USE_ACS_HEADER).unwrap().1, "true");
        assert_eq!(find(ON_DEMAND_HEADER).unwrap().1, "true");
        assert!(find(KNOWN_LANGUAGES_HEADER).is_none());
        assert_eq!(find("Content-Type").unwrap().1, "application/json");
        assert!(Uuid::parse_str(&find(REQUEST_ID_HEADER).unwrap().1).is_ok());
    }

    #[test]
    fn automatic_requests_send_the_known_languages_instead() {
        let headers = translate_headers(&TranslationTrigger::Automatic {
            known_languages: vec!["en".into(), "de".into()],
        });
        let find = |name: &str| headers.iter().find(|(key, _)| key == name);
        assert_eq!(find(KNOWN_LANGUAGES_HEADER).unwrap().1, "en,de");
        assert!(find(ON_DEMAND_HEADER).is_none());
        assert_eq!(find(USE_ACS_HEADER).unwrap().1, "true");
    }

    #[test]
    fn body_lists_ids_and_optional_versions() {
        let body = translate_body(&[
            TranslateRequest::new("1"),
            TranslateRequest {
                message_id: "2".into(),
                version: Some("7".into()),
            },
        ]);
        assert_eq!(body, json!([{"id": "1"}, {"id": "2", "version": "7"}]));
    }

    #[test]
    fn parses_success_and_not_found_results() {
        let parsed = parse_translations(&json!([
            {"id": "1", "version": 1700, "languageId": "en", "detectedLanguage": {"languageId": null, "score": 1},
             "translatedContent": "<p>Hello</p>", "translatedSubject": null, "translatedTitle": null, "resultCode": "Success"},
            {"id": "2", "resultCode": "MessageNotFound", "translatedContent": null},
            {"id": "3", "resultCode": "ContentIsEmpty"},
            {"id": "4", "resultCode": "Strange"},
            {"resultCode": "Success"},
        ]))
        .unwrap();
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].status, TranslationStatus::Done);
        assert_eq!(parsed[0].version.as_deref(), Some("1700"));
        assert_eq!(parsed[0].content_html.as_deref(), Some("<p>Hello</p>"));
        assert_eq!(parsed[1].status, TranslationStatus::MessageNotFound);
        assert_eq!(parsed[1].content_html, None);
        assert_eq!(parsed[2].status, TranslationStatus::NotNeeded);
        assert_eq!(parsed[3].status, TranslationStatus::Failed);
    }

    #[test]
    fn a_non_list_answer_is_unexpected() {
        assert!(parse_translations(&json!("Bad request.")).is_err());
    }

    #[test]
    fn parses_language_list_and_stamps() {
        let languages = parse_languages(&json!([
            {"languageTag": "de", "languageName": "German"},
            {"languageTag": "zh-chs", "languageName": "Chinese (Simplified)"},
            {"languageTag": "x"},
        ]))
        .unwrap();
        assert_eq!(
            languages,
            vec![
                Language {
                    code: "de".into(),
                    name: "German".into()
                },
                Language {
                    code: "zh-chs".into(),
                    name: "Chinese (Simplified)".into()
                },
            ]
        );
        let stamps = parse_message_languages(&json!({"messages": [
            {"id": "1", "properties": {"languageStamp": "languages=de:100;length:82;&detector=Bling"}},
            {"id": "2", "properties": {}},
            {"id": "3"},
        ]}));
        assert_eq!(
            stamps,
            vec![MessageLanguage {
                message_id: "1".into(),
                stamp: "languages=de:100;length:82;&detector=Bling".into()
            }]
        );
    }

    #[tokio::test]
    async fn chat_translation_posts_ids_to_the_language_route_with_the_headers() {
        let transport = Canned::new(vec![(
            200,
            json!([{"id": "m1", "resultCode": "Success", "translatedContent": "<p>Hi</p>"}]),
        )]);
        let translator = Translator::with_transport(&transport, "emea");
        let translations = translator
            .translate_chat(
                "19:a@thread.v2",
                "en",
                &[TranslateRequest::new("m1")],
                &TranslationTrigger::OnDemand,
            )
            .await
            .unwrap();
        assert_eq!(translations[0].content_html.as_deref(), Some("<p>Hi</p>"));
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Post);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/csa/emea/api/v1/chats/19%3Aa%40thread.v2/messages/languages/en"
        );
        assert_eq!(requests[0].body, Some(json!([{"id": "m1"}])));
        assert_eq!(header(&requests[0], USE_ACS_HEADER), Some("true"));
        assert_eq!(header(&requests[0], ON_DEMAND_HEADER), Some("true"));
    }

    #[tokio::test]
    async fn channel_translation_uses_the_root_message_route() {
        let transport = Canned::new(vec![(200, json!([]))]);
        Translator::with_transport(&transport, "emea")
            .translate_channel(
                "team1",
                "19:c@thread.tacv2",
                "root1",
                "fr",
                &[TranslateRequest::new("m1")],
                &TranslationTrigger::OnDemand,
            )
            .await
            .unwrap();
        assert_eq!(
            transport.requests.lock().unwrap()[0].url,
            "https://teams.cloud.microsoft/api/csa/emea/api/v1/teams/team1/channels/19%3Ac%40thread.tacv2/root1/messages/languages/fr"
        );
    }

    #[tokio::test]
    async fn a_rejected_request_is_an_error() {
        let transport = Canned::new(vec![(400, json!("Bad request."))]);
        let outcome = Translator::with_transport(&transport, "emea")
            .translate_chat(
                "c",
                "xx",
                &[TranslateRequest::new("m1")],
                &TranslationTrigger::OnDemand,
            )
            .await;
        assert!(outcome.is_err());
    }

    #[tokio::test]
    async fn language_list_asks_with_the_ui_locale_and_the_acs_header() {
        let transport = Canned::new(vec![(
            200,
            json!([{"languageTag": "de", "languageName": "German"}]),
        )]);
        let languages = Translator::with_transport(&transport, "emea")
            .languages("en-gb")
            .await
            .unwrap();
        assert_eq!(languages.len(), 1);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Get);
        assert!(
            requests[0]
                .url
                .ends_with("/translator/languages?locale=en-gb")
        );
        assert_eq!(header(&requests[0], USE_ACS_HEADER), Some("true"));
    }
}
