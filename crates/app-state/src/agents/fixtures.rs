use std::time::Duration;

use chrono::{TimeZone, Utc};

use super::runs::{AgentRun, RunState};

pub fn finished_run(id: u64) -> AgentRun {
    AgentRun {
        id,
        action_id: "answer-this".to_owned(),
        action_label: "Answer this".to_owned(),
        agent_id: "claude".to_owned(),
        agent_name: "Claude Code".to_owned(),
        conversation_id: "19:chat".to_owned(),
        message_id: format!("m{id}"),
        chat_title: "Ops".to_owned(),
        sender_name: "Ada".to_owned(),
        session_id: format!("session-{id}"),
        working_dir: "~".to_owned(),
        extra: String::new(),
        state: RunState::Done,
        current_step: "Done".to_owned(),
        steps: vec![(Duration::from_secs(3), "Read a.rs".to_owned())],
        final_text: "ok".to_owned(),
        started_at: Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
        ended_at: Some(Utc.with_ymd_and_hms(2026, 10, 1, 9, 1, 0).unwrap()),
        expanded: false,
        dismissed: false,
        process_id: None,
        pending_prompt: None,
        sent_to: None,
        launch_session_id: None,
        launched: true,
    }
}
