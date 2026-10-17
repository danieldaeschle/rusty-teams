use serde_json::{Value, json};
use session::{ApiResponse, App, Request, Scope, Session};

use crate::error::Result;

pub const CSA_RESOURCE: &str = "https://chatsvcagg.teams.microsoft.com";
pub const CSA_SCOPE: &str = "user_impersonation";

pub trait CsaTransport {
    fn send(&self, request: Request) -> impl std::future::Future<Output = Result<ApiResponse>>;
}

pub struct SessionTransport {
    session: Session,
    scope: Scope,
}

impl SessionTransport {
    pub fn new(session: &Session) -> Self {
        SessionTransport::with_scope(session, Scope::new(CSA_RESOURCE, CSA_SCOPE))
    }

    pub fn with_scope(session: &Session, scope: Scope) -> Self {
        SessionTransport {
            session: session.clone(),
            scope,
        }
    }
}

impl CsaTransport for SessionTransport {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        let mut answers = self.session.batch(&[request], &self.scope).await?;
        Ok(answers.remove(0))
    }
}

const SKYPE_FETCH_BODY: &str = include_str!("../../assets/csa_skype_fetch.js");

/// Teams sends the skype token and the chat service endpoint next to the CSA bearer on the pinned post routes.
pub struct SkypeTokenTransport {
    session: Session,
    scope: Scope,
}

impl SkypeTokenTransport {
    pub fn new(session: &Session) -> Self {
        SkypeTokenTransport {
            session: session.clone(),
            scope: Scope::new(CSA_RESOURCE, CSA_SCOPE),
        }
    }
}

pub fn skype_fetch_args(request: &Request) -> Value {
    let headers: serde_json::Map<String, Value> = request
        .headers
        .iter()
        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
        .collect();
    json!({
        "method": request.method.as_str(),
        "url": request.url,
        "headers": headers,
        "body": request.body,
    })
}

pub fn skype_fetch_answer(answer: Value) -> ApiResponse {
    ApiResponse {
        status: answer
            .get("status")
            .and_then(Value::as_u64)
            .and_then(|status| u16::try_from(status).ok())
            .unwrap_or(0),
        retry_after: answer
            .get("retryAfter")
            .and_then(Value::as_str)
            .map(str::to_owned),
        body: answer.get("body").cloned().unwrap_or(Value::Null),
    }
}

impl CsaTransport for SkypeTokenTransport {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        let answer = self
            .session
            .run_with_token(
                App::Teams,
                &self.scope,
                SKYPE_FETCH_BODY,
                &skype_fetch_args(&request),
                false,
            )
            .await?;
        Ok(skype_fetch_answer(answer))
    }
}

#[cfg(test)]
mod tests {
    use session::Method;

    use super::*;

    #[test]
    fn skype_fetch_args_carry_method_url_headers_and_body() {
        let mut request = Request::delete("https://csa/pin");
        request
            .headers
            .push(("x-ms-continuation".to_owned(), "next".to_owned()));
        assert_eq!(
            skype_fetch_args(&request),
            json!({
                "method": "DELETE",
                "url": "https://csa/pin",
                "headers": {"x-ms-continuation": "next"},
                "body": null,
            })
        );
        let post = Request {
            method: Method::Post,
            ..Request::get("https://csa/pin")
        };
        assert_eq!(skype_fetch_args(&post)["method"], "POST");
    }

    #[test]
    fn page_answers_become_api_responses() {
        let answer = skype_fetch_answer(json!({"status": 204, "retryAfter": null, "body": null}));
        assert_eq!(answer.status, 204);
        assert!(answer.is_success());
        let failed = skype_fetch_answer(json!({"status": 0, "body": "authz_http_401"}));
        assert!(!failed.is_success());
        assert_eq!(failed.body, json!("authz_http_401"));
    }
}
