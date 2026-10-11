use chatsvc::Pins;
use session::{DEFAULT_ENDPOINT, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let pins = Pins::new(&session);
    match pins.folders().await {
        Ok(folders) => println!("# folders ok: {}", folders.folders.len()),
        Err(error) => println!("# folders failed: {}", error.to_string().chars().take(160).collect::<String>()),
    }
    match pins.pinned_channels().await {
        Ok(channels) => println!("# pinned channels ok: {}", channels.channel_ids.len()),
        Err(error) => println!("# pinned channels failed: {}", error.to_string().chars().take(160).collect::<String>()),
    }
    match pins.team_layout().await {
        Ok(layout) => println!("# team layout ok: {} teams, {} tabs", layout.len(), layout.iter().flat_map(|team| team.channels.iter()).map(|channel| channel.tabs.len()).sum::<usize>()),
        Err(error) => println!("# team layout failed: {}", error.to_string().chars().take(160).collect::<String>()),
    }
}
