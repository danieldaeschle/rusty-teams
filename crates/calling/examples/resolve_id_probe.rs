use calling::{CallEngine, EngineConfig};
use chatsvc::InstanceNames;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, Method, Scope, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let poll_session = Session::connect(&endpoint).await.expect("browser");
    let scope = Scope::graph("OnlineMeetings.ReadWrite");
    let start = Utc::now() + Duration::hours(2);
    let body = json!({
        "subject": "Native client test",
        "startDateTime": start.to_rfc3339(),
        "endDateTime": (start + Duration::minutes(30)).to_rfc3339(),
    });
    let created = session
        .request(Method::Post, "https://graph.microsoft.com/v1.0/me/onlineMeetings", &scope, Some(body))
        .await
        .expect("create");
    println!("# create online meeting: HTTP {}", created.status);
    let id = created.body.get("id").and_then(Value::as_str).map(str::to_owned);
    let code = created.body.pointer("/joinMeetingIdSettings/joinMeetingId").and_then(Value::as_str).map(|code| code.replace(' ', ""));
    let passcode = created.body.pointer("/joinMeetingIdSettings/passcode").and_then(Value::as_str).map(str::to_owned);
    let thread = created.body.pointer("/chatInfo/threadId").and_then(Value::as_str).map(str::to_owned);
    println!("# meeting id found {}, passcode found {}, thread found {}", code.is_some(), passcode.is_some(), thread.is_some());
    if let Some(code) = &code {
        let config = EngineConfig {
            ringable: false,
            instance: InstanceNames {
                global: "__resolveIdTrouter".into(),
                binding: "__resolveIdRealtime".into(),
                endpoint_storage_key: "__resolveIdEpid".into(),
            },
            ..EngineConfig::default()
        };
        let (engine, _events) = CallEngine::start(session.clone(), poll_session, config).await.expect("engine");
        let url = match &passcode {
            Some(passcode) => format!("https://teams.microsoft.com/meet/{code}?p={passcode}"),
            None => format!("https://teams.microsoft.com/meet/{code}"),
        };
        let meeting_data = json!({"meetingCode": code, "passcode": passcode, "meetingUrl": url});
        match engine.resolve_meeting(&meeting_data).await {
            Ok(target) => println!(
                "# resolve by id: found, same thread {}, tenant and organizer present {}",
                thread.as_deref() == Some(target.thread_id.as_str()),
                !target.tenant_id.is_empty() && !target.organizer_id.is_empty()
            ),
            Err(error) => println!("# resolve by id: failed: {}", error.to_string().chars().take(200).collect::<String>()),
        }
        engine.stop().await;
    }
    if let Some(id) = id {
        let deleted = session
            .request(Method::Delete, &format!("https://graph.microsoft.com/v1.0/me/onlineMeetings/{id}"), &scope, None)
            .await
            .expect("delete");
        println!("# delete online meeting: HTTP {}", deleted.status);
    }
}
