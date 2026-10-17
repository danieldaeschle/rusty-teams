use std::sync::Arc;
use std::time::Duration;

use graph::{Graph, MessageExtras};
use serde_json::Value;
use session::{DEFAULT_ENDPOINT, IC3, Request, Scope, Session};
use store::Store;
use teams_core::{NOTES_CHAT_ID, SyncEngine};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let engine = SyncEngine::new(
        Graph::new(session.clone()),
        Arc::new(Store::open_in_memory().unwrap()),
    );

    let sent = engine
        .send_message_with_extras(
            NOTES_CHAT_ID,
            "<p>rusty-teams notes send probe</p>",
            None,
            &[],
            &MessageExtras::default(),
        )
        .await
        .expect("engine send to 48:notes");
    let message_id = sent.message_id.clone();
    println!("# engine send ok, id {message_id}");

    tokio::time::sleep(Duration::from_secs(2)).await;
    report(&session, &message_id, "after send").await;

    let edit = engine
        .edit_message_html(
            NOTES_CHAT_ID,
            &message_id,
            "<p>rusty-teams notes probe edited</p>",
            &[],
        )
        .await;
    println!("# engine edit: {}", outcome(&edit));
    let reaction = engine
        .set_reaction(NOTES_CHAT_ID, &message_id, "\u{1f44d}")
        .await;
    println!("# engine set reaction: {}", outcome(&reaction));
    let removal = engine
        .unset_reaction(NOTES_CHAT_ID, &message_id, "\u{1f44d}")
        .await;
    println!("# engine unset reaction: {}", outcome(&removal));
    tokio::time::sleep(Duration::from_secs(2)).await;
    report(&session, &message_id, "after edit and reactions").await;

    let deletion = engine.soft_delete_message(NOTES_CHAT_ID, &message_id).await;
    println!("# engine delete: {}", outcome(&deletion));
    tokio::time::sleep(Duration::from_secs(2)).await;
    report(&session, &message_id, "after delete").await;
}

fn outcome<T>(result: &teams_core::Result<T>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(error) => format!("error {error}"),
    }
}

async fn report(session: &Session, message_id: &str, label: &str) {
    let url = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Anotes/messages?view=msnp24Equivalent%7CsupportsMessageProperties&pageSize=20&startTime=1";
    let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
    let answer = session
        .batch(&[Request::get(url)], &scope)
        .await
        .expect("chatsvc")
        .remove(0);
    let found = answer
        .body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|message| message.get("id").and_then(Value::as_str) == Some(message_id))
        .cloned();
    let Some(message) = found else {
        println!(
            "# {label}: not in chatsvc listing (status {})",
            answer.status
        );
        return;
    };
    let text = |pointer: &str| {
        message
            .pointer(pointer)
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            })
            .unwrap_or_else(|| "-".to_owned())
    };
    println!(
        "# {label}: content={} edittime={} deletetime={} emotions={}",
        text("/content"),
        text("/properties/edittime"),
        text("/properties/deletetime"),
        text("/properties/emotions"),
    );
}
