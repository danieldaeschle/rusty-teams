use std::time::Duration;

use chatsvc::{EventKind, Realtime, RealtimeEvent, mask_conversation_id};
use chrono::Local;
use session::{DEFAULT_ENDPOINT, Session};

#[tokio::main]
async fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let mut realtime = Realtime::start(&session).await.expect("realtime start");
    println!("# started host={} for {seconds}s", realtime.host());
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            event = realtime.recv() => {
                let Some(event) = event else { break };
                println!("{}", describe(&event));
            }
            _ = &mut deadline => break,
        }
    }
    realtime.stop().await;
    println!("# stopped");
}

fn describe(event: &RealtimeEvent) -> String {
    match event {
        RealtimeEvent::Message(message) => format!(
            "{} {} {} conv={}",
            message
                .received_at
                .with_timezone(&Local)
                .format("%H:%M:%S%.3f"),
            kind_name(message.kind),
            message.resource_type,
            message
                .conversation_id
                .as_deref()
                .map(mask_conversation_id)
                .unwrap_or_else(|| "-".into()),
        ),
        RealtimeEvent::Typing(typing) => format!(
            "{} typing {} conv={}",
            typing
                .received_at
                .with_timezone(&Local)
                .format("%H:%M:%S%.3f"),
            if typing.active { "start" } else { "clear" },
            mask_conversation_id(&typing.conversation_id),
        ),
        RealtimeEvent::Status(status) => format!("# status {:?} {}", status.kind, status.detail),
        RealtimeEvent::Presence(updates) => format!("# presence updates={}", updates.len()),
        RealtimeEvent::Endpoint(endpoint) => format!(
            "# endpoint host={}",
            endpoint
                .trouter_uri
                .trim_start_matches("https://")
                .split('/')
                .next()
                .unwrap_or("-")
        ),
    }
}

fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::NewMessage => "new_message",
        EventKind::MessageUpdate => "message_update",
        EventKind::ThreadUpdate => "thread_update",
        EventKind::Typing => "typing",
        EventKind::ReadReceipt => "read_receipt",
        EventKind::ThreadActivity => "thread_activity",
        EventKind::Control => "control",
        EventKind::Other => "other",
    }
}
