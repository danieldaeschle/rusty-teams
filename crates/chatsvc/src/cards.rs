use serde_json::{Value, json};
use session::{ApiResponse, Method, Request, SPACES, Scope, Session};

use crate::error::{Error, Result};
use crate::messages::{encode, messages_scope};
use crate::pins::DEFAULT_REGION;

pub const DEFAULT_MT_REGION: &str = "emea";
pub const FALLBACK_INVOKE_REGION: &str = "de";
const SPACES_SCOPE: &str = "user_impersonation";
const TOKEN_PLACEHOLDER: &str = "@@rusty-teams-user-aad-token@@";
const BOT_MRI_PREFIX: &str = "28:";
const CARD_MESSAGE_TYPE: &str = "RichText/Media_Card";
const ADAPTIVE_CARD_TYPE: &str = "application/vnd.microsoft.card.adaptive";
const MESSAGE_TYPE: &str = "application/vnd.microsoft.activity.message";
const SEARCH_RESPONSE_TYPE: &str = "application/vnd.microsoft.search.searchResponse";
const ERROR_STATUS_FLOOR: u64 = 400;

pub fn spaces_scope() -> Scope {
    Scope::new(SPACES, SPACES_SCOPE)
}

pub fn bot_mri(bot_id: &str) -> String {
    if bot_id.starts_with(BOT_MRI_PREFIX) {
        bot_id.to_owned()
    } else {
        format!("{BOT_MRI_PREFIX}{bot_id}")
    }
}

pub fn bot_guid(bot_id: &str) -> &str {
    bot_id.strip_prefix(BOT_MRI_PREFIX).unwrap_or(bot_id)
}

pub trait CardTransport {
    fn send(
        &self,
        request: Request,
        scope: Scope,
    ) -> impl std::future::Future<Output = Result<ApiResponse>>;
}

pub struct SessionCardTransport {
    session: Session,
}

impl SessionCardTransport {
    pub fn new(session: &Session) -> Self {
        SessionCardTransport {
            session: session.clone(),
        }
    }
}

impl CardTransport for SessionCardTransport {
    async fn send(&self, request: Request, scope: Scope) -> Result<ApiResponse> {
        let mut answers = self.session.batch(&[request], &scope).await?;
        Ok(answers.remove(0))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvokeRequest {
    pub bot_id: String,
    pub app_id: String,
    pub name: String,
    pub value: Value,
    pub display_name: String,
    pub server_message_id: String,
    pub client_message_id: Option<String>,
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InvokeResponse {
    Empty,
    Card(Value),
    Message(String),
    Search(Value),
    Task(TaskResponse),
    Failed { status_code: u16, message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskResponse {
    Continue(TaskContinue),
    Message(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskContinue {
    pub title: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub content: TaskContent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskContent {
    Url {
        url: String,
        fallback_url: Option<String>,
    },
    Card(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatApp {
    pub app_id: String,
    pub name: String,
    pub bot_ids: Vec<String>,
    pub small_image_url: Option<String>,
    pub accent_color: Option<String>,
    pub web_application_resource: Option<String>,
}

impl ChatApp {
    pub fn has_bot(&self, bot_id: &str) -> bool {
        self.bot_ids
            .iter()
            .any(|known| bot_guid(known).eq_ignore_ascii_case(bot_guid(bot_id)))
    }
}

pub struct CardActions<T: CardTransport = SessionCardTransport> {
    transport: T,
    chatsvc_base: String,
    fallback_chatsvc_base: String,
    mt_base: String,
}

impl CardActions<SessionCardTransport> {
    pub fn new(session: &Session) -> Self {
        CardActions::with_transport(
            SessionCardTransport::new(session),
            DEFAULT_REGION,
            DEFAULT_MT_REGION,
        )
    }
}

impl<T: CardTransport> CardActions<T> {
    pub fn with_transport(transport: T, region: &str, mt_region: &str) -> Self {
        CardActions {
            transport,
            chatsvc_base: chatsvc_base(region),
            fallback_chatsvc_base: chatsvc_base(FALLBACK_INVOKE_REGION),
            mt_base: format!("https://teams.cloud.microsoft/api/mt/{mt_region}/beta/chats"),
        }
    }

    pub fn invoke_url(&self, bot_id: &str) -> String {
        invoke_url(&self.chatsvc_base, bot_id)
    }

    pub fn entitlements_url(&self, chat_id: &str) -> String {
        format!("{}/{}/apps/chatentitlements", self.mt_base, encode(chat_id))
    }

    pub async fn invoke(&self, invoke: InvokeRequest) -> Result<InvokeResponse> {
        let body = invoke_body(&invoke);
        let mut answer = self.post(&self.invoke_url(&invoke.bot_id), &body).await?;
        if answer.status == 404 && self.fallback_chatsvc_base != self.chatsvc_base {
            let url = invoke_url(&self.fallback_chatsvc_base, &invoke.bot_id);
            answer = self.post(&url, &body).await?;
        }
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                "chatsvc",
                answer.body,
            )));
        }
        Ok(parse_invoke_response(&answer.body))
    }

    pub async fn chat_apps(&self, chat_id: &str) -> Result<Vec<ChatApp>> {
        let answer = self
            .transport
            .send(Request::get(self.entitlements_url(chat_id)), spaces_scope())
            .await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                "chatentitlements",
                answer.body,
            )));
        }
        parse_chat_apps(&answer.body)
    }

    async fn post(&self, url: &str, body: &Value) -> Result<ApiResponse> {
        let mut request = Request::with_body(Method::Post, url, body.clone()).with_body_token(
            SPACES,
            SPACES_SCOPE,
            TOKEN_PLACEHOLDER,
        );
        request.headers = vec![
            ("x-ms-migration".to_owned(), "True".to_owned()),
            ("behavioroverride".to_owned(), "redirectAs404".to_owned()),
            ("content-type".to_owned(), "application/json".to_owned()),
        ];
        self.transport.send(request, messages_scope()).await
    }
}

fn chatsvc_base(region: &str) -> String {
    format!("https://teams.cloud.microsoft/api/chatsvc/{region}/v1/agents")
}

fn invoke_url(base: &str, bot_id: &str) -> String {
    format!("{base}/{}/invoke", encode(&bot_mri(bot_id)))
}

pub fn invoke_body(invoke: &InvokeRequest) -> Value {
    let mut body = json!({
        "name": invoke.name,
        "appId": invoke.app_id,
        "messageType": CARD_MESSAGE_TYPE,
        "value": invoke.value,
        "imdisplayname": invoke.display_name,
        "userAadToken": TOKEN_PLACEHOLDER,
        "serverMessageId": invoke.server_message_id,
        "conversation": {"id": invoke.conversation_id},
    });
    if let Some(client_message_id) = &invoke.client_message_id {
        body["clientMessageId"] = json!(client_message_id);
    }
    body
}

pub fn parse_invoke_response(body: &Value) -> InvokeResponse {
    let Some(object) = body.as_object() else {
        return InvokeResponse::Empty;
    };
    if let Some(task) = object.get("task") {
        return parse_task(task);
    }
    let status_code = object.get("statusCode").and_then(Value::as_u64);
    let value = object.get("value");
    if let Some(status_code) = status_code.filter(|code| *code >= ERROR_STATUS_FLOOR) {
        return InvokeResponse::Failed {
            status_code: status_code as u16,
            message: value.map(text_of).unwrap_or_default(),
        };
    }
    match object.get("type").and_then(Value::as_str) {
        Some(ADAPTIVE_CARD_TYPE) => value
            .map(|card| InvokeResponse::Card(card.clone()))
            .unwrap_or(InvokeResponse::Empty),
        Some(MESSAGE_TYPE) => InvokeResponse::Message(value.map(text_of).unwrap_or_default()),
        Some(SEARCH_RESPONSE_TYPE) => InvokeResponse::Search(value.cloned().unwrap_or(Value::Null)),
        _ => InvokeResponse::Empty,
    }
}

fn parse_task(task: &Value) -> InvokeResponse {
    let value = task.get("value");
    match task.get("type").and_then(Value::as_str) {
        Some("message") => InvokeResponse::Task(TaskResponse::Message(
            value.map(text_of).unwrap_or_default(),
        )),
        Some("continue") => {
            let Some(value) = value else {
                return InvokeResponse::Empty;
            };
            let content = match value.get("card") {
                Some(card) => TaskContent::Card(card_content(card)),
                None => match value.get("url").and_then(Value::as_str) {
                    Some(url) => TaskContent::Url {
                        url: url.to_owned(),
                        fallback_url: value
                            .get("fallbackUrl")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    },
                    None => return InvokeResponse::Empty,
                },
            };
            InvokeResponse::Task(TaskResponse::Continue(TaskContinue {
                title: value
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                width: dimension(value.get("width")),
                height: dimension(value.get("height")),
                content,
            }))
        }
        _ => InvokeResponse::Empty,
    }
}

fn dimension(value: Option<&Value>) -> Option<u32> {
    let pixels = value?.as_u64()?;
    u32::try_from(pixels).ok().filter(|pixels| *pixels > 0)
}

fn card_content(card: &Value) -> Value {
    match card.get("content") {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_else(|_| card.clone()),
        Some(content) => content.clone(),
        None => card.clone(),
    }
}

fn text_of(value: &Value) -> String {
    match value.as_str() {
        Some(text) => text.to_owned(),
        None => value.to_string(),
    }
}

pub fn parse_chat_apps(body: &Value) -> Result<Vec<ChatApp>> {
    let definitions = body
        .pointer("/value/definitions")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("no app definitions".into()))?;
    Ok(definitions
        .iter()
        .filter_map(|definition| {
            let text = |key: &str| {
                definition
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            Some(ChatApp {
                app_id: text("id")?,
                name: text("name")?,
                bot_ids: definition
                    .get("bots")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|bot| bot.get("id")?.as_str().map(str::to_owned))
                    .collect(),
                small_image_url: text("smallImageUrl"),
                accent_color: text("accentColor"),
                web_application_resource: definition
                    .pointer("/webApplicationInfo/resource")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::IC3;

    use super::*;

    struct Canned {
        answers: Mutex<Vec<(u16, Value)>>,
        sent: Mutex<Vec<(Request, Scope)>>,
    }

    impl CardTransport for Canned {
        async fn send(&self, request: Request, scope: Scope) -> Result<ApiResponse> {
            self.sent.lock().unwrap().push((request, scope));
            let (status, body) = self.answers.lock().unwrap().remove(0);
            Ok(ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    fn canned(answers: Vec<(u16, Value)>) -> CardActions<Canned> {
        CardActions::with_transport(
            Canned {
                answers: Mutex::new(answers),
                sent: Mutex::new(Vec::new()),
            },
            "emea",
            "emea",
        )
    }

    fn sample_request() -> InvokeRequest {
        InvokeRequest {
            bot_id: "0000aaaa-0000-0000-0000-000000000001".into(),
            app_id: "app-1".into(),
            name: "messageback".into(),
            value: json!({"type": "unwatch", "page": "p1"}),
            display_name: "Test User".into(),
            server_message_id: "1700000000001".into(),
            client_message_id: None,
            conversation_id: "19:chat@unq.gbl.spaces".into(),
        }
    }

    fn entitlements() -> Value {
        json!({"value": {"definitions": [
            {
                "id": "app-1",
                "name": "Wiki Bot",
                "smallImageUrl": "https://cdn.example/small.png",
                "largeImageUrl": "https://cdn.example/large.png",
                "accentColor": "#0052CC",
                "bots": [{"id": "0000aaaa-0000-0000-0000-000000000001"}],
                "webApplicationInfo": {"id": "web-1", "resource": "https://bot.example"}
            },
            {"id": "app-2", "name": "Tabs Only"},
            {"name": "No Id"}
        ]}})
    }

    #[tokio::test]
    async fn invoke_posts_to_the_bot_with_the_card_body() {
        let actions = canned(vec![(200, Value::Null)]);
        let response = actions.invoke(sample_request()).await.unwrap();
        assert_eq!(response, InvokeResponse::Empty);
        let sent = actions.transport.sent.lock().unwrap();
        let (request, scope) = &sent[0];
        assert_eq!(
            request.url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/agents/28%3A0000aaaa-0000-0000-0000-000000000001/invoke"
        );
        assert_eq!(scope.resource, IC3);
        assert!(
            request
                .headers
                .contains(&("x-ms-migration".into(), "True".into()))
        );
        assert!(
            request
                .headers
                .contains(&("behavioroverride".into(), "redirectAs404".into()))
        );
        assert_eq!(request.body_tokens[0].resource, SPACES);
        let body = request.body.as_ref().unwrap();
        assert_eq!(body["name"], "messageback");
        assert_eq!(body["appId"], "app-1");
        assert_eq!(body["messageType"], "RichText/Media_Card");
        assert_eq!(body["value"]["type"], "unwatch");
        assert_eq!(body["serverMessageId"], "1700000000001");
        assert_eq!(body["conversation"]["id"], "19:chat@unq.gbl.spaces");
        assert_eq!(body["userAadToken"], request.body_tokens[0].placeholder);
        assert!(body.get("clientMessageId").is_none());
    }

    #[tokio::test]
    async fn invoke_keeps_a_known_client_message_id() {
        let actions = canned(vec![(200, Value::Null)]);
        let request = InvokeRequest {
            client_message_id: Some("client-1".into()),
            ..sample_request()
        };
        actions.invoke(request).await.unwrap();
        let sent = actions.transport.sent.lock().unwrap();
        assert_eq!(
            sent[0].0.body.as_ref().unwrap()["clientMessageId"],
            "client-1"
        );
    }

    #[tokio::test]
    async fn invoke_retries_a_missing_route_in_the_fallback_region() {
        let actions = canned(vec![(404, Value::Null), (200, Value::Null)]);
        actions.invoke(sample_request()).await.unwrap();
        let sent = actions.transport.sent.lock().unwrap();
        assert!(sent[1].0.url.contains("/chatsvc/de/"));
    }

    #[tokio::test]
    async fn invoke_failure_is_an_error_without_retry() {
        let actions = canned(vec![(500, json!({"message": "boom"}))]);
        assert!(actions.invoke(sample_request()).await.is_err());
        assert_eq!(actions.transport.sent.lock().unwrap().len(), 1);
    }

    #[test]
    fn empty_and_unknown_answers_are_empty() {
        assert_eq!(parse_invoke_response(&Value::Null), InvokeResponse::Empty);
        assert_eq!(parse_invoke_response(&json!("")), InvokeResponse::Empty);
        assert_eq!(
            parse_invoke_response(&json!({"other": 1})),
            InvokeResponse::Empty
        );
    }

    #[test]
    fn task_with_a_url() {
        let response = parse_invoke_response(&json!({
            "responseType": "task",
            "task": {"type": "continue", "value": {
                "url": "https://bot.example/task", "fallbackUrl": "https://bot.example/fallback",
                "height": 380, "width": 620, "title": "Settings"
            }}
        }));
        assert_eq!(
            response,
            InvokeResponse::Task(TaskResponse::Continue(TaskContinue {
                title: Some("Settings".into()),
                width: Some(620),
                height: Some(380),
                content: TaskContent::Url {
                    url: "https://bot.example/task".into(),
                    fallback_url: Some("https://bot.example/fallback".into()),
                },
            }))
        );
    }

    #[test]
    fn task_with_a_card_attachment() {
        let response = parse_invoke_response(&json!({
            "task": {"type": "continue", "value": {
                "size": "medium",
                "card": {"contentType": "application/vnd.microsoft.card.adaptive", "content": {"type": "AdaptiveCard"}}
            }}
        }));
        let InvokeResponse::Task(TaskResponse::Continue(task)) = response else {
            panic!("expected a continue task");
        };
        assert_eq!(
            task.content,
            TaskContent::Card(json!({"type": "AdaptiveCard"}))
        );
        assert_eq!(task.width, None);
    }

    #[test]
    fn task_message_and_unusable_continue() {
        assert_eq!(
            parse_invoke_response(&json!({"task": {"type": "message", "value": "Done"}})),
            InvokeResponse::Task(TaskResponse::Message("Done".into()))
        );
        assert_eq!(
            parse_invoke_response(&json!({"task": {"type": "continue", "value": {}}})),
            InvokeResponse::Empty
        );
    }

    #[test]
    fn search_answers_carry_the_results_value() {
        let value = json!({"results": [{"title": "Ada", "value": "ada"}]});
        assert_eq!(
            parse_invoke_response(&json!({
                "type": "application/vnd.microsoft.search.searchResponse",
                "value": value
            })),
            InvokeResponse::Search(value)
        );
    }

    #[test]
    fn universal_action_answers() {
        assert_eq!(
            parse_invoke_response(&json!({
                "statusCode": 200,
                "type": "application/vnd.microsoft.card.adaptive",
                "value": {"type": "AdaptiveCard"}
            })),
            InvokeResponse::Card(json!({"type": "AdaptiveCard"}))
        );
        assert_eq!(
            parse_invoke_response(&json!({
                "statusCode": 200,
                "type": "application/vnd.microsoft.activity.message",
                "value": "Saved"
            })),
            InvokeResponse::Message("Saved".into())
        );
        assert_eq!(
            parse_invoke_response(&json!({"statusCode": 500, "type": "x", "value": "Broken"})),
            InvokeResponse::Failed {
                status_code: 500,
                message: "Broken".into()
            }
        );
    }

    #[tokio::test]
    async fn chat_apps_use_the_spaces_scope_and_parse_definitions() {
        let actions = canned(vec![(200, entitlements())]);
        let apps = actions.chat_apps("19:chat@unq.gbl.spaces").await.unwrap();
        assert_eq!(apps.len(), 2);
        let wiki = &apps[0];
        assert_eq!(wiki.name, "Wiki Bot");
        assert_eq!(
            wiki.small_image_url.as_deref(),
            Some("https://cdn.example/small.png")
        );
        assert_eq!(wiki.accent_color.as_deref(), Some("#0052CC"));
        assert_eq!(
            wiki.web_application_resource.as_deref(),
            Some("https://bot.example")
        );
        assert!(wiki.has_bot("0000AAAA-0000-0000-0000-000000000001"));
        assert!(wiki.has_bot("28:0000aaaa-0000-0000-0000-000000000001"));
        assert!(!apps[1].has_bot("0000aaaa-0000-0000-0000-000000000001"));
        let sent = actions.transport.sent.lock().unwrap();
        assert_eq!(
            sent[0].0.url,
            "https://teams.cloud.microsoft/api/mt/emea/beta/chats/19%3Achat%40unq.gbl.spaces/apps/chatentitlements"
        );
        assert_eq!(sent[0].1, spaces_scope());
    }

    #[test]
    fn entitlements_without_definitions_are_unexpected() {
        assert!(matches!(
            parse_chat_apps(&json!({"value": {}})),
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    #[test]
    fn bot_ids_convert_both_ways() {
        assert_eq!(bot_mri("abc"), "28:abc");
        assert_eq!(bot_mri("28:abc"), "28:abc");
        assert_eq!(bot_guid("28:abc"), "abc");
    }
}
