use serde_json::Value;

use super::{RunEvent, truncate};

const DETAIL_LIMIT: usize = 60;

pub(super) fn parse_line(line: &str, last_text: &mut Option<String>) -> Vec<RunEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match value.get("type").and_then(Value::as_str) {
        Some("tool.execution_start") => vec![RunEvent::Step(step_label(&value))],
        Some("assistant.message") => assistant_events(&value, last_text),
        Some("result") => result_events(&value, last_text),
        _ => Vec::new(),
    }
}

fn assistant_events(value: &Value, last_text: &mut Option<String>) -> Vec<RunEvent> {
    let content = value
        .pointer("/data/content")
        .and_then(Value::as_str)
        .unwrap_or("");
    if content.trim().is_empty() {
        return Vec::new();
    }
    *last_text = Some(content.to_string());
    vec![RunEvent::Text(content.to_string())]
}

fn result_events(value: &Value, last_text: &Option<String>) -> Vec<RunEvent> {
    let mut events = Vec::new();
    if let Some(session_id) = value.get("sessionId").and_then(Value::as_str) {
        events.push(RunEvent::SessionId(session_id.to_string()));
    }
    let exit_code = value.get("exitCode").and_then(Value::as_i64).unwrap_or(0);
    match last_text {
        Some(text) if exit_code == 0 => events.push(RunEvent::Final(text.clone())),
        _ if exit_code != 0 => events.push(RunEvent::Failed(format!(
            "agent exited with code {exit_code}"
        ))),
        _ => events.push(RunEvent::Failed("agent returned no answer".to_string())),
    }
    events
}

fn step_label(value: &Value) -> String {
    let title = value
        .pointer("/data/toolTitle")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/data/toolName").and_then(Value::as_str))
        .unwrap_or("Tool");
    match tool_detail(value.pointer("/data/arguments")) {
        Some(detail) => format!("{title}: {}", truncate(&detail, DETAIL_LIMIT)),
        None => title.to_string(),
    }
}

fn tool_detail(arguments: Option<&Value>) -> Option<String> {
    let arguments = arguments?.as_object()?;
    ["path", "command"]
        .iter()
        .find_map(|key| arguments.get(*key).and_then(Value::as_str))
        .or_else(|| arguments.values().find_map(Value::as_str))
        .map(str::to_string)
}
