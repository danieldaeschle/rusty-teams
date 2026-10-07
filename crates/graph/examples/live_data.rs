use std::collections::BTreeMap;
use std::time::Instant;

use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let graph = Graph::new(Session::connect(&endpoint).await.expect("browser"));
    let me = graph.me().await.expect("me").id;
    let chats = graph.chats_page(50).await.expect("chats").items;
    let with_preview = chats
        .iter()
        .filter(|chat| chat.last_message_preview.is_some())
        .count();
    let with_body = chats
        .iter()
        .filter_map(|chat| {
            chat.last_message_preview
                .as_ref()?
                .body
                .as_ref()?
                .content
                .as_ref()
        })
        .filter(|content| !content.trim().is_empty())
        .count();
    println!(
        "chats {} with_preview {with_preview} with_body {with_body}",
        chats.len()
    );
    let mut user_ids: Vec<String> = chats
        .iter()
        .flat_map(|chat| chat.members.iter())
        .filter_map(|member| member.user_id.clone())
        .filter(|id| *id != me)
        .collect();
    user_ids.sort();
    user_ids.dedup();
    user_ids.truncate(40);
    println!("distinct users {}", user_ids.len());

    let started = Instant::now();
    match graph.user_photos(&user_ids).await {
        Ok(results) => {
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            let (mut hits, mut misses, mut errors, mut total_bytes) = (0, 0, 0, 0usize);
            for result in &results {
                match result {
                    Ok(Some(photo)) => {
                        hits += 1;
                        total_bytes += photo.bytes.len();
                        *kinds.entry(photo.content_type.clone()).or_default() += 1;
                    }
                    Ok(None) => misses += 1,
                    Err(error) => {
                        errors += 1;
                        if errors == 1 {
                            println!("first photo error status: {}", status_of(error));
                        }
                    }
                }
            }
            println!(
                "photos hits {hits} misses {misses} errors {errors} bytes {total_bytes} types {kinds:?} in {} ms",
                started.elapsed().as_millis()
            );
        }
        Err(error) => println!("photos failed: {}", status_of(&error)),
    }

    let started = Instant::now();
    match graph.presences(&user_ids).await {
        Ok(found) => {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for presence in &found {
                *counts.entry(presence.availability.clone()).or_default() += 1;
            }
            println!(
                "teams presence ok {} {counts:?} in {} ms",
                found.len(),
                started.elapsed().as_millis()
            );
        }
        Err(error) => println!("teams presence failed: {}", status_of(&error)),
    }
}

fn status_of(error: &graph::Error) -> String {
    match error {
        graph::Error::Session(session::Error::Api { status, .. }) => format!("HTTP {status}"),
        graph::Error::Session(session::Error::NoFreshToken { .. }) => {
            "no fresh token (scope not granted)".into()
        }
        graph::Error::Session(session::Error::LoginRequired(_)) => {
            "login required (scope not granted)".into()
        }
        _ => "other error".into(),
    }
}
