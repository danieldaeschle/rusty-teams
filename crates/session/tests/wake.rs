use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use session::{Method, Scope, Session, SessionConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

type Calls = Arc<Mutex<Vec<(String, Value)>>>;
type Script = Arc<Mutex<VecDeque<Value>>>;

async fn start_browser(tab_url: &'static str, outcomes: Vec<Value>) -> (String, Calls) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let script: Script = Arc::new(Mutex::new(outcomes.into()));
    let seen = calls.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(serve(stream, port, tab_url, script.clone(), seen.clone()));
        }
    });
    (format!("http://127.0.0.1:{port}"), calls)
}

async fn serve(mut stream: TcpStream, port: u16, tab_url: &'static str, script: Script, calls: Calls) {
    let mut peek = [0u8; 16];
    let count = stream.peek(&mut peek).await.unwrap();
    if !peek[..count].starts_with(b"GET /json") {
        let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(Message::Text(text))) = websocket.next().await {
            let call: Value = serde_json::from_str(text.as_str()).unwrap();
            let method = call["method"].as_str().unwrap().to_owned();
            calls.lock().unwrap().push((method.clone(), call["params"].clone()));
            let result = if method == "Runtime.evaluate" {
                let outcome = script.lock().unwrap().pop_front().expect("script exhausted");
                json!({"result": {"type": "object", "value": outcome}})
            } else {
                json!({})
            };
            websocket
                .send(Message::text(json!({"id": call["id"], "result": result}).to_string()))
                .await
                .unwrap();
        }
        return;
    }
    let mut discard = [0u8; 1024];
    let _ = stream.read(&mut discard).await.unwrap();
    let targets = json!([{"type": "page", "url": tab_url, "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/0")}]);
    let body = targets.to_string();
    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(response.as_bytes()).await.unwrap();
}

fn fast_config() -> SessionConfig {
    SessionConfig {
        token_wait: Duration::from_secs(2),
        token_poll: Duration::from_millis(20),
        ..SessionConfig::default()
    }
}

async fn navigations(tab_url: &'static str) -> Vec<(String, Value)> {
    let missing = json!({"noToken": true, "refreshError": "invalid_grant"});
    let ok = json!({"results": [{"status": 200, "body": {"ok": true}}]});
    let (endpoint, calls) = start_browser(tab_url, vec![missing, ok]).await;
    let session = Session::connect_with(&endpoint, fast_config()).await.unwrap();
    session
        .request(Method::Get, "https://graph.microsoft.com/v1.0/me", &Scope::graph("User.Read"), None)
        .await
        .unwrap();
    let recorded = calls.lock().unwrap().clone();
    recorded
        .into_iter()
        .filter(|(method, _)| method == "Page.navigate" || method == "Page.reload")
        .collect()
}

#[tokio::test]
async fn parked_tab_is_woken_by_navigating_to_the_start_url() {
    let wake = navigations("https://teams.cloud.microsoft/robots.txt").await;
    assert_eq!(wake.len(), 1);
    assert_eq!(wake[0].0, "Page.navigate");
    assert_eq!(wake[0].1["url"], "https://teams.cloud.microsoft/");
}

#[tokio::test]
async fn live_app_tab_is_reloaded() {
    let wake = navigations("https://teams.cloud.microsoft/v2/").await;
    assert_eq!(wake.len(), 1);
    assert_eq!(wake[0].0, "Page.reload");
}
