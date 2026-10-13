use crate::action::AgentAction;
use crate::template::{Values, render};

const THREAD_LIMIT: usize = 20;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadMessage {
    pub sender: String,
    pub text: String,
    pub time: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageContext {
    pub sender: String,
    pub chat: String,
    pub conversation_id: String,
    pub message_id: String,
    pub message: String,
    pub thread: Vec<ThreadMessage>,
}

fn render_thread(thread: &[ThreadMessage]) -> String {
    if thread.is_empty() {
        return "(none)".to_string();
    }
    let skipped = thread.len().saturating_sub(THREAD_LIMIT);
    thread[skipped..]
        .iter()
        .map(|entry| format!("{} ({}): {}", entry.sender, entry.time, entry.text))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_prompt(
    action: &AgentAction,
    context: &MessageContext,
    extra: &str,
    session_id: &str,
    agent_id: &str,
) -> String {
    let values = Values {
        message: context.message.clone(),
        sender: context.sender.clone(),
        chat: context.chat.clone(),
        conversation_id: context.conversation_id.clone(),
        message_id: context.message_id.clone(),
        thread: render_thread(&context.thread),
        extra: extra.to_string(),
        session_id: session_id.to_string(),
        agent: agent_id.to_string(),
        ..Values::default()
    };
    let rendered =
        render(&action.prompt_template, &values).unwrap_or_else(|_| action.prompt_template.clone());
    rendered.trim().to_string()
}

pub fn build_session_prompt(
    context: &MessageContext,
    message: &str,
    session_id: &str,
    agent_id: &str,
) -> String {
    let quoted = context
        .message
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Context: {sender} wrote in the Teams chat \"{chat}\" (conversation_id \"{conversation_id}\", message_id \"{message_id}\"):\n\n{quoted}\n\nThe `teams` MCP server can show images of messages (`get_image`) and list the newest messages of other chats (`recent_messages`). To propose a reply, use the `propose_message` tool of the `teams` MCP server: conversation_id \"{conversation_id}\", reply_to_message_id \"{message_id}\", session_id \"{session_id}\", agent \"{agent_id}\". Never send anything yourself.\n\n{message}",
        sender = context.sender,
        chat = context.chat,
        conversation_id = context.conversation_id,
        message_id = context.message_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(thread_length: usize) -> MessageContext {
        MessageContext {
            sender: "Ada".to_string(),
            chat: "Ops".to_string(),
            conversation_id: "19:abc".to_string(),
            message_id: "42".to_string(),
            message: "Where is the report?".to_string(),
            thread: (0..thread_length)
                .map(|index| ThreadMessage {
                    sender: format!("User{index}"),
                    text: format!("text {index}"),
                    time: "10:00".to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_session_prompt_carries_the_message_ids_and_the_typed_text_last() {
        let prompt = build_session_prompt(&context(0), "and the tests?", "sid-9", "claude");
        assert!(prompt.starts_with("Context: Ada wrote in the Teams chat \"Ops\""));
        assert!(prompt.contains("conversation_id \"19:abc\", message_id \"42\""));
        assert!(prompt.contains("\n> Where is the report?\n"));
        assert!(prompt.contains("propose_message"));
        assert!(prompt.contains("session_id \"sid-9\", agent \"claude\""));
        assert!(prompt.ends_with("\n\nand the tests?"));
    }

    #[test]
    fn answer_prompt_renders_ids() {
        let prompt = build_prompt(
            &AgentAction::answer_this("claude"),
            &context(0),
            "",
            "sid-9",
            "claude",
        );
        assert!(
            prompt.starts_with("Ada asked me in the Teams chat \"Ops\":\n\nWhere is the report?")
        );
        assert!(prompt.contains("Earlier messages in this chat:\n(none)\n\n"));
        assert!(prompt.contains("conversation_id \"19:abc\", reply_to_message_id \"42\", session_id \"sid-9\", agent \"claude\"."));
        assert!(prompt.ends_with("Never send anything yourself."));
    }

    #[test]
    fn build_prompt_is_trimmed_and_takes_extra() {
        let prompt = build_prompt(
            &AgentAction::build_this("claude"),
            &context(1),
            "",
            "sid-9",
            "claude",
        );
        assert!(prompt.ends_with("agent \"claude\"."));
        assert!(prompt.contains("propose a short reply to Ada"));
        assert!(prompt.contains("User0 (10:00): text 0"));
        let with_extra = build_prompt(
            &AgentAction::build_this("claude"),
            &context(1),
            "Use Rust",
            "sid-9",
            "claude",
        );
        assert!(with_extra.ends_with("agent \"claude\". Use Rust"));
    }

    #[test]
    fn thread_keeps_last_twenty() {
        let prompt = build_prompt(
            &AgentAction::build_this("claude"),
            &context(25),
            "",
            "sid-9",
            "claude",
        );
        assert!(!prompt.contains("User4 "));
        assert!(prompt.contains("User5 (10:00)"));
        assert!(prompt.contains("User24 (10:00)"));
    }
}
