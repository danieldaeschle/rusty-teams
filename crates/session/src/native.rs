use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use futures_util::future::join_all;
use serde_json::{Value, json};

use crate::app::App;
use crate::request::{ApiResponse, Method, Request};
use crate::scope::Scope;
use crate::session::Session;

const TOKEN_BODY: &str = "return token;";
const CONCURRENCY: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenKey {
    resource: String,
    name: String,
    nested_client: Option<&'static str>,
}

impl TokenKey {
    fn of(scope: &Scope) -> Self {
        TokenKey {
            resource: scope.resource.to_string(),
            name: scope.name.to_string(),
            nested_client: scope.nested_app.map(|app| app.client_id),
        }
    }
}

struct HeldToken {
    app: App,
    token: String,
    usable_until: Instant,
}

/// Sends requests from Rust; the app tab is asked only for an access token, which stays in memory until shortly before it expires.
pub(crate) struct NativeHttp {
    client: reqwest::Client,
    tokens: Mutex<HashMap<TokenKey, HeldToken>>,
    max_bytes: u64,
}

impl NativeHttp {
    pub(crate) fn new(timeout: Duration, max_bytes: u64) -> Option<Self> {
        ensure_crypto_provider();
        let client = reqwest::Client::builder().timeout(timeout).build().ok()?;
        Some(NativeHttp { client, tokens: Mutex::new(HashMap::new()), max_bytes })
    }

    pub(crate) fn forget(&self, scope: &Scope) {
        self.tokens.lock().expect("token lock").remove(&TokenKey::of(scope));
    }

    fn held(&self, scope: &Scope) -> Option<(App, String)> {
        let tokens = self.tokens.lock().expect("token lock");
        let held = tokens.get(&TokenKey::of(scope))?;
        (Instant::now() < held.usable_until).then(|| (held.app, held.token.clone()))
    }

    /// `None` hands the batch to the page path, which waits for sign-in, reloads and explains failures.
    pub(crate) async fn run(&self, session: &Session, requests: &[&Request], scope: &Scope, force_refresh: bool) -> Option<Vec<ApiResponse>> {
        if !requests.iter().all(|request| eligible(request)) {
            return None;
        }
        let credential = if requests.iter().all(|request| request.anonymous) {
            None
        } else {
            Some(self.token(session, scope, force_refresh).await?)
        };
        let mut answers = Vec::with_capacity(requests.len());
        for chunk in requests.chunks(CONCURRENCY) {
            let mut pending = Vec::with_capacity(chunk.len());
            for request in chunk {
                pending.push(self.execute(request, credential.as_ref()));
            }
            answers.extend(join_all(pending).await);
        }
        if answers.iter().any(|answer| answer.status == 401) {
            self.forget(scope);
        }
        Some(answers)
    }

    async fn token(&self, session: &Session, scope: &Scope, force_refresh: bool) -> Option<(App, String)> {
        if !force_refresh && let Some(held) = self.held(scope) {
            return Some(held);
        }
        for app in scope.tab_order() {
            let Ok(Value::String(token)) = session.run_with_token(app, scope, TOKEN_BODY, &json!({}), force_refresh).await else {
                continue;
            };
            let usable_until = usable_until(&token, session.config.refresh_margin)?;
            self.tokens
                .lock()
                .expect("token lock")
                .insert(TokenKey::of(scope), HeldToken { app, token: token.clone(), usable_until });
            return Some((app, token));
        }
        None
    }

    async fn execute(&self, request: &Request, credential: Option<&(App, String)>) -> ApiResponse {
        match self.exchange(request, credential).await {
            Ok(response) => response,
            Err(error) => ApiResponse { status: 0, body: Value::String(error.without_url().to_string()), retry_after: None },
        }
    }

    async fn exchange(&self, request: &Request, credential: Option<&(App, String)>) -> reqwest::Result<ApiResponse> {
        let mut builder = self.client.request(method(request.method), &request.url);
        let mut has_content_type = false;
        for (name, value) in &request.headers {
            has_content_type |= name.eq_ignore_ascii_case("content-type");
            builder = builder.header(name, value);
        }
        if let Some((app, token)) = credential.filter(|_| !request.anonymous) {
            builder = builder.bearer_auth(token).header("Origin", app.origins()[0]);
        }
        if let Some(encoded) = &request.body_base64 {
            builder = builder.body(STANDARD.decode(encoded).unwrap_or_default());
        } else if let Some(body) = &request.body {
            if !has_content_type {
                builder = builder.header("Content-Type", "application/json");
            }
            builder = builder.body(body.to_string());
        } else if request.method != Method::Get {
            // Some Teams fronts answer a bodyless DELETE or POST with 411 Length Required.
            builder = builder.header(reqwest::header::CONTENT_LENGTH, 0);
        }
        let response = builder.send().await?;
        let status = response.status().as_u16();
        let retry_after = response.headers().get("Retry-After").and_then(|value| value.to_str().ok()).map(str::to_owned);
        if request.binary && response.status().is_success() {
            let content_type = response.headers().get("Content-Type").and_then(|value| value.to_str().ok()).map(str::to_owned);
            let bytes = response.bytes().await?;
            if bytes.len() as u64 > self.max_bytes {
                let body = format!("download is {} bytes, over the {} limit", bytes.len(), self.max_bytes);
                return Ok(ApiResponse { status: 413, body: Value::String(body), retry_after: None });
            }
            let body = json!({"base64": STANDARD.encode(&bytes), "contentType": content_type});
            return Ok(ApiResponse { status, body, retry_after });
        }
        let text = response.text().await?;
        let body = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
        Ok(ApiResponse { status, body, retry_after })
    }
}

const TEAMS_BEARER_PATHS: [&str; 2] = ["/api/", "/registrar/"];

/// Both rustls providers end up compiled in through dependencies, and then rustls refuses to pick one on its own.
pub fn ensure_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

/// Page-only: websocket exchanges, tokens spliced into bodies, and app-origin calls that may rely on its cookies.
fn eligible(request: &Request) -> bool {
    request.signalr.is_none()
        && request.augloop.is_none()
        && request.body_tokens.is_empty()
        && request.url.starts_with("https://")
        && !needs_page_cookies(&request.url)
}

fn needs_page_cookies(url: &str) -> bool {
    App::ALL.iter().any(|&app| {
        app.origins().iter().any(|origin| {
            same_origin(url, origin)
                && !(app == App::Teams && TEAMS_BEARER_PATHS.iter().any(|path| url[origin.len()..].starts_with(path)))
        })
    })
}

fn same_origin(url: &str, origin: &str) -> bool {
    url.strip_prefix(origin).is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
}

fn method(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
    }
}

fn expires_at(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?).ok()?;
    claims.get("exp")?.as_u64()
}

fn usable_until(token: &str, margin: Duration) -> Option<Instant> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let left = expires_at(token)?.checked_sub(now)?.checked_sub(margin.as_secs())?;
    Some(Instant::now() + Duration::from_secs(left))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_expiring_at(exp: u64) -> String {
        format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(format!("{{\"exp\":{exp}}}")))
    }

    #[test]
    fn a_token_is_used_until_the_margin_before_it_expires() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let until = usable_until(&token_expiring_at(now + 3600), Duration::from_secs(120)).unwrap();
        let left = until.saturating_duration_since(Instant::now()).as_secs();
        assert!((3470..=3480).contains(&left), "{left}");
        assert!(usable_until(&token_expiring_at(now + 60), Duration::from_secs(120)).is_none());
        assert!(usable_until("not-a-jwt", Duration::from_secs(120)).is_none());
    }

    #[tokio::test]
    async fn a_bodyless_delete_says_its_length_is_zero() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/messages/1?behavior=softDelete", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = vec![0; 4096];
            let read = socket.read(&mut head).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await.unwrap();
            String::from_utf8_lossy(&head[..read]).to_lowercase()
        });
        let http = NativeHttp::new(Duration::from_secs(5), 1024).unwrap();
        let answer = http.exchange(&Request::delete(&url), None).await.unwrap();
        assert_eq!(answer.status, 200);
        assert!(server.await.unwrap().contains("content-length: 0"));
    }

    #[test]
    fn only_plain_cross_origin_requests_leave_the_page() {
        assert!(eligible(&Request::get("https://graph.microsoft.com/v1.0/me")));
        assert!(eligible(&Request::anonymous_binary_get("https://media.giphy.com/x.gif", Vec::new())));
        assert!(eligible(&Request::get("https://teams.cloud.microsoft/api/mt/emea/beta/users")));
        assert!(!eligible(&Request::get("https://teams.cloud.microsoft/v2/")));
        assert!(!eligible(&Request::get("https://outlook.office.com/api/v2.0/me/messages")));
        assert!(!eligible(&Request::get("http://graph.microsoft.com/v1.0/me")));
        let spliced = Request::with_body(Method::Post, "https://x.example/", json!({"t": "@@t@@"})).with_body_token("https://r", "s", "@@t@@");
        assert!(!eligible(&spliced));
    }
}
