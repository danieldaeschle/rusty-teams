use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BodyToken {
    pub resource: String,
    pub scope: String,
    pub placeholder: String,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
    pub binary: bool,
    pub anonymous: bool,
    pub body_base64: Option<String>,
    pub body_tokens: Vec<BodyToken>,
}

impl Request {
    /// A successful answer carries `{"base64": .., "contentType": ..}` as its body.
    pub fn binary_get(url: impl Into<String>) -> Self {
        Request {
            binary: true,
            ..Request::get(url)
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Request {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            binary: false,
            anonymous: false,
            body_base64: None,
            body_tokens: Vec::new(),
        }
    }

    /// Replaces `placeholder` in the body with a token for `resource` inside the page, so the token never leaves it.
    pub fn with_body_token(mut self, resource: &str, scope: &str, placeholder: &str) -> Self {
        self.body_tokens.push(BodyToken {
            resource: resource.to_owned(),
            scope: scope.to_owned(),
            placeholder: placeholder.to_owned(),
        });
        self
    }

    /// Sent without an Authorization header, for pre-authenticated URLs such as upload sessions.
    pub fn anonymous_bytes(method: Method, url: impl Into<String>, headers: Vec<(String, String)>, body_base64: String) -> Self {
        Request {
            method,
            headers,
            anonymous: true,
            body_base64: Some(body_base64),
            ..Request::get(url)
        }
    }

    /// Sent without an Authorization header; a successful answer carries `{"base64": .., "contentType": ..}`.
    pub fn anonymous_binary_get(url: impl Into<String>, headers: Vec<(String, String)>) -> Self {
        Request {
            headers,
            anonymous: true,
            ..Request::binary_get(url)
        }
    }

    pub fn delete(url: impl Into<String>) -> Self {
        Request {
            method: Method::Delete,
            ..Request::get(url)
        }
    }

    pub fn with_body(method: Method, url: impl Into<String>, body: Value) -> Self {
        Request {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Some(body),
            binary: false,
            anonymous: false,
            body_base64: None,
            body_tokens: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub body: Value,
    pub retry_after: Option<String>,
}

impl ApiResponse {
    pub fn is_success(&self) -> bool {
        (200..400).contains(&self.status)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct WireRequest<'a> {
    pub method: &'static str,
    pub url: &'a str,
    pub headers: serde_json::Map<String, Value>,
    pub body: Option<String>,
    #[serde(rename = "bodyBase64")]
    pub body_base64: Option<&'a str>,
    pub binary: bool,
    pub anonymous: bool,
    #[serde(rename = "bodyTokens")]
    pub body_tokens: &'a [BodyToken],
}

impl<'a> From<&'a Request> for WireRequest<'a> {
    fn from(request: &'a Request) -> Self {
        let mut headers: serde_json::Map<String, Value> = request
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Value::String(value.clone())))
            .collect();
        if request.body.is_some() && !headers.keys().any(|name| name.eq_ignore_ascii_case("content-type")) {
            headers.insert("Content-Type".into(), "application/json".into());
        }
        WireRequest {
            method: request.method.as_str(),
            url: &request.url,
            headers,
            body: request.body.as_ref().map(Value::to_string),
            body_base64: request.body_base64.as_deref(),
            binary: request.binary,
            anonymous: request.anonymous,
            body_tokens: &request.body_tokens,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireResult {
    pub status: u16,
    #[serde(default)]
    pub body: Value,
    #[serde(default, rename = "retryAfter")]
    pub retry_after: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Outcome {
    pub results: Option<Vec<WireResult>>,
    #[serde(default, rename = "refreshError")]
    pub refresh_error: Option<String>,
}

impl From<WireResult> for ApiResponse {
    fn from(wire: WireResult) -> Self {
        ApiResponse {
            status: wire.status,
            body: wire.body,
            retry_after: wire.retry_after,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_body_gets_content_type() {
        let request = Request::with_body(Method::Post, "https://graph.microsoft.com/v1.0/x", json!({"a": 1}));
        let wire = WireRequest::from(&request);
        assert_eq!(wire.method, "POST");
        assert_eq!(wire.body.as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(wire.headers["Content-Type"], "application/json");
    }

    #[test]
    fn explicit_content_type_is_kept() {
        let mut request = Request::with_body(Method::Post, "u", json!({}));
        request.headers.push(("content-type".into(), "text/plain".into()));
        let wire = WireRequest::from(&request);
        assert_eq!(wire.headers.len(), 1);
    }

    #[test]
    fn binary_get_is_flagged_on_the_wire() {
        let request = Request::binary_get("u");
        assert!(WireRequest::from(&request).binary);
        assert!(!WireRequest::from(&Request::get("u")).binary);
    }

    #[test]
    fn anonymous_bytes_travel_as_base64_without_a_content_type() {
        let request = Request::anonymous_bytes(
            Method::Put,
            "https://upload.example/session",
            vec![("Content-Range".into(), "bytes 0-2/3".into())],
            "AQID".into(),
        );
        let wire = serde_json::to_value(WireRequest::from(&request)).unwrap();
        assert_eq!(wire["method"], "PUT");
        assert_eq!(wire["anonymous"], true);
        assert_eq!(wire["bodyBase64"], "AQID");
        assert_eq!(wire["body"], Value::Null);
        assert!(wire["headers"].get("Content-Type").is_none());
        assert_eq!(wire["headers"]["Content-Range"], "bytes 0-2/3");
    }

    #[test]
    fn anonymous_binary_get_keeps_range_header_and_flags() {
        let request = Request::anonymous_binary_get("https://download.example/file", vec![("Range".into(), "bytes=0-9".into())]);
        let wire = serde_json::to_value(WireRequest::from(&request)).unwrap();
        assert_eq!(wire["method"], "GET");
        assert_eq!(wire["anonymous"], true);
        assert_eq!(wire["binary"], true);
        assert_eq!(wire["bodyBase64"], Value::Null);
        assert_eq!(wire["headers"]["Range"], "bytes=0-9");
    }

    #[test]
    fn body_tokens_travel_as_placeholders() {
        let request = Request::with_body(Method::Post, "u", json!({"token": "@@t@@"}))
            .with_body_token(
                "https://api.spaces.skype.com",
                "user_impersonation",
                "@@t@@",
            );
        let wire = serde_json::to_value(WireRequest::from(&request)).unwrap();
        assert_eq!(
            wire["bodyTokens"][0]["resource"],
            "https://api.spaces.skype.com"
        );
        assert_eq!(wire["bodyTokens"][0]["placeholder"], "@@t@@");
        assert_eq!(wire["body"], r#"{"token":"@@t@@"}"#);
    }

    #[test]
    fn get_has_no_body() {
        let request = Request::get("u");
        let wire = WireRequest::from(&request);
        assert!(wire.body.is_none() && wire.headers.is_empty());
    }

    #[test]
    fn parses_results_outcome() {
        let outcome: Outcome = serde_json::from_value(json!({
            "results": [{"status": 200, "retryAfter": null, "body": {"value": []}}, {"status": 429, "retryAfter": "4", "body": "slow"}]
        }))
        .unwrap();
        let responses: Vec<ApiResponse> = outcome.results.unwrap().into_iter().map(Into::into).collect();
        assert_eq!(responses[0].status, 200);
        assert_eq!(responses[1].retry_after.as_deref(), Some("4"));
        assert!(!responses[1].is_success());
    }

    #[test]
    fn parses_no_token_outcome() {
        let outcome: Outcome =
            serde_json::from_value(json!({"noToken": true, "refreshError": "invalid_grant"})).unwrap();
        assert!(outcome.results.is_none());
        assert_eq!(outcome.refresh_error.as_deref(), Some("invalid_grant"));
    }
}
