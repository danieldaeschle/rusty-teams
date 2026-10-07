use std::path::PathBuf;
use std::time::{Duration, Instant};

use session::{App, Method, Scope, Session, SessionConfig};

#[tokio::main]
async fn main() {
    let folder = std::env::args().nth(1).map(PathBuf::from).expect("usage: probe <user-data-folder> [hold-seconds]");
    let hold = std::env::args().nth(2).and_then(|value| value.parse().ok()).unwrap_or(0);
    let started = Instant::now();
    let transport = webview::start(webview::HostConfig {
        user_data_folder: folder,
        window_title: "Rusty Teams probe - Sign in".into(),
    });
    let session = match Session::with_transport(transport, SessionConfig::default()).await {
        Ok(session) => session,
        Err(error) => return println!("connect failed after {:?}: {error}", started.elapsed()),
    };
    println!("webviews ready after {:?}", started.elapsed());
    graph_calls(&session, "loaded").await;
    presence_call(&session, "loaded").await;
    match session.subscribe(App::Teams).await {
        Ok((control, _events)) => match control.enable_events().await {
            Ok(()) => println!("teams events: enabled"),
            Err(error) => println!("teams events: {error}"),
        },
        Err(error) => println!("teams subscribe: {error}"),
    }
    println!("holding {hold} s");
    tokio::time::sleep(Duration::from_secs(hold)).await;
    graph_calls(&session, "after hold").await;
    presence_call(&session, "after hold").await;
}

async fn presence_call(session: &Session, phase: &str) {
    let me = match session.request(Method::Get, "https://graph.microsoft.com/v1.0/me?$select=id", &Scope::graph("User.Read"), None).await {
        Ok(response) => response.body["id"].as_str().unwrap_or_default().to_owned(),
        Err(error) => return println!("{phase} presence: no own id: {error}"),
    };
    let call = Instant::now();
    let body = serde_json::json!([{"mri": format!("8:orgid:{me}")}]);
    let scope = Scope::new(session::PRESENCE, "user_impersonation");
    match session.request(Method::Post, "https://presence.teams.microsoft.com/v1/presence/getpresence/", &scope, Some(body)).await {
        Ok(response) => println!(
            "{phase} presence: HTTP {} after {:?}, availability present: {}",
            response.status,
            call.elapsed(),
            response.body.pointer("/0/presence/availability").is_some()
        ),
        Err(error) => println!("{phase} presence: failed after {:?}: {}", call.elapsed(), error.to_string().chars().take(200).collect::<String>()),
    }
}

async fn graph_calls(session: &Session, phase: &str) {
    for (label, url, scope) in [
        ("graph me", "https://graph.microsoft.com/v1.0/me?$select=id", Scope::graph("User.Read")),
        ("graph chats", "https://graph.microsoft.com/v1.0/me/chats?$top=1&$select=id", Scope::graph("Chat.Read")),
    ] {
        let call = Instant::now();
        match session.request(Method::Get, url, &scope, None).await {
            Ok(response) => println!("{phase} {label}: HTTP {} after {:?}", response.status, call.elapsed()),
            Err(error) => println!("{phase} {label}: failed after {:?}: {}", call.elapsed(), error.to_string().chars().take(160).collect::<String>()),
        }
    }
}
