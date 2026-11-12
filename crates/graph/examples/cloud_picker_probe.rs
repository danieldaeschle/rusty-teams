use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let graph = Graph::new(Session::connect(&endpoint).await.expect("browser"));
    match graph.recent_files_with_owners(40).await {
        Ok((files, owners)) => println!("recent {} files, {} owners", files.len(), owners.len()),
        Err(error) => println!("recent error: {error}"),
    }
    match graph.drive_root(None).await {
        Ok(root) => match graph.list_children(&root).await {
            Ok(children) => println!(
                "my files {} entries, {} folders, {} with modifier",
                children.len(),
                children.iter().filter(|entry| entry.is_folder()).count(),
                children
                    .iter()
                    .filter(|entry| entry.modified_by.is_some())
                    .count()
            ),
            Err(error) => println!("my files error: {error}"),
        },
        Err(error) => println!("drive root error: {error}"),
    }
    match graph.shared_with_me().await {
        Ok(shared) => println!(
            "shared {} entries, {} with sharer, {} with date",
            shared.len(),
            shared
                .iter()
                .filter(|entry| entry.modified_by.is_some())
                .count(),
            shared
                .iter()
                .filter(|entry| entry.modified_at.is_some())
                .count()
        ),
        Err(error) => println!("shared error: {error}"),
    }
    match graph.quick_access_sites().await {
        Ok(sites) => {
            println!("sites {}", sites.len());
            for site in &sites {
                match graph.drive_root(Some(&site.id)).await {
                    Ok(root) => match graph.list_children(&root).await {
                        Ok(children) => println!("first site library: {} entries", children.len()),
                        Err(error) => println!("site library error: {error}"),
                    },
                    Err(error) => println!("site root error: {error}"),
                }
            }
        }
        Err(error) => println!("sites error: {error}"),
    }
}
