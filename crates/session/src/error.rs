use serde_json::Value;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no browser on {endpoint}: {reason}")]
    NoBrowser { endpoint: String, reason: String },
    #[error("no open Teams or Outlook tab in the browser")]
    NoAppTab,
    #[error("Microsoft 365 login required: {0}")]
    LoginRequired(String),
    #[error("no fresh token for {scope}{detail}")]
    NoFreshToken { scope: String, detail: String },
    #[error("HTTP {status}: {message} ({url})")]
    Api {
        status: u16,
        url: String,
        message: String,
        body: Box<Value>,
    },
    #[error("browser protocol error: {0}")]
    Cdp(String),
    #[error("tab connection lost during a write ({0}); check whether it was sent before retrying")]
    WriteInterrupted(String),
}

impl Error {
    pub fn api(status: u16, url: &str, body: Value) -> Self {
        let message = match body.pointer("/error/message").and_then(Value::as_str) {
            Some(text) => text.to_owned(),
            None => body.to_string().chars().take(300).collect(),
        };
        Error::Api {
            status,
            url: url.to_owned(),
            message,
            body: Box::new(body),
        }
    }

    /// Microsoft refused the refresh token, e.g. a tenant that wants MFA: only a sign-in page can help.
    pub fn refresh_refused(&self) -> bool {
        matches!(self, Error::NoFreshToken { detail, .. } if is_refusal(detail))
    }
}

pub(crate) fn is_refusal(reason: &str) -> bool {
    reason.contains("invalid_grant") || reason.contains("interaction_required")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn api_error_uses_graph_message() {
        let error = Error::api(
            403,
            "https://graph.microsoft.com/v1.0/me",
            json!({"error": {"message": "Forbidden"}}),
        );
        assert_eq!(
            error.to_string(),
            "HTTP 403: Forbidden (https://graph.microsoft.com/v1.0/me)"
        );
    }

    #[test]
    fn a_refused_refresh_is_told_apart_from_a_missing_token() {
        let refused = |detail: &str| {
            Error::NoFreshToken {
                scope: "s".into(),
                detail: detail.into(),
            }
            .refresh_refused()
        };
        assert!(refused(
            " (token refresh failed in teams: invalid_grant: AADSTS50076: MFA)"
        ));
        assert!(refused(
            " (token refresh failed in teams: interaction_required)"
        ));
        assert!(!refused(" [teams: https://teams.cloud.microsoft/]"));
        assert!(!Error::LoginRequired("x".into()).refresh_refused());
    }

    #[test]
    fn api_error_truncates_plain_bodies() {
        let Error::Api { message, .. } = Error::api(0, "u", json!("x".repeat(1000))) else {
            unreachable!()
        };
        assert_eq!(message.chars().count(), 300);
    }
}
