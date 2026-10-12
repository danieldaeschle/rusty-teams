use serde::{Deserialize, Serialize};

use crate::action::AgentAction;
use crate::profile::AgentProfile;
use crate::template::{is_known_placeholder, placeholders_in};

pub const SETTINGS_KEY: &str = "agents.settings";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsProblem {
    #[error("action {action_id} references unknown agent {agent_id}")]
    UnknownAgent { action_id: String, agent_id: String },
    #[error("agent {agent_id} has an empty command")]
    EmptyCommand { agent_id: String },
    #[error("{item_id} uses unknown placeholder {{{placeholder}}}")]
    UnknownPlaceholder {
        item_id: String,
        placeholder: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub agents: Vec<AgentProfile>,
    pub actions: Vec<AgentAction>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            agents: vec![AgentProfile::claude_wsl(), AgentProfile::copilot_wsl()],
            actions: vec![
                AgentAction::answer_this("claude"),
                AgentAction::build_this("claude"),
            ],
        }
    }
}

fn check_placeholders(item_id: &str, texts: &[&str], problems: &mut Vec<SettingsProblem>) {
    for text in texts {
        for placeholder in placeholders_in(text) {
            if !is_known_placeholder(&placeholder) {
                problems.push(SettingsProblem::UnknownPlaceholder {
                    item_id: item_id.to_string(),
                    placeholder,
                });
            }
        }
    }
}

impl AgentSettings {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn validate(&self) -> Vec<SettingsProblem> {
        let mut problems = Vec::new();
        for agent in &self.agents {
            if agent.command.trim().is_empty() {
                problems.push(SettingsProblem::EmptyCommand {
                    agent_id: agent.id.clone(),
                });
            }
            check_placeholders(
                &agent.id,
                &[&agent.command, &agent.open_session_command],
                &mut problems,
            );
        }
        for action in &self.actions {
            if !self.agents.iter().any(|agent| agent.id == action.agent_id) {
                problems.push(SettingsProblem::UnknownAgent {
                    action_id: action.id.clone(),
                    agent_id: action.agent_id.clone(),
                });
            }
            check_placeholders(
                &action.id,
                &[&action.prompt_template, &action.extra_args],
                &mut problems,
            );
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_and_is_valid() {
        let settings = AgentSettings::default();
        let loaded = AgentSettings::from_json(&settings.to_json()).unwrap();
        assert_eq!(loaded, settings);
        assert!(settings.validate().is_empty());
    }

    #[test]
    fn empty_object_loads_defaults() {
        assert_eq!(
            AgentSettings::from_json("{}").unwrap(),
            AgentSettings::default()
        );
    }

    #[test]
    fn partial_json_fills_missing_fields() {
        let loaded = AgentSettings::from_json(
            r#"{"agents":[{"id":"x","command":"x {session_id}"}],"actions":[{"id":"a","agent_id":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(loaded.agents.len(), 1);
        assert_eq!(
            loaded.agents[0].output,
            crate::OutputFormat::ClaudeStreamJson
        );
        assert_eq!(loaded.actions[0].working_dir, "");
        let only_actions = AgentSettings::from_json(r#"{"actions":[]}"#).unwrap();
        assert_eq!(only_actions.agents.len(), 2);
    }

    #[test]
    fn validate_finds_problems() {
        let mut settings = AgentSettings::default();
        settings.actions[0].agent_id = "ghost".to_string();
        settings.agents[1].command = String::new();
        settings.agents[0].command.push_str(" {bogus}");
        settings.actions[1].prompt_template.push_str(" {nope}");
        let problems = settings.validate();
        assert!(problems.contains(&SettingsProblem::UnknownAgent {
            action_id: "answer-this".to_string(),
            agent_id: "ghost".to_string(),
        }));
        assert!(problems.contains(&SettingsProblem::EmptyCommand {
            agent_id: "copilot".to_string(),
        }));
        assert!(problems.contains(&SettingsProblem::UnknownPlaceholder {
            item_id: "claude".to_string(),
            placeholder: "bogus".to_string(),
        }));
        assert!(problems.contains(&SettingsProblem::UnknownPlaceholder {
            item_id: "build-this".to_string(),
            placeholder: "nope".to_string(),
        }));
    }
}
