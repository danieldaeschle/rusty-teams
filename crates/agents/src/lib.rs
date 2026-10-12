mod action;
mod context;
mod invocation;
mod parse;
mod profile;
mod runner;
mod settings;
mod template;

pub use action::AgentAction;
pub use context::{MessageContext, ThreadMessage, build_prompt};
pub use invocation::{
    Invocation, InvocationError, McpEndpoint, RunPlan, build_invocation, kill_process_invocation,
    open_session_invocation, resume_invocation, supports_resume, version_check_invocation,
    wsl_path,
};
pub use parse::{OutputParser, RunEvent, parse_line};
pub use profile::{AgentProfile, Environment, OutputFormat};
pub use runner::{RunOutcome, RunStatus, RunUpdate, probe, run, run_stop_invocation};
pub use settings::{AgentSettings, SETTINGS_KEY, SettingsProblem};
pub use template::{UnknownPlaceholder, Values, placeholders_in, render, split_command};

pub fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
