use std::sync::Arc;

use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};
use store::Store;
use teams_core::{ChatsvcFolderSource, SyncEngine};

#[tokio::main]
async fn main() {
    let path = std::env::args().nth(1).expect("cache copy path");
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let store = Arc::new(Store::open(std::path::Path::new(&path)).expect("store"));
    let engine = SyncEngine::new(Graph::new(session.clone()), store.clone())
        .with_folder_source(Arc::new(ChatsvcFolderSource::new(&session)));
    match engine.refresh_folders().await {
        Ok(()) => println!("# refresh folders ok"),
        Err(error) => println!("# refresh folders failed: {}", error.to_string().chars().take(200).collect::<String>()),
    }
}
