use serde_json::{Value, json};

use crate::profile::{AgentBackend, AgentProfile};

const TURN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const MAX_TOKENS: u32 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: String,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            text: text.into(),
            tool_calls: Vec::new(),
            tool_call_id: String::new(),
        }
    }

    pub fn assistant(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: ChatRole::Assistant,
            text: text.into(),
            tool_calls,
            tool_call_id: String::new(),
        }
    }

    pub fn tool(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            text: text.into(),
            tool_calls: Vec::new(),
            tool_call_id: id.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatReply {
    Text(String),
    Tools { text: String, calls: Vec<ToolCall> },
}

pub async fn complete_text(
    profile: &AgentProfile,
    system: &str,
    user: &str,
) -> Result<String, String> {
    match next_turn(profile, system, &[ChatMessage::user(user)], &[]).await? {
        ChatReply::Text(text) => Ok(text),
        ChatReply::Tools { text, .. } => Ok(text),
    }
}

pub async fn next_turn(
    profile: &AgentProfile,
    system: &str,
    messages: &[ChatMessage],
    tools: &[ToolSpec],
) -> Result<ChatReply, String> {
    let body = match &profile.backend {
        AgentBackend::OpenAi { .. } => openai_body(system, messages, tools),
        AgentBackend::Anthropic { .. } => anthropic_body(profile, system, messages, tools)?,
        AgentBackend::Command => return Err("this agent is a command, not a chat API".to_owned()),
    };
    let value = post(profile, &body).await?;
    match &profile.backend {
        AgentBackend::OpenAi { .. } => parse_openai(&value),
        AgentBackend::Anthropic { .. } => parse_anthropic(&value),
        AgentBackend::Command => unreachable!(),
    }
}

const MODEL_LIST_LIMIT: usize = 400;

pub async fn list_models(profile: &AgentProfile) -> Result<Vec<String>, String> {
    let (url, key, anthropic) = models_endpoint(profile)?;
    let value = get(&key, &url, anthropic).await?;
    let mut names = model_names(&value);
    names.sort_by(|left, right| left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()));
    names.dedup();
    names.truncate(MODEL_LIST_LIMIT);
    if names.is_empty() {
        return Err("the endpoint listed no models".to_owned());
    }
    Ok(names)
}

pub async fn probe_backend(profile: &AgentProfile) -> Result<String, String> {
    let names = list_models(profile).await?;
    Ok(match names.len() {
        1 => names[0].clone(),
        2..=4 => names.join(", "),
        count => format!("{count} models"),
    })
}

fn models_endpoint(profile: &AgentProfile) -> Result<(String, String, bool), String> {
    match &profile.backend {
        AgentBackend::OpenAi {
            base_url, api_key, ..
        } => Ok((models_url(base_url), api_key.clone(), false)),
        AgentBackend::Anthropic { api_key, .. } => {
            if api_key.trim().is_empty() {
                return Err("the Anthropic key is empty".to_owned());
            }
            Ok((
                "https://api.anthropic.com/v1/models".to_owned(),
                api_key.clone(),
                true,
            ))
        }
        AgentBackend::Command => Err("this agent is a command".to_owned()),
    }
}

fn openai_body(system: &str, messages: &[ChatMessage], tools: &[ToolSpec]) -> Value {
    let mut chat = Vec::new();
    if !system.is_empty() {
        chat.push(json!({"role": "system", "content": system}));
    }
    for message in messages {
        match message.role {
            ChatRole::User => chat.push(json!({"role": "user", "content": message.text})),
            ChatRole::Tool => chat.push(json!({
                "role": "tool",
                "tool_call_id": message.tool_call_id,
                "content": message.text,
            })),
            ChatRole::Assistant => {
                let mut row = json!({"role": "assistant", "content": message.text});
                if !message.tool_calls.is_empty() {
                    row["tool_calls"] = Value::Array(
                        message
                            .tool_calls
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.id,
                                    "type": "function",
                                    "function": { "name": call.name, "arguments": call.arguments },
                                })
                            })
                            .collect(),
                    );
                }
                chat.push(row);
            }
        }
    }
    let mut body = json!({"messages": chat, "stream": false});
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect(),
        );
    }
    body
}

fn anthropic_body(
    profile: &AgentProfile,
    system: &str,
    messages: &[ChatMessage],
    tools: &[ToolSpec],
) -> Result<Value, String> {
    let AgentBackend::Anthropic { model, .. } = &profile.backend else {
        return Err("not an Anthropic profile".to_owned());
    };
    let mut chat = Vec::new();
    for message in messages {
        match message.role {
            ChatRole::User => chat.push(json!({"role": "user", "content": message.text})),
            ChatRole::Tool => chat.push(json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": message.tool_call_id,
                    "content": message.text,
                }]
            })),
            ChatRole::Assistant => {
                let mut content = Vec::new();
                if !message.text.is_empty() {
                    content.push(json!({"type": "text", "text": message.text}));
                }
                for call in &message.tool_calls {
                    let input = serde_json::from_str::<Value>(&call.arguments)
                        .unwrap_or(Value::Object(Default::default()));
                    content.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": input,
                    }));
                }
                chat.push(json!({"role": "assistant", "content": content}));
            }
        }
    }
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "messages": chat,
    });
    if !system.is_empty() {
        body["system"] = Value::String(system.to_owned());
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters,
                    })
                })
                .collect(),
        );
    }
    Ok(body)
}

fn chat_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return base.to_owned();
    }
    // Ollama answers the OpenAI route only under /v1. The model list also
    // accepts the bare port, so a base without /v1 still has to land there.
    if base.contains(":11434") {
        let origin = base.strip_suffix("/v1").unwrap_or(base);
        return format!("{origin}/v1/chat/completions");
    }
    format!("{base}/chat/completions")
}

fn models_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.contains(":11434") {
        let origin = base.trim_end_matches("/v1");
        format!("{origin}/api/tags")
    } else {
        format!("{base}/models")
    }
}

async fn post(profile: &AgentProfile, body: &Value) -> Result<Value, String> {
    let (url, key, anthropic, model) = endpoint(profile)?;
    let mut payload = body.clone();
    if !anthropic {
        payload["model"] = Value::String(model);
    }
    let client = client(TURN_TIMEOUT)?;
    let mut request = client.post(url).json(&payload);
    request = authorize(request, &key, anthropic);
    read_json(request).await
}

async fn get(key: &str, url: &str, anthropic: bool) -> Result<Value, String> {
    let client = client(PROBE_TIMEOUT)?;
    let request = authorize(client.get(url), key, anthropic);
    read_json(request).await
}

fn endpoint(profile: &AgentProfile) -> Result<(String, String, bool, String), String> {
    match &profile.backend {
        AgentBackend::OpenAi {
            base_url,
            model,
            api_key,
        } => {
            if model.trim().is_empty() {
                return Err("choose a model first".to_owned());
            }
            Ok((chat_url(base_url), api_key.clone(), false, model.clone()))
        }
        AgentBackend::Anthropic { model, api_key } => {
            if api_key.trim().is_empty() {
                return Err("the Anthropic key is empty".to_owned());
            }
            if model.trim().is_empty() {
                return Err("choose a model first".to_owned());
            }
            Ok((
                ANTHROPIC_URL.to_owned(),
                api_key.clone(),
                true,
                model.clone(),
            ))
        }
        AgentBackend::Command => Err("this agent is a command".to_owned()),
    }
}

fn client(timeout: std::time::Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| error.to_string())
}

fn authorize(
    request: reqwest::RequestBuilder,
    key: &str,
    anthropic: bool,
) -> reqwest::RequestBuilder {
    if anthropic {
        return request
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION);
    }
    if key.trim().is_empty() {
        request
    } else {
        request.bearer_auth(key)
    }
}

async fn read_json(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        let body = String::from_utf8_lossy(&bytes);
        let snippet: String = body.trim().chars().take(180).collect();
        if snippet.is_empty() {
            format!("HTTP {status} with an empty body")
        } else {
            format!("HTTP {status}: {snippet}")
        }
    })?;
    if !status.is_success() {
        return Err(api_error(&value).unwrap_or_else(|| format!("HTTP {status}")));
    }
    Ok(value)
}

fn api_error(value: &Value) -> Option<String> {
    value["error"]["message"]
        .as_str()
        .or_else(|| value["error"].as_str())
        .map(str::to_owned)
}

pub(crate) fn parse_openai(value: &Value) -> Result<ChatReply, String> {
    if let Some(message) = api_error(value) {
        return Err(message);
    }
    let message = &value["choices"][0]["message"];
    let text = message["content"].as_str().unwrap_or("").trim().to_owned();
    let calls = message["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    let name = call["function"]["name"].as_str()?;
                    let arguments = call["function"]["arguments"].as_str().unwrap_or("{}");
                    Some(ToolCall {
                        id: call["id"].as_str().unwrap_or(name).to_owned(),
                        name: name.to_owned(),
                        arguments: arguments.to_owned(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if calls.is_empty() {
        if text.is_empty() {
            return Err("the model returned an empty answer".to_owned());
        }
        Ok(ChatReply::Text(text))
    } else {
        Ok(ChatReply::Tools { text, calls })
    }
}

pub(crate) fn parse_anthropic(value: &Value) -> Result<ChatReply, String> {
    if let Some(message) = api_error(value) {
        return Err(message);
    }
    let mut text = String::new();
    let mut calls = Vec::new();
    for block in value["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("text") => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(block["text"].as_str().unwrap_or(""));
            }
            Some("tool_use") => {
                let name = block["name"].as_str().unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let input = &block["input"];
                calls.push(ToolCall {
                    id: block["id"].as_str().unwrap_or(name).to_owned(),
                    name: name.to_owned(),
                    arguments: input.to_string(),
                });
            }
            _ => {}
        }
    }
    let text = text.trim().to_owned();
    if calls.is_empty() {
        if text.is_empty() {
            return Err("the model returned an empty answer".to_owned());
        }
        Ok(ChatReply::Text(text))
    } else {
        Ok(ChatReply::Tools { text, calls })
    }
}

fn model_names(value: &Value) -> Vec<String> {
    let list = value["models"]
        .as_array()
        .or_else(|| value["data"].as_array());
    list.map(|models| {
        models
            .iter()
            .filter_map(|model| {
                model["name"]
                    .as_str()
                    .or_else(|| model["id"].as_str())
                    .map(str::to_owned)
            })
            .collect()
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_text_and_tool_calls_parse() {
        let text = parse_openai(&json!({
            "choices": [{"message": {"content": " hello "}}]
        }))
        .unwrap();
        assert_eq!(text, ChatReply::Text("hello".to_owned()));
        let tools = parse_openai(&json!({
            "choices": [{"message": {
                "content": "",
                "tool_calls": [{
                    "id": "c1",
                    "function": {"name": "propose_message", "arguments": "{\"text\":\"hi\"}"}
                }]
            }}]
        }))
        .unwrap();
        assert_eq!(
            tools,
            ChatReply::Tools {
                text: String::new(),
                calls: vec![ToolCall {
                    id: "c1".to_owned(),
                    name: "propose_message".to_owned(),
                    arguments: "{\"text\":\"hi\"}".to_owned(),
                }],
            }
        );
    }

    #[test]
    fn anthropic_text_and_tool_use_parse() {
        let text = parse_anthropic(&json!({
            "content": [{"type": "text", "text": "done"}]
        }))
        .unwrap();
        assert_eq!(text, ChatReply::Text("done".to_owned()));
        let tools = parse_anthropic(&json!({
            "content": [
                {"type": "text", "text": "looking"},
                {"type": "tool_use", "id": "t1", "name": "list_chats", "input": {"limit": 1}}
            ]
        }))
        .unwrap();
        match tools {
            ChatReply::Tools { text, calls } => {
                assert_eq!(text, "looking");
                assert_eq!(calls[0].name, "list_chats");
                assert_eq!(calls[0].arguments, "{\"limit\":1}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn model_lists_keep_ollama_names_and_openai_ids() {
        let ollama = model_names(&json!({
            "models": [{"name": "llama3.2:latest"}, {"name": "qwen2.5:7b"}]
        }));
        assert_eq!(ollama, ["llama3.2:latest", "qwen2.5:7b"]);
        let openai = model_names(&json!({"data": [{"id": "gpt-4o"}, {"id": "gpt-4o-mini"}]}));
        assert_eq!(openai, ["gpt-4o", "gpt-4o-mini"]);
        let many = model_names(&json!({
            "data": (0..12).map(|index| json!({"id": format!("m{index}")})).collect::<Vec<_>>()
        }));
        assert_eq!(many.len(), 12);
    }

    #[test]
    fn ollama_models_use_the_native_tags_path() {
        assert_eq!(
            models_url("http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434/api/tags"
        );
        assert_eq!(
            chat_url("http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(
            chat_url("http://mac-01.lan:11434"),
            "http://mac-01.lan:11434/v1/chat/completions"
        );
    }
}
