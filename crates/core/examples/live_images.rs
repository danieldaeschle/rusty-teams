use std::sync::Arc;
use std::time::Instant;

use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};
use store::Store;
use teams_core::{SyncEngine, images};

const CHATS_TO_SCAN: usize = 30;
const IMAGES_TO_FETCH: usize = 2;

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let engine = SyncEngine::new(
        Graph::new(session),
        Arc::new(Store::open_in_memory().unwrap()),
    );
    engine.refresh_sidebar().await.expect("sidebar");
    let chats = engine.sidebar().unwrap().chats;
    let mut scanned = 0;
    let mut messages = 0;
    let mut with_images = Vec::new();
    for chat in chats.iter().take(CHATS_TO_SCAN) {
        if engine.fetch_newer(&chat.id).await.is_err() {
            continue;
        }
        scanned += 1;
        for record in engine.open_conversation(&chat.id).unwrap() {
            messages += 1;
            with_images.extend(images(&record));
        }
    }
    let graph_hosted = with_images
        .iter()
        .filter(|image| image.url.starts_with("https://graph.microsoft.com/"))
        .count();
    println!(
        "chats scanned {scanned} messages {messages} images {} graph-hosted {graph_hosted}",
        with_images.len()
    );
    let mut fetched = 0;
    let mut total_bytes = 0;
    let started = Instant::now();
    for image in with_images
        .iter()
        .filter(|image| image.url.starts_with("https://graph.microsoft.com/"))
        .take(IMAGES_TO_FETCH)
    {
        let outcome = engine.fetch_image(image).await;
        match (outcome, engine.image(image.key())) {
            (Ok(_), Some(stored)) => {
                fetched += 1;
                total_bytes += stored.bytes.len();
            }
            (Err(error), _) => println!(
                "fetch failed: {}",
                error.to_string().chars().take(60).collect::<String>()
            ),
            _ => println!("fetch returned nothing"),
        }
    }
    println!(
        "fetched {fetched} images, {total_bytes} bytes, {} ms",
        started.elapsed().as_millis()
    );
}
