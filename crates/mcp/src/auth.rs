use axum::body::Body;
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, HOST, ORIGIN};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

pub const MAX_BODY_BYTES: usize = 1024 * 1024;
const LOOPBACK_HOSTS: [&str; 2] = ["127.0.0.1", "localhost"];
const BEARER_PREFIX: &str = "Bearer ";

#[derive(Clone)]
pub struct AuthState {
    pub port: u16,
    pub token: String,
}

impl AuthState {
    pub fn allowed_hosts(&self) -> Vec<String> {
        LOOPBACK_HOSTS
            .iter()
            .map(|host| format!("{host}:{}", self.port))
            .collect()
    }

    pub fn allowed_origins(&self) -> Vec<String> {
        self.allowed_hosts()
            .into_iter()
            .map(|authority| format!("http://{authority}"))
            .collect()
    }

    fn host_allowed(&self, headers: &HeaderMap) -> bool {
        header_text(headers, HOST)
            .is_some_and(|host| self.allowed_hosts().iter().any(|allowed| allowed == host))
    }

    fn origin_allowed(&self, headers: &HeaderMap) -> bool {
        match headers.get(ORIGIN) {
            None => true,
            Some(origin) => origin.to_str().is_ok_and(|origin| {
                self.allowed_origins()
                    .iter()
                    .any(|allowed| allowed == origin)
            }),
        }
    }

    fn token_valid(&self, headers: &HeaderMap) -> bool {
        header_text(headers, AUTHORIZATION)
            .and_then(|value| value.strip_prefix(BEARER_PREFIX))
            .is_some_and(|presented| presented.as_bytes().ct_eq(self.token.as_bytes()).into())
    }
}

fn header_text(headers: &HeaderMap, name: axum::http::HeaderName) -> Option<&str> {
    headers.get(name)?.to_str().ok()
}

pub async fn require_auth(
    State(auth): State<AuthState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let headers = request.headers();
    if !auth.host_allowed(headers) || !auth.origin_allowed(headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !auth.token_valid(headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

pub async fn healthz() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn auth() -> AuthState {
        AuthState {
            port: 47615,
            token: "secret".to_owned(),
        }
    }

    fn headers(pairs: &[(axum::http::HeaderName, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(name.clone(), HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn host_must_be_loopback_with_the_port() {
        let cases = [
            ("127.0.0.1:47615", true),
            ("localhost:47615", true),
            ("localhost:1", false),
            ("127.0.0.1", false),
            ("evil.example:47615", false),
            ("evil.example", false),
        ];
        for (host, allowed) in cases {
            assert_eq!(
                auth().host_allowed(&headers(&[(HOST, host)])),
                allowed,
                "{host}"
            );
        }
        assert!(!auth().host_allowed(&HeaderMap::new()));
    }

    #[test]
    fn origin_is_absent_or_loopback() {
        let cases = [
            (None, true),
            (Some("http://127.0.0.1:47615"), true),
            (Some("http://localhost:47615"), true),
            (Some("https://localhost:47615"), false),
            (Some("http://evil.example"), false),
            (Some("null"), false),
        ];
        for (origin, allowed) in cases {
            let map = origin.map_or_else(HeaderMap::new, |value| headers(&[(ORIGIN, value)]));
            assert_eq!(auth().origin_allowed(&map), allowed, "{origin:?}");
        }
    }

    #[test]
    fn token_needs_the_exact_bearer_value() {
        let cases = [
            ("Bearer secret", true),
            ("Bearer secre", false),
            ("Bearer secrets", false),
            ("bearer secret", false),
            ("secret", false),
        ];
        for (value, valid) in cases {
            assert_eq!(
                auth().token_valid(&headers(&[(AUTHORIZATION, value)])),
                valid,
                "{value}"
            );
        }
        assert!(!auth().token_valid(&HeaderMap::new()));
    }
}
