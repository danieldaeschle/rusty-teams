//! Connects the native Trouter client and prints event kinds (no content) for the given seconds.
use std::time::Duration;

use chatsvc::{Realtime, RealtimeConfig, RealtimeEvent};
use session::{DEFAULT_ENDPOINT, Session, SessionConfig};

#[tokio::main]
async fn main() {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect_with(
        &endpoint,
        SessionConfig {
            native_http: std::env::var_os("PAGE_FETCH").is_none(),
            ..SessionConfig::default()
        },
    )
    .await
    .expect("browser");
    let ringable = std::env::var_os("RINGABLE").is_some();
    let config = RealtimeConfig {
        native: true,
        initially_active: true,
        forward_callbacks: ringable,
        ringable,
        ..RealtimeConfig::default()
    };
    let mut realtime = Realtime::start_with(&session, config)
        .await
        .expect("realtime start");
    println!("host {}", realtime.host());
    let watch = async {
        while let Some(event) = realtime.recv().await {
            let label = match &event {
                RealtimeEvent::Status(status) => {
                    format!("status {:?} {}", status.kind, status.detail)
                }
                RealtimeEvent::Endpoint(_) => "endpoint".to_owned(),
                RealtimeEvent::Message(message) => format!("message {:?}", message.kind),
                RealtimeEvent::Presence(entries) => format!("presence x{}", entries.len()),
                RealtimeEvent::Typing(_) => "typing".to_owned(),
            };
            println!("{label}");
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(seconds), watch).await;
    realtime.stop().await;
    println!("stopped");
}
