use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

use super::entry::{SERVER_NAME, ServerEntry};

pub(super) fn vscode(entry: &ServerEntry) -> String {
    let mut config = entry.to_json(true, false);
    config
        .as_object_mut()
        .expect("entry is an object")
        .insert("name".into(), Value::from(SERVER_NAME));
    let encoded = utf8_percent_encode(&config.to_string(), NON_ALPHANUMERIC).to_string();
    format!("vscode:mcp/install?{encoded}")
}
