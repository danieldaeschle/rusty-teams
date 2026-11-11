use serde::{Deserialize, Serialize};

const CLAUDE_COMMAND: &str =
    "claude -p --output-format stream-json --verbose --session-id {session_id}";
const COPILOT_COMMAND: &str = "copilot -p {prompt} --output-format json --session-id {session_id}";
pub(crate) const LEGACY_CLAUDE_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e claude --resume {session_id}";
pub(crate) const LEGACY_COPILOT_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e copilot --resume {session_id}";
const CLAUDE_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e sh -lc \"exec claude --resume {session_id}\"";
const COPILOT_WSL_OPEN_SESSION: &str =
    "wt.exe wsl.exe --cd {cwd} -e sh -lc \"exec copilot --resume {session_id}\"";

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

/// Where a run is answered. Missing JSON stays a command, so older settings keep working.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentBackend {
    #[default]
    Command,
    OpenAi {
        base_url: String,
        model: String,
        #[serde(default)]
        api_key: String,
    },
    Anthropic {
        model: String,
        #[serde(default)]
        api_key: String,
    },
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
    pub backend: AgentBackend,
}

impl AgentProfile {
    pub fn is_command(&self) -> bool {
        matches!(self.backend, AgentBackend::Command)
    }

    pub fn is_chat(&self) -> bool {
        !self.is_command()
    }

    pub fn backend_label(&self) -> String {
        match &self.backend {
            AgentBackend::Command => String::new(),
            AgentBackend::OpenAi {
                base_url, model, ..
            } => {
                let model = if model.is_empty() { "no model" } else { model };
                format!("{model} · {base_url}")
            }
            AgentBackend::Anthropic { model, .. } => {
                let model = if model.is_empty() { "no model" } else { model };
                format!("Anthropic · {model}")
            }
        }
    }

    pub fn command_program(&self) -> &str {
        self.command.split_whitespace().next().unwrap_or("")
    }

    pub(crate) fn migrate_open_session_command(&mut self) {
        let replacement = match self.open_session_command.as_str() {
            LEGACY_CLAUDE_WSL_OPEN_SESSION => CLAUDE_WSL_OPEN_SESSION,
            LEGACY_COPILOT_WSL_OPEN_SESSION => COPILOT_WSL_OPEN_SESSION,
            _ => return,
        };
        self.open_session_command = replacement.to_string();
    }

    pub fn claude_wsl() -> Self {
        Self {
            id: "claude".to_string(),
            name: "Claude Code (WSL)".to_string(),
            environment: Environment::Wsl { distro: None },
            command: CLAUDE_COMMAND.to_string(),
            output: OutputFormat::ClaudeStreamJson,
            open_session_command: CLAUDE_WSL_OPEN_SESSION.to_string(),
            mcp_config_flag: "--mcp-config".to_string(),
            backend: AgentBackend::Command,
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
            backend: AgentBackend::Command,
        }
    }

    pub fn copilot_host() -> Self {
        Self {
            id: "copilot-host".to_string(),
            name: "Copilot CLI".to_string(),
            environment: Environment::Host,
            command: COPILOT_COMMAND.to_string(),
            output: OutputFormat::CopilotJsonl,
            open_session_command: "wt.exe -d {cwd} copilot --resume {session_id}".to_string(),
            mcp_config_flag: "--additional-mcp-config".to_string(),
            backend: AgentBackend::Command,
        }
    }

    pub fn codex_host() -> Self {
        Self {
            id: "codex".to_string(),
            name: "Codex CLI".to_string(),
            environment: Environment::Host,
            command: "codex exec --skip-git-repo-check {prompt}".to_string(),
            output: OutputFormat::Text,
            ..Self::default()
        }
    }

    pub fn cursor_host() -> Self {
        Self {
            id: "cursor".to_string(),
            name: "Cursor CLI".to_string(),
            environment: Environment::Host,
            command: "agent -p --trust --approve-mcps {prompt}".to_string(),
            output: OutputFormat::Text,
            ..Self::default()
        }
    }

    pub fn agy_host() -> Self {
        Self {
            id: "agy".to_string(),
            name: "Antigravity CLI".to_string(),
            environment: Environment::Host,
            command: "agy -p {prompt}".to_string(),
            output: OutputFormat::Text,
            ..Self::default()
        }
    }

    pub fn ollama() -> Self {
        Self::open_ai("ollama", "Ollama", "http://127.0.0.1:11434/v1", "", "")
    }

    pub fn openai() -> Self {
        Self::open_ai(
            "openai",
            "OpenAI",
            "https://api.openai.com/v1",
            "gpt-4o",
            "",
        )
    }

    pub fn openrouter() -> Self {
        Self::open_ai(
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "openrouter/auto",
            "",
        )
    }

    pub fn openai_compatible() -> Self {
        Self::open_ai(
            "openai-compatible",
            "OpenAI-compatible",
            "http://127.0.0.1:8080/v1",
            "",
            "",
        )
    }

    pub fn anthropic() -> Self {
        Self {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            backend: AgentBackend::Anthropic {
                model: "claude-sonnet-4-5".to_string(),
                api_key: String::new(),
            },
            ..Self::default()
        }
    }

    fn open_ai(id: &str, name: &str, base_url: &str, model: &str, api_key: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            backend: AgentBackend::OpenAi {
                base_url: base_url.to_string(),
                model: model.to_string(),
                api_key: api_key.to_string(),
            },
            ..Self::default()
        }
    }

    pub fn copilot_wsl() -> Self {
        Self {
            id: "copilot".to_string(),
            name: "Copilot CLI (WSL)".to_string(),
            environment: Environment::Wsl { distro: None },
            command: COPILOT_COMMAND.to_string(),
            output: OutputFormat::CopilotJsonl,
            open_session_command: COPILOT_WSL_OPEN_SESSION.to_string(),
            mcp_config_flag: "--additional-mcp-config".to_string(),
            backend: AgentBackend::Command,
        }
    }
}
