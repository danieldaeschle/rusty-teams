use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use session::{Error, Method, Request, Scope, Session, SessionConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

type Script = Arc<Mutex<VecDeque<Value>>>;

struct MockBrowser {
    endpoint: String,
    expressions: Arc<Mutex<Vec<String>>>,
}

async fn start_browser(outcomes: Vec<Value>, tab_urls: Vec<&'static str>) -> MockBrowser {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let script: Script = Arc::new(Mutex::new(outcomes.into()));
    let expressions = Arc::new(Mutex::new(Vec::new()));
    let seen = expressions.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(serve(stream, port, tab_urls.clone(), script.clone(), seen.clone()));
        }
    });
    MockBrowser {
        endpoint: format!("http://127.0.0.1:{port}"),
        expressions,
    }
}

async fn serve(mut stream: TcpStream, port: u16, tab_urls: Vec<&'static str>, script: Script, seen: Arc<Mutex<Vec<String>>>) {
    let mut peek = [0u8; 16];
    let count = stream.peek(&mut peek).await.unwrap();
    if !peek[..count].starts_with(b"GET /json") {
        let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(Message::Text(text))) = websocket.next().await {
            let call: Value = serde_json::from_str(text.as_str()).unwrap();
            let result = match call["method"].as_str().unwrap() {
                "Runtime.evaluate" => {
                    seen.lock().unwrap().push(call["params"]["expression"].as_str().unwrap().to_owned());
                    let outcome = script.lock().unwrap().pop_front().expect("script exhausted");
                    json!({"result": {"type": "object", "value": outcome}})
                }
                _ => json!({}),
            };
            let answer = json!({"id": call["id"], "result": result});
            websocket.send(Message::text(answer.to_string())).await.unwrap();
        }
        return;
    }
    let mut discard = [0u8; 1024];
    let _ = stream.read(&mut discard).await.unwrap();
    let targets: Vec<Value> = tab_urls
        .iter()
        .enumerate()
        .map(|(index, url)| json!({"type": "page", "url": url, "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/{index}")}))
        .collect();
    let body = serde_json::to_string(&targets).unwrap();
    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(response.as_bytes()).await.unwrap();
}

fn fast_config() -> SessionConfig {
    SessionConfig {
        token_wait: Duration::from_millis(100),
        token_poll: Duration::from_millis(20),
        default_retry_wait: Duration::from_millis(10),
        unauthorized_reload_wait: Duration::from_millis(10),
        ..SessionConfig::default()
    }
}

const BOTH_TABS: [&str; 2] = ["https://teams.cloud.microsoft/v2/", "https://outlook.cloud.microsoft/mail/"];

#[tokio::test]
async fn request_returns_body() {
    let browser = start_browser(vec![json!({"results": [{"status": 200, "body": {"id": "u1"}}]})], BOTH_TABS.to_vec()).await;
    let session = Session::connect_with(&browser.endpoint, fast_config()).await.unwrap();
    let response = session
        .request(Method::Get, "https://graph.microsoft.com/v1.0/me", &Scope::graph("User.Read"), None)
        .await
        .unwrap();
    assert_eq!(response.body["id"], "u1");
    let expressions = browser.expressions.lock().unwrap();
    assert!(expressions[0].contains("\"scope\":\"User.Read\""));
    assert!(expressions[0].contains("\"forceRefresh\":false"));
}

#[tokio::test]
async fn throttled_request_is_retried_after_retry_after() {
    let browser = start_browser(
        vec![
            json!({"results": [{"status": 429, "retryAfter": "0", "body": "slow down"}]}),
            json!({"results": [{"status": 200, "body": {"ok": true}}]}),
        ],
        BOTH_TABS.to_vec(),
    )
    .await;
    let session = Session::connect_with(&browser.endpoint, fast_config()).await.unwrap();
    let response = session
        .request(Method::Get, "https://graph.microsoft.com/v1.0/me/chats", &Scope::graph("Chat.Read"), None)
        .await
        .unwrap();
    assert_eq!(response.body["ok"], true);
    assert_eq!(browser.expressions.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn batch_retries_only_throttled_requests() {
    let browser = start_browser(
        vec![
            json!({"results": [{"status": 200, "body": "a"}, {"status": 429, "retryAfter": "0", "body": null}]}),
            json!({"results": [{"status": 200, "body": "b"}]}),
        ],
        BOTH_TABS.to_vec(),
    )
    .await;
    let session = Session::connect_with(&browser.endpoint, fast_config()).await.unwrap();
    let requests = [Request::get("https://graph.microsoft.com/v1.0/a"), Request::get("https://graph.microsoft.com/v1.0/b")];
    let responses = session.batch(&requests, &Scope::graph("Chat.Read")).await.unwrap();
    assert_eq!(responses[0].body, "a");
    assert_eq!(responses[1].body, "b");
}

#[tokio::test]
async fn api_error_is_typed() {
    let browser = start_browser(
        vec![json!({"results": [{"status": 404, "body": {"error": {"message": "gone"}}}]})],
        BOTH_TABS.to_vec(),
    )
    .await;
    let session = Session::connect_with(&browser.endpoint, fast_config()).await.unwrap();
    let error = session
        .request(Method::Get, "https://graph.microsoft.com/v1.0/chats/x", &Scope::graph("Chat.Read"), None)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Api { status: 404, .. }));
}

#[tokio::test]
async fn ungranted_scope_in_both_tabs_is_login_required() {
    let denied = json!({"noToken": true, "refreshError": "the app is not granted Chat.Read"});
    let browser = start_browser(vec![denied.clone(), denied], BOTH_TABS.to_vec()).await;
    let session = Session::connect_with(&browser.endpoint, fast_config()).await.unwrap();
    let error = session
        .request(Method::Get, "https://graph.microsoft.com/v1.0/me/chats", &Scope::graph("Chat.Read"), None)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::LoginRequired(_)));
}

#[tokio::test]
async fn missing_token_without_login_tab_is_no_fresh_token() {
    let missing = json!({"noToken": true, "refreshError": "invalid_grant"});
    let browser = start_browser(vec![missing; 40], BOTH_TABS.to_vec()).await;
    let session = Session::connect_with(&browser.endpoint, fast_config()).await.unwrap();
    let error = session
        .request(Method::Get, "https://graph.microsoft.com/v1.0/me", &Scope::graph("User.Read"), None)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NoFreshToken { .. }), "{error}");
}

#[tokio::test]
async fn missing_token_with_login_tab_is_login_required() {
    let missing = json!({"noToken": true, "refreshError": "invalid_grant"});
    let tabs = vec!["https://teams.cloud.microsoft/v2/", "https://login.microsoftonline.com/common/oauth2/authorize"];
    let browser = start_browser(vec![missing; 40], tabs).await;
    let session = Session::connect_with(&browser.endpoint, fast_config()).await.unwrap();
    let error = session
        .request(Method::Get, "https://graph.microsoft.com/v1.0/me", &Scope::graph("User.Read"), None)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::LoginRequired(_)), "{error}");
}

#[tokio::test]
async fn no_browser_is_typed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let error = Session::connect(&format!("http://127.0.0.1:{port}")).await.err().unwrap();
    assert!(matches!(error, Error::NoBrowser { .. }));
}
