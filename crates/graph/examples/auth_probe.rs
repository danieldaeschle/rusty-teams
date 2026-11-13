//! Read-only diagnostic: report whether profile, chat list, and chat history load.
//! Never prints credentials, user identifiers, chat titles, or message contents.
use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session, SessionConfig};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let graph =
        Graph::new(Session::connect_with(DEFAULT_ENDPOINT, SessionConfig::for_app()).await?);
    let _ = graph.me().await?;
    println!("profile: ok");
    let chats = graph.chats_page(30).await?;
    println!(
        "chat list: ok ({} chats, more={})",
        chats.items.len(),
        chats.next_link.is_some()
    );
    if let Some(chat) = chats.items.iter().find(|c| c.chat_type == "oneOnOne") {
        println!(
            "chat members: {}",
            graph.chat_members(&chat.id).await?.len()
        );
        let messages = graph.chat_messages(&chat.id, None, 5).await?;
        println!(
            "chat history: ok ({} messages, more={})",
            messages.items.len(),
            messages.next_link.is_some()
        );
        if let Some(oldest) = messages
            .items
            .iter()
            .filter_map(|m| m.created_date_time)
            .min()
        {
            let older = graph.chat_messages(&chat.id, Some(oldest), 5).await?;
            if older
                .items
                .iter()
                .any(|m| m.created_date_time.is_some_and(|t| t >= oldest))
            {
                return Err("older history overlaps its cursor".into());
            }
            println!("older history: ok ({} messages)", older.items.len());
        }
        if let Some(message) = messages.items.first() {
            let _ = graph.chat_message(&chat.id, &message.id).await?;
            println!("single message: ok");
        }
    }
    Ok(())
}
