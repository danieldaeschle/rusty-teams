use std::collections::HashSet;

use gpui_kit::*;

pub fn endpoint(cx: &App) -> Option<agents::McpEndpoint> {
    crate::agent_api::mcp_endpoint(cx).map(|endpoint| agents::McpEndpoint {
        url: endpoint.url,
        token: endpoint.token,
    })
}

pub fn chat_allowed(conversation_id: &str, cx: &App) -> bool {
    crate::agent_api::chat_allowed(conversation_id, cx)
}

pub fn allow_chat(conversation_id: &str, cx: &mut App) {
    crate::agent_api::allow_chat(conversation_id, cx);
}

pub fn set_app_sessions(session_ids: HashSet<String>, cx: &App) {
    crate::agent_api::set_app_sessions(session_ids, cx);
}
