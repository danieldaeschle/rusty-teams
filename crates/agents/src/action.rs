use serde::{Deserialize, Serialize};

const ANSWER_TEMPLATE: &str = "{sender} asked me in the Teams chat \"{chat}\":\n\n{message}\n\nEarlier messages in this chat:\n{thread}\n\nResearch what is needed, then propose my reply with the `propose_message` tool of the `teams` MCP server: conversation_id \"{conversation_id}\", reply_to_message_id \"{message_id}\", session_id \"{session_id}\", agent \"{agent}\". Never send anything yourself. {extra}";
const BUILD_TEMPLATE: &str = "{sender} asked me in the Teams chat \"{chat}\" to build something:\n\n{message}\n\nEarlier messages in this chat:\n{thread}\n\nBuild what is asked in this repository. When done, propose a short reply to {sender} with the `propose_message` tool of the `teams` MCP server: conversation_id \"{conversation_id}\", reply_to_message_id \"{message_id}\", session_id \"{session_id}\", agent \"{agent}\". {extra}";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentAction {
    pub id: String,
    pub label: String,
    pub agent_id: String,
    pub working_dir: String,
    pub prompt_template: String,
    pub ask_extra_instructions: bool,
    pub extra_args: String,
}

impl AgentAction {
    pub fn answer_this(agent_id: &str) -> Self {
        Self {
            id: "answer-this".to_string(),
            label: "Answer this".to_string(),
            agent_id: agent_id.to_string(),
            working_dir: "~".to_string(),
            prompt_template: ANSWER_TEMPLATE.to_string(),
            ask_extra_instructions: false,
            extra_args: "--allowedTools WebSearch,WebFetch,Read,Grep,Glob,mcp__teams".to_string(),
        }
    }

    pub fn build_this(agent_id: &str) -> Self {
        Self {
            id: "build-this".to_string(),
            label: "Build this".to_string(),
            agent_id: agent_id.to_string(),
            working_dir: "~".to_string(),
            prompt_template: BUILD_TEMPLATE.to_string(),
            ask_extra_instructions: true,
            extra_args: "--permission-mode auto".to_string(),
        }
    }
}
