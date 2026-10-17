use std::time::Duration;

use chatsvc::{ConversationRef, Messages};
use chrono::Utc;
use graph::{Graph, MessageExtras};
use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, IC3, Method, Request, Scope, Session};
use teams_core::{Draft, NOTES_CHAT_ID, Table};

const HISTORY_PAGES: usize = 30;

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");

    let mut draft = Draft::default();
    draft.insert_table(
        0,
        Table::from_rows(vec![
            vec!["a".into(), "b".into()],
            vec!["1".into(), "2".into()],
            vec!["line one\nline two".into(), String::new()],
        ]),
    );
    let html = draft.to_html();
    println!("# composer html\n{html}\n");

    let via_chatsvc = std::env::args().any(|argument| argument == "--chatsvc");
    let message_id = if via_chatsvc {
        send_via_chatsvc(&session, &html).await
    } else {
        let sent = Graph::new(session.clone())
            .send_chat_message(NOTES_CHAT_ID, &html, &[], &MessageExtras::default())
            .await
            .expect("graph send to 48:notes");
        println!("# sent via graph");
        sent.id
    };
    println!("# sent id {message_id}");

    tokio::time::sleep(Duration::from_secs(2)).await;
    match find_raw(&session, &message_id).await {
        Some(message) => print_raw("chatsvc stored (ours)", &message),
        None => println!("# own message not found in chatsvc"),
    }

    let mut found_teams_table = false;
    for message in table_history(&session, &message_id).await {
        found_teams_table = true;
        print_raw("chatsvc stored (history table)", &message);
    }
    if !found_teams_table {
        println!("# no other table in 48:notes history ({HISTORY_PAGES} pages)");
    }

    Messages::new(&session)
        .soft_delete_message(&ConversationRef::chat(NOTES_CHAT_ID), &message_id)
        .await
        .expect("delete");
    tokio::time::sleep(Duration::from_secs(2)).await;
    match find_raw(&session, &message_id).await {
        Some(message) => println!(
            "# after delete: deletetime={} content_len={}",
            message
                .pointer("/properties/deletetime")
                .map(Value::to_string)
                .unwrap_or_else(|| "-".into()),
            message
                .get("content")
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or(0)
        ),
        None => println!("# after delete: message no longer listed"),
    }
}

async fn send_via_chatsvc(session: &Session, html: &str) -> String {
    let client_message_id = Utc::now().timestamp_millis().to_string();
    let body = json!({
        "clientmessageid": client_message_id,
        "content": html,
        "messagetype": "RichText/Html",
        "contenttype": "text",
        "imdisplayname": "",
        "properties": {"importance": "", "subject": ""},
    });
    let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
    let answer = session
        .batch(
            &[Request::with_body(Method::Post, messages_url(""), body)],
            &scope,
        )
        .await
        .expect("chatsvc send")
        .remove(0);
    assert!(answer.is_success(), "chatsvc send status {}", answer.status);
    println!("# sent via chatsvc");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let listing = page(session, &first_page_url()).await;
    listing["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|message| message["clientmessageid"].as_str() == Some(client_message_id.as_str()))
        .and_then(|message| message["id"].as_str())
        .expect("sent message in listing")
        .to_owned()
}

fn messages_url(query: &str) -> String {
    format!(
        "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Anotes/messages{query}"
    )
}

async fn page(session: &Session, url: &str) -> Value {
    let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
    let answer = session
        .batch(&[Request::get(url)], &scope)
        .await
        .expect("chatsvc")
        .remove(0);
    assert!(answer.is_success(), "chatsvc status {}", answer.status);
    answer.body
}

fn first_page_url() -> String {
    messages_url("?view=msnp24Equivalent%7CsupportsMessageProperties&pageSize=50&startTime=1")
}

async fn find_raw(session: &Session, message_id: &str) -> Option<Value> {
    let body = page(session, &first_page_url()).await;
    body.get("messages")?
        .as_array()?
        .iter()
        .find(|message| message.get("id").and_then(Value::as_str) == Some(message_id))
        .cloned()
}

async fn table_history(session: &Session, own_id: &str) -> Vec<Value> {
    let mut tables = Vec::new();
    let mut url = first_page_url();
    for _ in 0..HISTORY_PAGES {
        let body = page(session, &url).await;
        for message in body
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let is_own = message.get("id").and_then(Value::as_str) == Some(own_id);
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !is_own && content.contains("<table") {
                tables.push(message.clone());
            }
        }
        match body
            .pointer("/_metadata/backwardLink")
            .and_then(Value::as_str)
        {
            Some(next) => match next.split_once('?') {
                Some((_, query)) => url = messages_url(&format!("?{query}")),
                None => break,
            },
            _ => break,
        }
    }
    tables
}

fn print_raw(label: &str, message: &Value) {
    let text = |key: &str| {
        message
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_owned()
    };
    println!(
        "# {label}: id={} messagetype={} contenttype={} clientinfo={}",
        text("id"),
        text("messagetype"),
        text("contenttype"),
        message
            .pointer("/properties/clientinfo")
            .and_then(Value::as_str)
            .unwrap_or("-")
    );
    println!("{}\n", text("content"));
}
