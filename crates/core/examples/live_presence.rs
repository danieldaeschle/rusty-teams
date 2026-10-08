use std::sync::Arc;
use std::time::{Duration, Instant};

use chatsvc::{Realtime, RealtimeEvent};
use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};
use store::Store;
use teams_core::SyncEngine;

#[tokio::main]
async fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(120);
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let engine = SyncEngine::new(
        Graph::new(session.clone()),
        Arc::new(Store::open_in_memory().unwrap()),
    );
    engine.refresh_sidebar().await.expect("sidebar");
    let mut user_ids: Vec<String> = engine
        .sidebar()
        .unwrap()
        .chats
        .iter()
        .flat_map(|chat| {
            chat.members
                .iter()
                .filter_map(|member| member.user_id.clone())
        })
        .collect();
    user_ids.sort();
    user_ids.dedup();
    engine.watch_presence(&user_ids).await.expect("watch");
    println!("# watching {} users for {seconds}s", user_ids.len());
    let mut realtime = Realtime::start(&session).await.expect("realtime start");
    let started = Instant::now();
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            event = realtime.recv() => {
                let Some(event) = event else { break };
                let at = started.elapsed().as_secs_f32();
                match event {
                    RealtimeEvent::Endpoint(endpoint) => {
                        let outcome = engine.presence_endpoint(endpoint).await;
                        println!("{at:6.1}s endpoint subscribe {:?}", outcome.map_err(|error| error.to_string()));
                    }
                    RealtimeEvent::Presence(updates) => {
                        engine.apply_presence(&updates);
                        let known = user_ids.iter().filter(|id| engine.presence(id).is_some()).count();
                        println!("{at:6.1}s presence push {} entries, known {known}/{}", updates.len(), user_ids.len());
                    }
                    RealtimeEvent::Status(status) => println!("{at:6.1}s status {:?} {}", status.kind, status.detail),
                    RealtimeEvent::Message(_) | RealtimeEvent::Typing(_) => {}
                }
            }
            _ = &mut deadline => break,
        }
    }
    realtime.stop().await;
}
