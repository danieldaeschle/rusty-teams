use std::sync::Arc;

use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};
use store::Store;
use teams_core::{ReceiptState, SyncEngine};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let engine = SyncEngine::new(
        Graph::new(session),
        Arc::new(Store::open_in_memory().unwrap()),
    );
    engine.refresh_sidebar().await.expect("sidebar");
    let me = engine.me().expect("me").user_id;
    let chats = engine.sidebar().unwrap().chats;
    let chat = chats
        .iter()
        .find(|chat| chat.kind == "oneOnOne")
        .expect("a 1:1 chat");
    engine.fetch_newer(&chat.id).await.expect("messages");
    engine.refresh_receipts(&chat.id).await.expect("receipts");
    let (mut sent, mut read, mut unknown, mut own) = (0, 0, 0, 0);
    for record in engine.open_conversation(&chat.id).unwrap() {
        if record.sender_id.as_deref() == Some(me.as_str()) {
            own += 1;
        }
        match engine.receipt_state_for(&record) {
            ReceiptState::Sent => sent += 1,
            ReceiptState::Read { .. } => read += 1,
            ReceiptState::Unknown => unknown += 1,
        }
    }
    println!("own messages {own}: sent {sent}, read {read}, other or unknown {unknown}");
}
