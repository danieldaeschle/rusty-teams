use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let graph = Graph::new(Session::connect(&endpoint).await.expect("browser"));
    let files = graph.recent_files(40).await.expect("recent files");
    println!("recent {}", files.len());
    for file in files.iter().take(12) {
        println!("  {} opened {:?} ids {}", file.name, file.last_opened, file.item_id.is_some());
    }
    if let Some(first) = files.first() {
        match graph.cloud_file_reference(first).await {
            Ok(uploaded) => println!("reference ok: {} etag {}", uploaded.name, uploaded.reference().is_some()),
            Err(error) => println!("reference error: {error}"),
        }
    }
}
