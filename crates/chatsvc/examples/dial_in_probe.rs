use std::collections::BTreeSet;

use chatsvc::Calendar;
use chrono::{Duration, Utc};
use serde_json::Value;
use session::{DEFAULT_ENDPOINT, Session};

const DIAL_IN_HINTS: [&str; 5] = [
    "tel:",
    "Conference ID",
    "Konferenz-ID",
    "tollNumber",
    "dialin",
];

fn key_paths(value: &Value, prefix: &str, paths: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = format!("{prefix}.{key}");
                paths.insert(path.clone());
                key_paths(child, &path, paths);
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(1) {
                key_paths(item, &format!("{prefix}[]"), paths);
            }
        }
        _ => {}
    }
}

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let start = Utc::now() - Duration::days(7);
    let answer = Calendar::new(&session)
        .view_response(start, start + Duration::days(21))
        .await
        .expect("calendar view");
    println!("# calendar view: HTTP {}", answer.status);
    let events: Vec<&Value> = match &answer.body {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => map
            .get("value")
            .and_then(Value::as_array)
            .map(|items| items.iter().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    println!("# events: {}", events.len());
    for field in ["onlineMeetingConferenceId", "onlineMeetingTollNumber"] {
        let filled = events
            .iter()
            .filter(|event| {
                event
                    .get(field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty())
            })
            .count();
        println!("# {field} filled: {filled}");
    }
    let shape = events
        .iter()
        .find_map(|event| event.get("onlineMeetingTollNumber").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .chars()
                .map(|character| {
                    if character.is_ascii_digit() {
                        '9'
                    } else {
                        character
                    }
                })
                .collect::<String>()
        });
    println!("# toll number shape: {shape:?}");
    let online = events.iter().find(|event| {
        event.get("isOnlineMeeting").and_then(Value::as_bool) == Some(true)
            || event.get("skypeTeamsMeetingUrl").is_some()
    });
    let Some(event) = online else {
        println!("# no online meeting in range");
        return;
    };
    let mut paths = BTreeSet::new();
    key_paths(event, "", &mut paths);
    println!("# keys of one online meeting:");
    for path in &paths {
        println!("{path}");
    }
    let text = event.to_string();
    for hint in DIAL_IN_HINTS {
        println!("# contains {hint:?}: {}", text.contains(hint));
    }
}
