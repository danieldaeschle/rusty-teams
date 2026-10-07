use chatsvc::{Pins, mask_conversation_id};
use session::{DEFAULT_ENDPOINT, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let pins = Pins::new(&session);
    let chats = pins.pinned_chats().await.expect("pinned chats");
    println!("pinned chats: {}", chats.chat_ids.len());
    for (position, chat_id) in chats.chat_ids.iter().enumerate() {
        println!("{position:>3} {}", mask_conversation_id(chat_id));
    }
    let channels = pins.pinned_channels().await.expect("pinned channels");
    println!("pinned channels: {}", channels.channel_ids.len());
    for (position, channel_id) in channels.channel_ids.iter().enumerate() {
        println!("{position:>3} {}", mask_conversation_id(channel_id));
    }
}
