use agents::{
    ChatMessage, ChatReply, McpEndpoint, RunEvent, RunOutcome, RunStatus, RunUpdate, ToolSpec,
    next_turn,
};
use tokio::sync::{mpsc, oneshot};

use mcp::{ListedTool, ToolSession};

const SYSTEM: &str = "You help with one Teams message. Use the tools to read what you need and to propose a reply or a reaction. You never send anything yourself.";
const ROUNDS: usize = 8;

pub async fn run_chat(
    profile: agents::AgentProfile,
    prompt: String,
    endpoint: Option<McpEndpoint>,
    events: mpsc::UnboundedSender<RunUpdate>,
    mut stop: oneshot::Receiver<()>,
) -> RunOutcome {
    let session = match endpoint {
        Some(endpoint) => {
            match ToolSession::connect(&mcp::McpEndpoint {
                url: endpoint.url,
                token: endpoint.token,
            })
            .await
            {
                Ok(session) => Some(session),
                Err(error) => {
                    let _ = events.send(RunUpdate::Event(RunEvent::Step(format!(
                        "MCP tools unavailable: {error}"
                    ))));
                    None
                }
            }
        }
        None => None,
    };
    let specs = match &session {
        Some(session) => session
            .tools()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(spec)
            .collect(),
        None => Vec::new(),
    };
    let mut messages = vec![ChatMessage::user(prompt)];
    let mut stop_armed = true;
    for _ in 0..ROUNDS {
        let turn = tokio::select! {
            biased;
            result = &mut stop, if stop_armed => {
                if result.is_ok() {
                    return RunOutcome { status: RunStatus::Stopped, exit_code: None };
                }
                stop_armed = false;
                continue;
            }
            turn = next_turn(&profile, SYSTEM, &messages, &specs) => turn,
        };
        match turn {
            Err(message) => return failed(message),
            Ok(ChatReply::Text(text)) => {
                let _ = events.send(RunUpdate::Event(RunEvent::Text(text.clone())));
                return RunOutcome {
                    status: RunStatus::Completed { final_text: text },
                    exit_code: None,
                };
            }
            Ok(ChatReply::Tools { text, calls }) => {
                messages.push(ChatMessage::assistant(text, calls.clone()));
                for call in calls {
                    let _ = events.send(RunUpdate::Event(RunEvent::Step(call.name.clone())));
                    let result = match &session {
                        Some(session) => session
                            .call(&call.name, &call.arguments)
                            .await
                            .unwrap_or_else(|error| error),
                        None => "tools are not available".to_owned(),
                    };
                    messages.push(ChatMessage::tool(call.id, result));
                }
            }
        }
    }
    failed("stopped after too many tool calls".to_owned())
}

fn spec(tool: ListedTool) -> ToolSpec {
    ToolSpec {
        name: tool.name,
        description: tool.description,
        parameters: tool.parameters,
    }
}

fn failed(message: String) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Failed {
            message,
            stderr_tail: Vec::new(),
        },
        exit_code: None,
    }
}
