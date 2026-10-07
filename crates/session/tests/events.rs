use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use session::{App, Error, Scope, Session};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct MockTab {
    endpoint: String,
    calls: Calls,
}

async fn start_tab(evaluate_value: Value) -> MockTab {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(serve(stream, port, evaluate_value.clone(), seen.clone()));
        }
    });
    MockTab {
        endpoint: format!("http://127.0.0.1:{port}"),
        calls,
    }
}

async fn serve(mut stream: TcpStream, port: u16, evaluate_value: Value, calls: Calls) {
    let mut peek = [0u8; 16];
    let count = stream.peek(&mut peek).await.unwrap();
    if !peek[..count].starts_with(b"GET /json") {
        let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(Message::Text(text))) = websocket.next().await {
            let call: Value = serde_json::from_str(text.as_str()).unwrap();
            let method = call["method"].as_str().unwrap().to_owned();
            calls.lock().unwrap().push((method.clone(), call["params"].clone()));
            let result = match method.as_str() {
                "Runtime.evaluate" => json!({"result": {"type": "object", "value": evaluate_value}}),
                "Page.addScriptToEvaluateOnNewDocument" => json!({"identifier": "7"}),
                "Runtime.disable" => {
                    websocket.send(Message::text(json!({"id": call["id"], "error": {"message": "nope"}}).to_string())).await.unwrap();
                    continue;
                }
                _ => json!({}),
            };
            websocket
                .send(Message::text(json!({"id": call["id"], "result": result}).to_string()))
                .await
                .unwrap();
            if method == "Runtime.addBinding" {
                let name = call["params"]["name"].clone();
                for payload in ["first", "second"] {
                    let event = json!({"method": "Runtime.bindingCalled", "params": {"name": name, "payload": payload, "executionContextId": 1}});
                    websocket.send(Message::text(event.to_string())).await.unwrap();
                }
                let navigation = json!({"method": "Page.frameNavigated", "params": {"frame": {"url": "https://teams.cloud.microsoft/"}}});
                websocket.send(Message::text(navigation.to_string())).await.unwrap();
            }
        }
        return;
    }
    let mut discard = [0u8; 1024];
    let _ = stream.read(&mut discard).await.unwrap();
    let targets = json!([{"type": "page", "url": "https://teams.cloud.microsoft/v2/", "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/0")}]);
    let body = targets.to_string();
    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(response.as_bytes()).await.unwrap();
}

#[tokio::test]
async fn binding_calls_arrive_as_events_in_order() {
    let tab = start_tab(Value::Null).await;
    let session = Session::connect(&tab.endpoint).await.unwrap();
    let (control, mut events) = session.subscribe(App::Teams).await.unwrap();
    control.enable_events().await.unwrap();
    control.add_binding("sink").await.unwrap();

    let first = events.recv().await.unwrap();
    assert_eq!(first.binding_payload("sink"), Some("first"));
    assert_eq!(events.recv().await.unwrap().binding_payload("sink"), Some("second"));
    assert_eq!(events.recv().await.unwrap().method, "Page.frameNavigated");

    let methods: Vec<String> = tab.calls.lock().unwrap().iter().map(|(method, _)| method.clone()).collect();
    assert_eq!(methods, ["Runtime.enable", "Page.enable", "Runtime.addBinding"]);
}

#[tokio::test]
async fn inject_script_registers_for_new_documents_and_runs_now() {
    let tab = start_tab(json!("done")).await;
    let session = Session::connect(&tab.endpoint).await.unwrap();
    let (control, _events) = session.subscribe(App::Teams).await.unwrap();
    let identifier = control.inject_script("window.__x = 1").await.unwrap();
    assert_eq!(identifier, "7");
    let calls = tab.calls.lock().unwrap();
    assert_eq!(calls[0].0, "Page.addScriptToEvaluateOnNewDocument");
    assert_eq!(calls[0].1["source"], "window.__x = 1");
    assert_eq!(calls[1].0, "Runtime.evaluate");
    assert_eq!(calls[1].1["expression"], "window.__x = 1");
}

#[tokio::test]
async fn concurrent_calls_get_their_own_answers_and_errors_are_typed() {
    let tab = start_tab(Value::Null).await;
    let session = Session::connect(&tab.endpoint).await.unwrap();
    let (control, _events) = session.subscribe(App::Teams).await.unwrap();
    let (identifier, failure) = tokio::join!(
        control.call("Page.addScriptToEvaluateOnNewDocument", json!({"source": ""})),
        control.call("Runtime.disable", json!({})),
    );
    assert_eq!(identifier.unwrap()["identifier"], "7");
    assert!(matches!(failure.unwrap_err(), Error::Cdp(reason) if reason == "nope"));
}

#[tokio::test]
async fn events_survive_dropping_the_control() {
    let tab = start_tab(Value::Null).await;
    let session = Session::connect(&tab.endpoint).await.unwrap();
    let (control, mut events) = session.subscribe(App::Teams).await.unwrap();
    control.add_binding("sink").await.unwrap();
    drop(control);
    let mut received = 0;
    while let Some(_event) = events.next().await {
        received += 1;
        if received == 3 {
            break;
        }
    }
    assert_eq!(received, 3);
}

#[tokio::test]
async fn run_with_token_splices_body_after_the_token_lookup() {
    let tab = start_tab(json!({"ok": true})).await;
    let session = Session::connect(&tab.endpoint).await.unwrap();
    let scope = Scope::new("https://ic3.teams.office.com", "Teams.AccessAsUser.All");
    let answer = session
        .run_with_token(App::Teams, &scope, "return {ok: !!token};", &json!({"host": "h"}), true)
        .await
        .unwrap();
    assert_eq!(answer["ok"], true);
    let calls = tab.calls.lock().unwrap();
    let expression = calls[0].1["expression"].as_str().unwrap();
    let lookup = expression.find("if (!token) return {noToken").unwrap();
    let body = expression.find("return {ok: !!token};").unwrap();
    assert!(lookup < body);
    assert!(expression.contains(r#"const args = {"host":"h"};"#));
    assert!(expression.contains(r#""forceRefresh":true"#));
    assert!(expression.contains(r#""scope":"Teams.AccessAsUser.All""#));
}

#[tokio::test]
async fn run_with_token_maps_missing_token_to_typed_errors() {
    let tab = start_tab(json!({"noToken": true, "refreshError": "invalid_grant"})).await;
    let session = Session::connect(&tab.endpoint).await.unwrap();
    let scope = Scope::new("https://ic3.teams.office.com", "Teams.AccessAsUser.All");
    let error = session
        .run_with_token(App::Teams, &scope, "return 1;", &json!({}), false)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NoFreshToken { .. }), "{error}");

    let tab = start_tab(json!({"noToken": true, "refreshError": "the app is not granted x"})).await;
    let session = Session::connect(&tab.endpoint).await.unwrap();
    let error = session
        .run_with_token(App::Teams, &scope, "return 1;", &json!({}), false)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::LoginRequired(_)), "{error}");
}
