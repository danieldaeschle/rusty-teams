use serde_json::{Value, json};

use crate::McpEndpoint;

pub(super) const SERVER_NAME: &str = "teams";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ServerEntry {
    pub url: String,
    pub authorization: String,
}

impl ServerEntry {
    pub fn expected(endpoint: &McpEndpoint) -> Self {
        ServerEntry {
            url: endpoint.url.clone(),
            authorization: format!("Bearer {}", endpoint.token),
        }
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        Some(ServerEntry {
            url: value.get("url")?.as_str()?.to_string(),
            authorization: value
                .get("headers")?
                .get("Authorization")?
                .as_str()?
                .to_string(),
        })
    }

    pub fn to_json(&self, with_type: bool, with_tools: bool) -> Value {
        let mut value = json!({
            "url": self.url,
            "headers": { "Authorization": self.authorization },
        });
        let object = value.as_object_mut().expect("object literal");
        if with_type {
            object.insert("type".into(), json!("http"));
        }
        if with_tools {
            object.insert("tools".into(), json!(["*"]));
        }
        value
    }
}
