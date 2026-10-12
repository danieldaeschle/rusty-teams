use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use store::MessageRecord;

use super::runs::{AgentRun, RunState};
use crate::app_state::{AppState, Selection};
use crate::rows::message_text;

const DEMO_RUN_COUNT: usize = 3;

fn run_for(
    id: u64,
    record: &MessageRecord,
    label: &str,
    state: RunState,
    steps: &[(u64, &str)],
    elapsed_seconds: i64,
) -> AgentRun {
    let started_at = Utc::now() - ChronoDuration::seconds(elapsed_seconds);
    let finished = state != RunState::Running;
    let current_step = match &state {
        RunState::Done => "Done - reply proposed".to_owned(),
        RunState::Failed { message, .. } => message.clone(),
        _ => steps
            .last()
            .map(|(_, text)| (*text).to_owned())
            .unwrap_or_default(),
    };
    AgentRun {
        id,
        action_id: label.to_lowercase().replace(' ', "-"),
        action_label: label.to_owned(),
        agent_id: "claude".to_owned(),
        agent_name: "Claude Code (WSL)".to_owned(),
        conversation_id: record.conversation_id.clone(),
        message_id: record.message_id.clone(),
        chat_title: "Release".to_owned(),
        sender_name: record.sender_name.clone().unwrap_or_default(),
        session_id: format!("demo-session-{id}"),
        working_dir: "~".to_owned(),
        extra: message_text(record),
        state,
        current_step,
        steps: steps
            .iter()
            .map(|(seconds, text)| (Duration::from_secs(*seconds), (*text).to_owned()))
            .collect(),
        final_text: "Group the shift report by plant, then export it.".to_owned(),
        started_at,
        ended_at: finished.then(|| started_at + ChronoDuration::seconds(elapsed_seconds)),
        expanded: false,
        dismissed: false,
    }
}

pub fn runs(app: &AppState) -> Vec<AgentRun> {
    let (Selection::Chat(conversation_id) | Selection::Channel(conversation_id)) =
        crate::demo::first_selection();
    let records = app
        .store
        .messages(&conversation_id, None, DEMO_RUN_COUNT)
        .unwrap_or_default();
    let [done, failed, running] = records.as_slice() else {
        return Vec::new();
    };
    vec![
        run_for(
            1,
            done,
            "Answer this",
            RunState::Done,
            &[(4, "Read ShiftReport.cs")],
            118,
        ),
        run_for(
            2,
            failed,
            "Build this",
            RunState::Failed {
                message: "claude exited 1: Not logged in".to_owned(),
                log: vec!["Not logged in".to_owned(), "Run claude /login".to_owned()],
            },
            &[],
            3,
        ),
        run_for(
            3,
            running,
            "Build this",
            RunState::Running,
            &[
                (4, "Read ShiftReportService.cs"),
                (72, "Searched \"ExportFormat\""),
                (190, "Edited ShiftReportExport.cs"),
                (391, "Running dotnet test --filter ShiftReport"),
            ],
            391,
        ),
    ]
}
