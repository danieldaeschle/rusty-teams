use std::time::Duration;

use chatsvc::{PresenceService, Realtime, RealtimeConfig, RealtimeEvent, StatusKind};
use session::{DEFAULT_ENDPOINT, Session};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const SETTLE: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() {
    let user_id = std::env::args().nth(1).expect("usage: presence_endpoint_probe <own user id GUID> [inactive wait seconds]");
    let inactive_wait = std::env::args()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .map_or(SETTLE, Duration::from_secs);
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let presence = PresenceService::new(&session);

    let config = RealtimeConfig {
        initially_active: true,
        ..RealtimeConfig::default()
    };
    let mut realtime = Realtime::start_with(&session, config).await.expect("realtime start");
    let endpoint_id = wait_for_endpoint(&mut realtime).await;
    println!("# trouter connected");

    println!("register: {}", outcome(presence.register_endpoint(&endpoint_id).await));
    realtime.set_activity(true);
    println!("# sent active");
    tokio::time::sleep(SETTLE).await;
    print_own_presence(&presence, &user_id).await;

    realtime.set_activity(false);
    println!("# sent inactive");
    tokio::time::sleep(inactive_wait).await;
    print_own_presence(&presence, &user_id).await;

    println!("http report inactive: {}", outcome(presence.report_activity(&endpoint_id, false).await));
    println!("unregister: {}", outcome(presence.unregister_endpoint(&endpoint_id).await));
    realtime.stop().await;
    println!("# stopped");
}

async fn wait_for_endpoint(realtime: &mut Realtime) -> String {
    let wait = async {
        let mut endpoint_id = None;
        let mut connected = false;
        while let Some(event) = realtime.recv().await {
            match event {
                RealtimeEvent::Endpoint(endpoint) => endpoint_id = Some(endpoint.endpoint_id),
                RealtimeEvent::Status(status) if status.kind == StatusKind::Connected => connected = true,
                _ => {}
            }
            if connected && let Some(endpoint_id) = endpoint_id.take() {
                return endpoint_id;
            }
        }
        panic!("realtime closed before connecting");
    };
    tokio::time::timeout(CONNECT_TIMEOUT, wait).await.expect("trouter did not connect")
}

async fn print_own_presence(presence: &PresenceService, user_id: &str) {
    match presence.status(user_id).await {
        Ok(status) => println!(
            "own presence: availability={} activity={}",
            status.availability,
            status.activity.as_deref().unwrap_or("-")
        ),
        Err(error) => println!("own presence: failed {}", status_of(&error)),
    }
}

fn outcome(result: chatsvc::Result<()>) -> String {
    match result {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("failed {}", status_of(&error)),
    }
}

fn status_of(error: &chatsvc::Error) -> String {
    match error {
        chatsvc::Error::Session(session::Error::Api { status, .. }) => format!("http {status}"),
        _ => "no http status".to_owned(),
    }
}
