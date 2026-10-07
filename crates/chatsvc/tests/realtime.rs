use std::sync::{Arc, Mutex};
use std::time::Duration;

use chatsvc::{EventKind, Realtime, RealtimeConfig, RealtimeEvent, StatusKind};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use session::Session;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

async fn start_tab() -> (String, Calls) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(serve(stream, port, seen.clone()));
        }
    });
    (format!("http://127.0.0.1:{port}"), calls)
}

fn binding_event(payload: Value) -> Message {
    let event = json!({"method": "Runtime.bindingCalled", "params": {"name": "__chatsvcRealtime", "payload": payload.to_string()}});
    Message::text(event.to_string())
}

async fn serve(mut stream: TcpStream, port: u16, calls: Calls) {
    let mut peek = [0u8; 16];
    let count = stream.peek(&mut peek).await.unwrap();
    if !peek[..count].starts_with(b"GET /json") {
        let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(Message::Text(text))) = websocket.next().await {
            let call: Value = serde_json::from_str(text.as_str()).unwrap();
            let method = call["method"].as_str().unwrap().to_owned();
            calls
                .lock()
                .unwrap()
                .push((method.clone(), call["params"].clone()));
            let result = match method.as_str() {
                "Runtime.evaluate" => json!({"result": {"type": "string", "value": "ready"}}),
                "Page.addScriptToEvaluateOnNewDocument" => json!({"identifier": "9"}),
                _ => json!({}),
            };
            websocket
                .send(Message::text(
                    json!({"id": call["id"], "result": result}).to_string(),
                ))
                .await
                .unwrap();
            if method == "Runtime.addBinding" {
                let payloads = [
                    json!({"channel": "status", "kind": "connected", "detail": ""}),
                    json!({"channel": "event", "resourceType": "NewMessage", "eventKind": "new_message", "conversationId": "19:abc@thread.v2", "messageId": "1", "receivedAt": 1791368146999i64}),
                    json!({"channel": "event", "resourceType": "NewMessage", "eventKind": "typing", "conversationId": null, "messageId": null, "receivedAt": 1791368147000i64}),
                ];
                for payload in payloads {
                    websocket.send(binding_event(payload)).await.unwrap();
                }
                websocket.send(Message::text(json!({"method": "Runtime.bindingCalled", "params": {"name": "someoneElse", "payload": "x"}}).to_string())).await.unwrap();
                websocket.send(Message::text(json!({"method": "Runtime.bindingCalled", "params": {"name": "__chatsvcRealtime", "payload": "garbage"}}).to_string())).await.unwrap();
            }
        }
        return;
    }
    let mut discard = [0u8; 1024];
    let _ = stream.read(&mut discard).await.unwrap();
    let targets = json!([{"type": "page", "url": "https://teams.cloud.microsoft/v2/", "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/0")}]);
    let body = targets.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await.unwrap();
}

fn quiet_config() -> RealtimeConfig {
    RealtimeConfig {
        ensure_interval: Duration::from_secs(3600),
        ..RealtimeConfig::default()
    }
}

#[tokio::test]
async fn forwards_decoded_events_and_drops_foreign_or_garbage_payloads() {
    let (endpoint, calls) = start_tab().await;
    let session = Session::connect(&endpoint).await.unwrap();
    let mut realtime = Realtime::start_with(&session, quiet_config())
        .await
        .unwrap();

    let RealtimeEvent::Status(status) = realtime.recv().await.unwrap() else {
        panic!("expected status first")
    };
    assert_eq!(status.kind, StatusKind::Connected);
    let RealtimeEvent::Message(message) = realtime.recv().await.unwrap() else {
        panic!("expected message")
    };
    assert_eq!(message.kind, EventKind::NewMessage);
    assert_eq!(message.conversation_id.as_deref(), Some("19:abc@thread.v2"));
    let RealtimeEvent::Message(typing) = realtime.recv().await.unwrap() else {
        panic!("expected typing")
    };
    assert_eq!(typing.kind, EventKind::Typing);
    assert_eq!(typing.conversation_id, None);

    let methods: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(method, _)| method.clone())
        .filter(|method| method != "Runtime.evaluate")
        .collect();
    assert_eq!(
        &methods[..4],
        [
            "Runtime.enable",
            "Page.enable",
            "Runtime.addBinding",
            "Page.addScriptToEvaluateOnNewDocument"
        ]
    );
    realtime.stop().await;
}

#[tokio::test]
async fn start_injects_worker_and_ensures_with_default_host() {
    let (endpoint, calls) = start_tab().await;
    let session = Session::connect(&endpoint).await.unwrap();
    let realtime = Realtime::start_with(&session, quiet_config())
        .await
        .unwrap();
    assert_eq!(realtime.host(), "go-eu.trouter.teams.microsoft.com");
    let recorded = calls.lock().unwrap().clone();
    let expressions: Vec<&str> = recorded
        .iter()
        .filter(|(method, _)| method == "Runtime.evaluate")
        .filter_map(|(_, params)| params["expression"].as_str())
        .collect();
    assert!(
        expressions
            .iter()
            .any(|expression| expression.contains("window.__chatsvcTrouter.ensure("))
    );
    assert!(
        expressions
            .iter()
            .all(|expression| !expression.contains("Bearer ey"))
    );
    let ensure = expressions
        .iter()
        .find(|expression| expression.contains("window.__chatsvcTrouter.ensure("))
        .unwrap();
    let arguments_json = ensure
        .split_once("const args = ")
        .and_then(|(_, rest)| rest.split_once(";\n").or_else(|| rest.split_once(';')))
        .map(|(json, _)| json)
        .unwrap();
    let arguments: serde_json::Value = serde_json::from_str(arguments_json).unwrap();
    assert_eq!(
        arguments,
        serde_json::json!({"forwardPresence": false, "host": "go-eu.trouter.teams.microsoft.com"})
    );
    realtime.stop().await;
}

#[tokio::test]
async fn stop_unregisters_in_the_page_and_removes_the_injected_script() {
    let (endpoint, calls) = start_tab().await;
    let session = Session::connect(&endpoint).await.unwrap();
    let realtime = Realtime::start_with(&session, quiet_config())
        .await
        .unwrap();
    realtime.stop().await;
    let recorded = calls.lock().unwrap().clone();
    let stop_call = recorded.iter().rposition(|(method, params)| {
        method == "Runtime.evaluate"
            && params["expression"].as_str()
                == Some("window.__chatsvcTrouter ? window.__chatsvcTrouter.stop() : null")
    });
    let remove_call = recorded.iter().rposition(|(method, params)| {
        method == "Page.removeScriptToEvaluateOnNewDocument" && params["identifier"] == "9"
    });
    assert!(stop_call.is_some() && remove_call.is_some());
    assert!(stop_call < remove_call);
}
