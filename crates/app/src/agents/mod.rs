mod agents_panel;
mod demo;
mod kills;
mod lanes;
mod mcp_bridge;
mod message_context;
mod run_chip;
mod runs;
mod session_options;
mod settings_controls;
mod settings_forms;
mod settings_problems;
mod settings_section;
mod spinner;
mod start_dialog;
mod start_dialog_pickers;
mod store;

use std::path::PathBuf;
use std::time::Duration;

use agents::{AgentAction, Environment};
use directories::ProjectDirs;
use gpui_kit::*;

pub use agents_panel::{AgentsPanel, AgentsPanelEvent};
pub use run_chip::run_chip;
pub use runs::{AgentRun, AgentRuns, AgentRunsEvent, AgentRunsHandle, RunState, StartOptions};
pub use settings_section::AgentsSettings;
pub use start_dialog::{StartDialog, StartDialogEvent, StartRequest};

use crate::app_state::{AppHandle, AppState};

pub fn install(app: Entity<AppState>, cx: &mut App) {
    let runs = cx.new(|cx| AgentRuns::new(app, cx));
    cx.set_global(AgentRunsHandle(runs));
}

const MCP_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MCP_WAIT_LIMIT: Duration = Duration::from_secs(5);
const MCP_CONFIG_FOLDER: &str = "agent-mcp";

fn mcp_config_directory() -> Option<PathBuf> {
    ProjectDirs::from("", "", ::store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(MCP_CONFIG_FOLDER))
}

pub fn runs_handle(cx: &App) -> Option<Entity<AgentRuns>> {
    cx.try_global::<AgentRunsHandle>()
        .map(|handle| handle.0.clone())
}

pub fn agents_enabled(cx: &App) -> bool {
    runs_handle(cx).is_some_and(|runs| !runs.read(cx).settings().actions.is_empty())
}

fn raise_notice(text: String, cx: &mut App) {
    let Some(handle) = cx.try_global::<AppHandle>() else {
        return;
    };
    handle
        .0
        .clone()
        .update(cx, |state, cx| state.raise_notice(text, None, cx));
}

pub fn start_run(
    action_id: &str,
    conversation_id: &str,
    message_id: &str,
    extra: &str,
    options: StartOptions,
    cx: &mut App,
) {
    let Some(runs) = runs_handle(cx) else {
        return;
    };
    let started = runs.update(cx, |runs, cx| {
        runs.start_with(action_id, conversation_id, message_id, extra, options, cx)
    });
    if let Err(error) = started {
        raise_notice(format!("Agent could not start: {error}"), cx);
    }
}

pub fn start_run_when_mcp_ready(
    action_id: String,
    conversation_id: String,
    message_id: String,
    extra: String,
    options: StartOptions,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let mut waited = Duration::ZERO;
        while waited < MCP_WAIT_LIMIT && !cx.update(|cx| mcp_bridge::endpoint(cx).is_some()) {
            cx.background_executor().timer(MCP_POLL_INTERVAL).await;
            waited += MCP_POLL_INTERVAL;
        }
        cx.update(|cx| {
            if mcp_bridge::endpoint(cx).is_none() {
                raise_notice(
                    "MCP server is not running - the agent cannot propose a reply".to_owned(),
                    cx,
                );
            }
            start_run(
                &action_id,
                &conversation_id,
                &message_id,
                &extra,
                options,
                cx,
            );
        });
    })
    .detach();
}

pub fn retry_run(run_id: u64, cx: &mut App) {
    let Some(runs) = runs_handle(cx) else {
        return;
    };
    let retried = runs.update(cx, |runs, cx| runs.retry(run_id, cx));
    if let Err(error) = retried {
        raise_notice(format!("Agent could not start: {error}"), cx);
    }
}

pub fn delegate(request: StartRequest, cx: &mut App) {
    let Some(runs) = runs_handle(cx) else {
        return;
    };
    let action = runs
        .read(cx)
        .settings()
        .actions
        .iter()
        .find(|action| action.id == request.action_id)
        .cloned();
    let Some(action) = action else {
        return;
    };
    let allowed = mcp_bridge::chat_allowed(&request.conversation_id, cx);
    if start_dialog::needs_dialog(&action, allowed) {
        runs.update(cx, |_, cx| cx.emit(AgentRunsEvent::StartRequested(request)));
    } else {
        start_run(
            &request.action_id,
            &request.conversation_id,
            &request.message_id,
            "",
            StartOptions::default(),
            cx,
        );
    }
}

pub fn open_settings(cx: &mut App) {
    if let Some(runs) = runs_handle(cx) {
        runs.update(cx, |_, cx| cx.emit(AgentRunsEvent::OpenSettings));
    }
}

pub fn has_session(session_id: &str, cx: &App) -> bool {
    runs_handle(cx).is_some_and(|runs| runs.read(cx).by_session(session_id).is_some())
}

pub fn open_session(session_id: &str, cx: &mut App) {
    let run = runs_handle(cx).and_then(|runs| runs.read(cx).by_session(session_id).cloned());
    match run {
        Some(run) => open_run_session(&run, cx),
        None => raise_notice(
            "This session was not started from this app, open it in your terminal".to_owned(),
            cx,
        ),
    }
}

fn open_run_session(run: &AgentRun, cx: &mut App) {
    let Some(handle) = cx.try_global::<AppHandle>() else {
        return;
    };
    let settings = store::load_settings(&handle.0.read(cx).store);
    let Some(profile) = settings
        .agents
        .iter()
        .find(|profile| profile.id == run.agent_id)
    else {
        raise_notice("The agent of this run no longer exists".to_owned(), cx);
        return;
    };
    let action = AgentAction {
        working_dir: run.working_dir.clone(),
        ..AgentAction::default()
    };
    let spawned = agents::open_session_invocation(profile, &action, &run.session_id)
        .map_err(|error| error.to_string())
        .and_then(|invocation| {
            std::process::Command::new(&invocation.program)
                .args(&invocation.args)
                .spawn()
                .map(drop)
                .map_err(|error| error.to_string())
        });
    if let Err(error) = spawned {
        raise_notice(format!("Could not open the session: {error}"), cx);
    }
}

pub fn environment_label(environment: &Environment) -> String {
    match environment {
        Environment::Host => "Windows".to_owned(),
        Environment::Wsl { distro: None } => "WSL".to_owned(),
        Environment::Wsl {
            distro: Some(distro),
        } => format!("WSL - {distro}"),
    }
}
