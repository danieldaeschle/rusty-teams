use std::sync::Arc;
use std::time::Instant;

use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};
use store::Store;
use teams_core::{ChatsvcFolderSource, SyncEngine};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let engine = SyncEngine::new(
        Graph::new(session.clone()),
        Arc::new(Store::open_in_memory().unwrap()),
    )
    .with_folder_source(Arc::new(ChatsvcFolderSource::new(&session)));

    let started = Instant::now();
    let summary = engine.refresh_sidebar().await.expect("sidebar");
    let chats = engine.sidebar().unwrap().chats;
    let with_preview = chats
        .iter()
        .filter(|chat| chat.last_message_preview.is_some())
        .count();
    let with_sender = chats
        .iter()
        .filter(|chat| chat.last_message_sender_name.is_some())
        .count();
    let deleted = chats
        .iter()
        .filter(|chat| chat.last_message_deleted)
        .count();
    println!(
        "sidebar chats {} preview {with_preview} sender {with_sender} deleted {deleted} teams {} in {} ms",
        chats.len(),
        summary.teams,
        started.elapsed().as_millis()
    );
    println!("me known {}", engine.me().is_some());

    let mut user_ids: Vec<String> = chats
        .iter()
        .flat_map(|chat| chat.members.iter())
        .filter_map(|member| member.user_id.clone())
        .collect();
    let members_total: usize = chats.iter().map(|chat| chat.members.len()).sum();
    let members_with_id = user_ids.len();
    user_ids.sort();
    user_ids.dedup();
    println!(
        "members {members_total} with user_id {members_with_id} distinct {}",
        user_ids.len()
    );

    let started = Instant::now();
    engine.fetch_avatars(&user_ids).await.expect("avatars");
    let hits = user_ids
        .iter()
        .filter(|id| engine.avatar(id).is_some())
        .count();
    println!(
        "avatars hits {hits} of {} in {} ms",
        user_ids.len(),
        started.elapsed().as_millis()
    );
    let started = Instant::now();
    engine
        .fetch_avatars(&user_ids)
        .await
        .expect("avatars again");
    println!("second fetch (cached) {} ms", started.elapsed().as_millis());

    engine.refresh_presence(&user_ids).await.expect("presence");
    let known = user_ids
        .iter()
        .filter(|id| engine.presence(id).is_some())
        .count();
    println!("presence known {known} of {}", user_ids.len());

    let started = Instant::now();
    engine.refresh_folders().await.expect("folders");
    let folders = engine.chat_folders().unwrap();
    let items: usize = folders
        .iter()
        .map(|folder| folder.conversation_ids.len())
        .sum();
    println!(
        "folders {} items {items} pinned channels {} in {} ms",
        folders.len(),
        engine.pinned_channels().unwrap().len(),
        started.elapsed().as_millis()
    );
    let unread: u32 = chats.iter().map(|chat| engine.unread_count(&chat.id)).sum();
    println!(
        "unread chats {} total {unread}",
        chats.iter().filter(|chat| chat.unread).count()
    );
}
