use serde_json::Value;
use session::{App, is_login_url};

/// Coarse state of the app tab for error reports, never the URL itself.
pub(crate) fn tab_state(app: App, url: &str) -> &'static str {
    if url.is_empty() {
        "no document"
    } else if app.is_park_url(url) {
        "parked"
    } else if is_login_url(url) {
        "login"
    } else if app.origins().iter().any(|origin| url.starts_with(origin)) {
        "app"
    } else if url.starts_with("about:") {
        "about"
    } else if url.starts_with("chrome-error:") {
        "error page"
    } else {
        "other"
    }
}

pub(crate) struct CallContext<'a> {
    pub method: &'a str,
    pub params_bytes: usize,
    pub tab: &'static str,
    pub in_flight: usize,
}

/// WebView2 answers every protocol error with E_INVALIDARG; the protocol's own error sits in the JSON.
pub(crate) fn failure_reason(context: &CallContext, hresult: &str, json: &str) -> String {
    let parsed = serde_json::from_str::<Value>(json).unwrap_or(Value::Null);
    let error = parsed.get("error").unwrap_or(&parsed);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(hresult);
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .map(|code| format!(" code {code},"))
        .unwrap_or_default();
    format!(
        "{message} [{}{code} params {} bytes, tab {}, {} call(s) in flight]",
        context.method, context.params_bytes, context.tab, context.in_flight,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HRESULT: &str = "The parameter is incorrect. (0x80070057)";

    fn context() -> CallContext<'static> {
        CallContext {
            method: "Runtime.evaluate",
            params_bytes: 48213,
            tab: "parked",
            in_flight: 2,
        }
    }

    #[test]
    fn the_protocol_message_leads_the_reason() {
        let json = r#"{"code":-32000,"message":"Execution context was destroyed."}"#;
        assert_eq!(
            failure_reason(&context(), HRESULT, json),
            "Execution context was destroyed. [Runtime.evaluate code -32000, params 48213 bytes, tab parked, 2 call(s) in flight]"
        );
    }

    #[test]
    fn a_nested_protocol_error_is_read_too() {
        let json = r#"{"error":{"code":-32601,"message":"'Foo.bar' wasn't found"}}"#;
        assert!(
            failure_reason(&context(), HRESULT, json)
                .starts_with("'Foo.bar' wasn't found [Runtime.evaluate code -32601,")
        );
    }

    #[test]
    fn without_a_protocol_error_the_hresult_and_the_call_are_named() {
        assert_eq!(
            failure_reason(&context(), HRESULT, ""),
            "The parameter is incorrect. (0x80070057) [Runtime.evaluate params 48213 bytes, tab parked, 2 call(s) in flight]"
        );
    }

    #[test]
    fn the_tab_state_never_carries_the_url() {
        assert_eq!(
            tab_state(App::Teams, "https://teams.cloud.microsoft/robots.txt"),
            "parked"
        );
        assert_eq!(
            tab_state(App::Teams, "https://teams.cloud.microsoft/v2/?ctx=chat"),
            "app"
        );
        assert_eq!(tab_state(App::Teams, "about:blank"), "about");
        assert_eq!(
            tab_state(App::Teams, "chrome-error://chromewebdata/"),
            "error page"
        );
        assert_eq!(tab_state(App::Teams, ""), "no document");
        assert_eq!(
            tab_state(App::Teams, "https://example.com/private/path"),
            "other"
        );
    }
}
