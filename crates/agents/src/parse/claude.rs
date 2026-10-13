use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::{RunEvent, truncate};

const COMMAND_LIMIT: usize = 60;

pub(super) fn parse_line(line: &str) -> Vec<RunEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match value.get("type").and_then(Value::as_str) {
        Some("system") => match value.get("subtype").and_then(Value::as_str) {
            Some("init") => session_id_events(&value),
            Some("api_error" | "api_retry") => vec![api_retry_event(&value)],
            _ => Vec::new(),
        },
        Some("assistant") => assistant_events(&value),
        Some("result") => result_events(&value),
        _ => Vec::new(),
    }
}

fn session_id_events(value: &Value) -> Vec<RunEvent> {
    value
        .get("session_id")
        .and_then(Value::as_str)
        .map(|session_id| vec![RunEvent::SessionId(session_id.to_string())])
        .unwrap_or_default()
}

fn api_retry_event(value: &Value) -> RunEvent {
    let error = value.get("error");
    let message = error
        .and_then(|error| {
            error.as_str().or_else(|| {
                ["formatted", "message"]
                    .iter()
                    .find_map(|key| error.get(*key).and_then(Value::as_str))
            })
        })
        .unwrap_or("API error")
        .to_string();
    let retry_milliseconds = [
        error.and_then(|error| error.pointer("/noResponse/retryWaitMs")),
        error.and_then(|error| error.get("retryWaitMs")),
        value.get("retry_delay_ms"),
        value.get("retryWaitMs"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_f64);
    RunEvent::ApiRetry {
        message,
        retry_in: retry_milliseconds
            .filter(|milliseconds| *milliseconds >= 0.0)
            .map(|milliseconds| Duration::from_millis(milliseconds as u64)),
    }
}

fn assistant_events(value: &Value) -> Vec<RunEvent> {
    let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("tool_use") => Some(RunEvent::Step(tool_label(block))),
            Some("text") => block
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .map(|text| RunEvent::Text(text.to_string())),
            _ => None,
        })
        .collect()
}

fn result_events(value: &Value) -> Vec<RunEvent> {
    let mut events = session_id_events(value);
    let subtype = value.get("subtype").and_then(Value::as_str).unwrap_or("");
    let is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let result = value.get("result").and_then(Value::as_str);
    if subtype == "success" && !is_error {
        events.push(RunEvent::Final(result.unwrap_or_default().to_string()));
    } else {
        let message = result.filter(|text| !text.is_empty()).unwrap_or(subtype);
        events.push(RunEvent::Failed(message.to_string()));
    }
    events
}

fn input_text<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn host_of(url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    without_scheme
        .split(['/', ':', '?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

fn tool_label(block: &Value) -> String {
    let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
    let null = Value::Null;
    let input = block.get("input").unwrap_or(&null);
    match name {
        "Read" => format!("Reading {}", file_name(input_text(input, "file_path"))),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let path = match input_text(input, "file_path") {
                "" => input_text(input, "notebook_path"),
                path => path,
            };
            format!("Editing {}", file_name(path))
        }
        "Bash" => {
            let description = input_text(input, "description");
            let detail = if description.is_empty() {
                truncate(input_text(input, "command"), COMMAND_LIMIT)
            } else {
                description.to_string()
            };
            format!("Running {detail}")
        }
        "Grep" => format!("Searching \"{}\"", input_text(input, "pattern")),
        "Glob" => format!("Finding {}", input_text(input, "pattern")),
        "WebSearch" => format!("Searching web: {}", input_text(input, "query")),
        "WebFetch" => format!("Reading {}", host_of(input_text(input, "url"))),
        "Task" | "Agent" => "Running subagent".to_string(),
        "TodoWrite" => "Planning".to_string(),
        "mcp__teams__propose_message" => "Proposing reply".to_string(),
        other if other.starts_with("mcp__teams__") => "Reading Teams".to_string(),
        other => other.to_string(),
    }
}
