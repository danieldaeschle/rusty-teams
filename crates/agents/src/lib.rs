mod action;
mod context;
mod invocation;
mod parse;
mod profile;
mod runner;
mod sessions;
mod settings;
mod template;

pub use action::AgentAction;
pub use context::{MessageContext, ThreadMessage, build_prompt, build_session_prompt};
pub use invocation::{
    Invocation, InvocationError, McpEndpoint, RunPlan, build_invocation, fork_invocation,
    kill_process_invocation, linux_path, open_session_invocation, resume_invocation,
    supports_resume, version_check_invocation, wsl_path, wsl_stop_invocation, wsl_unc_distro,
};
pub use parse::{OutputParser, RunEvent, parse_line};
pub use profile::{AgentProfile, Environment, OutputFormat};
pub use runner::{
    RunOutcome, RunStatus, RunUpdate, probe, run, run_stop_invocation, run_stop_invocation_within,
};
pub use sessions::{
    ExternalSession, encode_project_dir, live_session_names, resolve_project_dir,
    scan_external_sessions,
};
pub use settings::{AgentSettings, RECENT_WORKING_DIR_LIMIT, SETTINGS_KEY, SettingsProblem};
pub use template::{UnknownPlaceholder, Values, placeholders_in, render, split_command};

pub fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
