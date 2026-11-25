//! Read-only live probe for the strict no-browser authentication transport.
use std::path::PathBuf;
use std::sync::Arc;

use session::{GRAPH, IC3, NativeTransport, Request, Scope, Session, SessionConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let refresh_token_file = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("RUSTY_TEAMS_REFRESH_TOKEN_FILE").map(PathBuf::from))
        .ok_or("pass the saved native refresh-token file as the first argument")?;
    let transport = Arc::new(NativeTransport::teams(refresh_token_file));
    let session = Session::with_transport(
        transport,
        SessionConfig {
            native_http: true,
            ..SessionConfig::default()
        },
    )
    .await?;

    let graph = session
        .send(
            Request::get("https://graph.microsoft.com/v1.0/me?$select=id"),
            &Scope::new(GRAPH, "User.ReadBasic.All"),
        )
        .await?;
    let chats = session
        .send(
            Request::get(
                "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations?view=msnp24Equivalent&pageSize=1",
            ),
            &Scope::new(IC3, "Teams.AccessAsUser.All"),
        )
        .await?;
    let conversation_count = chats
        .body
        .get("conversations")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .ok_or("Teams chatsvc returned no conversation list")?;
    println!(
        "native no-browser authentication succeeded: Graph HTTP {}, Teams chatsvc HTTP {}, {} conversation(s) in the probe page",
        graph.status, chats.status, conversation_count
    );
    Ok(())
}
