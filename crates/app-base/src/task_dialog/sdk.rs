use serde_json::{Map, Value, json};

pub const HOST_ORIGIN: &str = "https://teams.cloud.microsoft";
pub const HOST_PAGE_PATH: &str = "/__rusty/dialog";
const SDK_VERSION: &str = "2.1.0";
const FRAME_CONTEXT: &str = "task";
const CLIENT_TYPE: &str = "web";
const HOST_NAME: &str = "Teams";
const LOCALE: &str = "en-us";
const NOT_SUPPORTED: &str = "not supported";
const NO_SSO_RESOURCE: &str = "resourceDisabled";
const TOKEN_MARGIN_MS: u64 = 60_000;
const CHANNEL_SUFFIX: &str = "@thread.tacv2";
const HOST_PAGE_TEMPLATE: &str = include_str!("host_page.html");

pub fn host_page_url() -> String {
    format!("{HOST_ORIGIN}{HOST_PAGE_PATH}")
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostContext {
    pub user_id: String,
    pub user_display_name: String,
    pub tenant_id: String,
    pub conversation_id: String,
    pub app_id: String,
    pub session_id: String,
    pub dark: bool,
    pub web_application_resource: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Submit(Value),
    Resize {
        width: Option<u32>,
        height: Option<u32>,
    },
    OpenLink(String),
}

#[derive(Debug, Default, PartialEq)]
pub struct Handled {
    pub directives: Vec<Value>,
    pub effect: Option<Effect>,
}

struct Request {
    id: Value,
    uuid: Value,
    func: String,
    args: Vec<Value>,
}

impl Request {
    fn parse(envelope: &Value) -> Option<Request> {
        let message = envelope.get("message")?;
        Some(Request {
            id: message.get("id").cloned().unwrap_or(Value::Null),
            uuid: message.get("uuidAsString").cloned().unwrap_or(Value::Null),
            func: message.get("func")?.as_str()?.to_owned(),
            args: message
                .get("args")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        })
    }

    fn reply(&self, args: Vec<Value>) -> Value {
        json!({
            "kind": "post",
            "message": {"id": self.id, "uuidAsString": self.uuid, "args": args},
        })
    }

    fn auth_token(&self, resource: &str) -> Value {
        json!({
            "kind": "authToken",
            "id": self.id,
            "uuidAsString": self.uuid,
            "resource": resource,
        })
    }
}

pub fn handle(raw: &str, context: &HostContext) -> Handled {
    let Ok(envelope) = serde_json::from_str::<Value>(raw) else {
        return Handled::default();
    };
    match envelope.get("kind").and_then(Value::as_str) {
        Some("sdk") => Request::parse(&envelope)
            .map_or_else(Handled::default, |request| call(&request, context)),
        Some("newWindow") => new_window(&envelope),
        _ => Handled::default(),
    }
}

fn new_window(envelope: &Value) -> Handled {
    let link = envelope
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Handled {
        directives: Vec::new(),
        effect: is_web_link(link).then(|| Effect::OpenLink(link.to_owned())),
    }
}

fn is_web_link(link: &str) -> bool {
    link.starts_with("https://") || link.starts_with("http://")
}

fn call(request: &Request, context: &HostContext) -> Handled {
    match request.func.as_str() {
        "initialize" => answer(request.reply(vec![
            json!(FRAME_CONTEXT),
            json!(CLIENT_TYPE),
            json!(runtime_config().to_string()),
            json!(SDK_VERSION),
        ])),
        "getContext" => answer(request.reply(vec![legacy_context(context)])),
        "authentication.getAuthToken" => {
            answer(match context.web_application_resource.as_deref() {
                Some(resource) => request.auth_token(resource),
                None => request.reply(vec![json!(false), json!(NO_SSO_RESOURCE)]),
            })
        }
        "tasks.completeTask" | "tasks.submitTask" | "dialog.url.submit" => Handled {
            directives: Vec::new(),
            effect: request
                .args
                .first()
                .filter(|result| !result.is_null())
                .cloned()
                .map(Effect::Submit),
        },
        "tasks.updateTask" | "dialog.update.resize" => Handled {
            directives: Vec::new(),
            effect: Some(resize_of(request.args.first())),
        },
        "executeDeepLink" | "app.openLink" => open_link(request),
        "registerHandler" => register_handler(request, context),
        name if is_notification(name) => Handled::default(),
        _ => answer(request.reply(vec![json!(false), json!(NOT_SUPPORTED)])),
    }
}

fn answer(directive: Value) -> Handled {
    Handled {
        directives: vec![directive],
        effect: None,
    }
}

fn is_notification(name: &str) -> bool {
    name.starts_with("appInitialization.") || name == "readyToUnload" || name == "setFrameContext"
}

fn resize_of(dimensions: Option<&Value>) -> Effect {
    let pixels = |key: &str| {
        dimensions
            .and_then(|dimensions| dimensions.get(key))
            .and_then(Value::as_u64)
            .and_then(|pixels| u32::try_from(pixels).ok())
    };
    Effect::Resize {
        width: pixels("width"),
        height: pixels("height"),
    }
}

fn open_link(request: &Request) -> Handled {
    let link = request
        .args
        .first()
        .and_then(Value::as_str)
        .unwrap_or_default();
    if is_web_link(link) {
        Handled {
            directives: vec![request.reply(vec![json!(true)])],
            effect: Some(Effect::OpenLink(link.to_owned())),
        }
    } else {
        answer(request.reply(vec![json!(false), json!(NOT_SUPPORTED)]))
    }
}

fn register_handler(request: &Request, context: &HostContext) -> Handled {
    let wants_theme = request.args.first().and_then(Value::as_str) == Some("themeChange");
    Handled {
        directives: if wants_theme {
            vec![json!({
                "kind": "post",
                "message": {"func": "themeChange", "args": [theme_name(context.dark)]},
            })]
        } else {
            Vec::new()
        },
        effect: None,
    }
}

fn theme_name(dark: bool) -> &'static str {
    if dark { "dark" } else { "default" }
}

pub fn runtime_config() -> Value {
    json!({
        "apiVersion": 4,
        "hostVersionsInfo": {"adaptiveCardSchemaVersion": {"major": 1, "minor": 5}},
        "isLegacyTeams": true,
        "supports": {
            "dialog": {
                "card": {"bot": {}},
                "url": {"bot": {}, "parentCommunication": {}},
                "update": {},
            },
            "logs": {},
            "teamsCore": {},
        },
    })
}

pub fn legacy_context(context: &HostContext) -> Value {
    let mut fields = Map::new();
    let mut put = |key: &str, value: Value| {
        fields.insert(key.to_owned(), value);
    };
    put("locale", json!(LOCALE));
    put("theme", json!(theme_name(context.dark)));
    put("frameContext", json!(FRAME_CONTEXT));
    put("hostClientType", json!(CLIENT_TYPE));
    put("hostName", json!(HOST_NAME));
    put("sessionId", json!(context.session_id));
    put("appSessionId", json!(context.session_id));
    put("userObjectId", json!(context.user_id));
    put("userDisplayName", json!(context.user_display_name));
    put("tid", json!(context.tenant_id));
    put("appId", json!(context.app_id));
    put("isFullScreen", json!(false));
    let conversation_key = if context.conversation_id.ends_with(CHANNEL_SUFFIX) {
        "channelId"
    } else {
        "chatId"
    };
    put(conversation_key, json!(context.conversation_id));
    Value::Object(fields)
}

pub struct HostPage<'a> {
    pub task_url: &'a str,
    pub background: u32,
    pub token_acquirer_script: &'a str,
}

impl HostPage<'_> {
    pub fn html(&self) -> String {
        let config = json!({"url": self.task_url, "marginMs": TOKEN_MARGIN_MS});
        let safe_config = config.to_string().replace('<', "\\u003c");
        HOST_PAGE_TEMPLATE
            .replace("__BACKGROUND__", &format!("#{:06x}", self.background))
            .replace("__CONFIG__", &safe_config)
            .replace("__ACQUIRE__", self.token_acquirer_script)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> HostContext {
        HostContext {
            user_id: "user-1".into(),
            user_display_name: "Ada".into(),
            tenant_id: "tenant-1".into(),
            conversation_id: "19:chat@thread.v2".into(),
            app_id: "app-1".into(),
            session_id: "session-1".into(),
            dark: true,
            web_application_resource: Some("api://bot.example/app-1".into()),
        }
    }

    fn call(func: &str, args: Value) -> String {
        json!({"kind": "sdk", "message": {"id": 7, "uuidAsString": "uuid-7", "func": func, "args": args}})
            .to_string()
    }

    fn reply_args(handled: &Handled) -> Value {
        handled.directives[0]["message"]["args"].clone()
    }

    #[test]
    fn replies_carry_the_numeric_id_and_the_uuid_string() {
        let handled = handle(&call("getContext", json!([])), &context());
        let message = &handled.directives[0]["message"];
        assert_eq!(handled.directives[0]["kind"], "post");
        assert_eq!(message["id"], 7);
        assert_eq!(message["uuidAsString"], "uuid-7");
    }

    #[test]
    fn initialize_answers_a_task_frame_on_the_web_client_with_a_runtime_config_string() {
        let handled = handle(&call("initialize", json!(["2.0.0", 4, []])), &context());
        let args = reply_args(&handled);
        assert_eq!(args[0], "task");
        assert_eq!(args[1], "web");
        let runtime: Value = serde_json::from_str(args[2].as_str().unwrap()).unwrap();
        assert_eq!(runtime, runtime_config());
        assert_eq!(args[3], SDK_VERSION);
        assert!(handled.effect.is_none());
    }

    #[test]
    fn the_runtime_config_supports_bot_url_dialogs_and_resizing() {
        let runtime = runtime_config();
        assert_eq!(runtime["apiVersion"], 4);
        assert!(runtime["supports"]["dialog"]["url"]["bot"].is_object());
        assert!(runtime["supports"]["dialog"]["update"].is_object());
    }

    #[test]
    fn get_context_describes_the_user_tenant_chat_and_theme() {
        let args = reply_args(&handle(&call("getContext", json!([])), &context()));
        let legacy = &args[0];
        assert_eq!(legacy["frameContext"], "task");
        assert_eq!(legacy["hostClientType"], "web");
        assert_eq!(legacy["hostName"], "Teams");
        assert_eq!(legacy["locale"], "en-us");
        assert_eq!(legacy["theme"], "dark");
        assert_eq!(legacy["userObjectId"], "user-1");
        assert_eq!(legacy["tid"], "tenant-1");
        assert_eq!(legacy["chatId"], "19:chat@thread.v2");
        assert_eq!(legacy["appSessionId"], "session-1");
        assert!(legacy.get("channelId").is_none());
    }

    #[test]
    fn channel_conversations_are_reported_as_channels() {
        let mut channel = context();
        channel.conversation_id = "19:abc@thread.tacv2".into();
        let legacy = legacy_context(&channel);
        assert_eq!(legacy["channelId"], "19:abc@thread.tacv2");
        assert!(legacy.get("chatId").is_none());
    }

    #[test]
    fn a_light_theme_is_called_default() {
        let mut light = context();
        light.dark = false;
        assert_eq!(legacy_context(&light)["theme"], "default");
    }

    #[test]
    fn auth_tokens_are_requested_from_the_page_for_the_app_resource() {
        let handled = handle(
            &call("authentication.getAuthToken", json!([[], null, false])),
            &context(),
        );
        assert_eq!(
            handled.directives,
            vec![json!({
                "kind": "authToken",
                "id": 7,
                "uuidAsString": "uuid-7",
                "resource": "api://bot.example/app-1",
            })]
        );
    }

    #[test]
    fn auth_tokens_fail_when_the_app_has_no_sso_resource() {
        let mut without_resource = context();
        without_resource.web_application_resource = None;
        let handled = handle(
            &call("authentication.getAuthToken", json!([])),
            &without_resource,
        );
        assert_eq!(reply_args(&handled), json!([false, NO_SSO_RESOURCE]));
    }

    #[test]
    fn submitting_hands_the_result_to_the_bot() {
        for func in [
            "tasks.completeTask",
            "tasks.submitTask",
            "dialog.url.submit",
        ] {
            let handled = handle(&call(func, json!([{"choice": "a"}, ["app-1"]])), &context());
            assert_eq!(handled.effect, Some(Effect::Submit(json!({"choice": "a"}))));
            assert!(handled.directives.is_empty());
        }
    }

    #[test]
    fn submitting_without_a_result_sends_nothing() {
        let handled = handle(&call("tasks.completeTask", json!([])), &context());
        assert_eq!(handled, Handled::default());
    }

    #[test]
    fn resizing_takes_numeric_dimensions_only() {
        let handled = handle(
            &call(
                "tasks.updateTask",
                json!([{"width": 700, "height": "large"}]),
            ),
            &context(),
        );
        assert_eq!(
            handled.effect,
            Some(Effect::Resize {
                width: Some(700),
                height: None
            })
        );
    }

    #[test]
    fn web_links_open_in_the_browser_and_other_schemes_are_refused() {
        let handled = handle(
            &call("executeDeepLink", json!(["https://example.com/page"])),
            &context(),
        );
        assert_eq!(
            handled.effect,
            Some(Effect::OpenLink("https://example.com/page".into()))
        );
        assert_eq!(reply_args(&handled), json!([true]));
        let refused = handle(
            &call("executeDeepLink", json!(["file:///etc/passwd"])),
            &context(),
        );
        assert!(refused.effect.is_none());
        assert_eq!(reply_args(&refused), json!([false, NOT_SUPPORTED]));
    }

    #[test]
    fn popups_of_the_page_open_in_the_browser() {
        let popup = |url: &str| json!({"kind": "newWindow", "url": url}).to_string();
        let handled = handle(&popup("https://example.com/docs"), &context());
        assert_eq!(
            handled.effect,
            Some(Effect::OpenLink("https://example.com/docs".into()))
        );
        assert!(
            handle(&popup("javascript:alert(1)"), &context())
                .effect
                .is_none()
        );
    }

    #[test]
    fn registering_a_theme_handler_pushes_the_current_theme() {
        let handled = handle(&call("registerHandler", json!(["themeChange"])), &context());
        assert_eq!(
            handled.directives,
            vec![json!({"kind": "post", "message": {"func": "themeChange", "args": ["dark"]}})]
        );
        let other = handle(&call("registerHandler", json!(["load"])), &context());
        assert_eq!(other, Handled::default());
    }

    #[test]
    fn lifecycle_notifications_are_not_answered() {
        for func in [
            "appInitialization.appLoaded",
            "appInitialization.success",
            "appInitialization.failure",
            "readyToUnload",
        ] {
            assert_eq!(
                handle(&call(func, json!([])), &context()),
                Handled::default()
            );
        }
    }

    #[test]
    fn unknown_calls_are_answered_as_unsupported() {
        let handled = handle(&call("media.captureImage", json!([])), &context());
        assert_eq!(reply_args(&handled), json!([false, NOT_SUPPORTED]));
    }

    #[test]
    fn malformed_or_foreign_messages_are_ignored() {
        for raw in [
            "not json",
            r#"{"kind":"other"}"#,
            r#"{"kind":"sdk","message":{"id":1}}"#,
        ] {
            assert_eq!(handle(raw, &context()), Handled::default());
        }
    }

    #[test]
    fn the_host_page_embeds_the_task_url_and_cannot_be_broken_out_of() {
        let page = HostPage {
            task_url: "https://bot.example/task?x=</script><script>alert(1)",
            background: 0x0f0f10,
            token_acquirer_script: "(async () => () => null)",
        }
        .html();
        assert!(page.contains("#0f0f10"));
        assert!(page.contains("(async () => () => null)"));
        assert!(page.contains(r"\u003c/script>\u003cscript>alert(1)"));
        assert_eq!(page.matches("</script>").count(), 1);
        assert!(
            !page.contains("__CONFIG__")
                && !page.contains("__ACQUIRE__")
                && !page.contains("__BACKGROUND__")
        );
    }

    #[test]
    fn the_host_page_url_lives_on_an_origin_the_sdk_trusts() {
        assert_eq!(
            host_page_url(),
            "https://teams.cloud.microsoft/__rusty/dialog"
        );
    }
}
