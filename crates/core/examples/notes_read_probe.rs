use chatsvc::conversations::thread_url;
use graph::Graph;
use serde_json::Value;
use session::{DEFAULT_ENDPOINT, IC3, Request, Scope, Session};
use teams_core::NOTES_CHAT_ID;

const LISTING_URL: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations?view=msnp24Equivalent&pageSize=200";

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let graph = Graph::new(session.clone());

    match graph.chat_messages(NOTES_CHAT_ID, None, 5).await {
        Ok(page) => {
            println!(
                "# graph messages: {} (next: {})",
                page.items.len(),
                page.next_link.is_some()
            );
            for message in &page.items {
                println!(
                    "  {} {:?} type={:?} deleted={}",
                    message.id,
                    message.created_date_time,
                    message.message_type,
                    message.deleted_date_time.is_some()
                );
            }
        }
        Err(error) => println!("# graph messages failed: {error}"),
    }

    let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
    let mut url = Some(LISTING_URL.to_owned());
    let mut pages = 0;
    while let Some(current) = url.take() {
        pages += 1;
        let body = get(&session, &current, &scope).await;
        let found = body["conversations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|conversation| conversation["id"].as_str() == Some(NOTES_CHAT_ID))
            .cloned();
        if let Some(conversation) = found {
            println!("# listing page {pages}: notes conversation found");
            describe(&conversation);
            break;
        }
        url = body
            .pointer("/_metadata/backwardLink")
            .and_then(Value::as_str)
            .filter(|next| !next.is_empty() && *next != current)
            .map(str::to_owned);
        if url.is_none() {
            println!("# notes conversation not in the listing ({pages} pages)");
        }
    }

    let thread = get(&session, &thread_url("emea", NOTES_CHAT_ID), &scope).await;
    println!("# thread GET");
    describe(&thread);
}

async fn get(session: &Session, url: &str, scope: &Scope) -> Value {
    let answer = session
        .batch(&[Request::get(url)], scope)
        .await
        .expect("chatsvc")
        .remove(0);
    println!("# GET status {}", answer.status);
    answer.body
}

fn describe(conversation: &Value) {
    println!("  id={}", conversation["id"]);
    println!(
        "  type={} version={}",
        conversation["type"], conversation["version"]
    );
    for group in ["properties", "threadProperties"] {
        if let Some(fields) = conversation[group].as_object() {
            for (key, value) in fields {
                let shown = match value {
                    Value::String(text) if text.len() > 80 => format!("<{} chars>", text.len()),
                    other => other.to_string(),
                };
                println!("  {group}.{key} = {shown}");
            }
        }
    }
    let last = &conversation["lastMessage"];
    println!(
        "  lastMessage id={} messagetype={} from={} arrival={} content_len={}",
        last["id"],
        last["messagetype"],
        last["from"]
            .as_str()
            .map(|from| from.rsplit('/').next().unwrap_or(""))
            .unwrap_or("-"),
        last["originalarrivaltime"],
        last["content"].as_str().map(str::len).unwrap_or(0)
    );
}
