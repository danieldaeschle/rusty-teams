use serde::{Deserialize, Serialize};

const CLAUDE_COMMAND: &str =
    "claude -p --output-format stream-json --verbose --session-id {session_id}";
const COPILOT_COMMAND: &str = "copilot -p {prompt} --output-format json --session-id {session_id}";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Environment {
    #[default]
    Host,
    Wsl {
        distro: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    #[default]
    ClaudeStreamJson,
    CopilotJsonl,
    Text,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub environment: Environment,
    pub command: String,
    pub output: OutputFormat,
    pub open_session_command: String,
    pub mcp_config_flag: String,
}

impl AgentProfile {
    pub fn claude_wsl() -> Self {
        Self {
            id: "claude".to_string(),
            name: "Claude Code (WSL)".to_string(),
            environment: Environment::Wsl { distro: None },
            command: CLAUDE_COMMAND.to_string(),
            output: OutputFormat::ClaudeStreamJson,
            open_session_command: "wt.exe wsl.exe --cd {cwd} -e claude --resume {session_id}"
                .to_string(),
            mcp_config_flag: "--mcp-config".to_string(),
        }
    }

    pub fn claude_host() -> Self {
        Self {
            id: "claude-host".to_string(),
            name: "Claude Code".to_string(),
            environment: Environment::Host,
            command: CLAUDE_COMMAND.to_string(),
            output: OutputFormat::ClaudeStreamJson,
            open_session_command: "wt.exe -d {cwd} claude --resume {session_id}".to_string(),
            mcp_config_flag: "--mcp-config".to_string(),
        }
    }

    pub fn copilot_wsl() -> Self {
        Self {
            id: "copilot".to_string(),
            name: "Copilot CLI (WSL)".to_string(),
            environment: Environment::Wsl { distro: None },
            command: COPILOT_COMMAND.to_string(),
            output: OutputFormat::CopilotJsonl,
            open_session_command: "wt.exe wsl.exe --cd {cwd} -e copilot --resume {session_id}"
                .to_string(),
            mcp_config_flag: "--additional-mcp-config".to_string(),
        }
    }
}
