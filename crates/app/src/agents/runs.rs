use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use agents::{
    AgentAction, AgentProfile, AgentSettings, McpEndpoint, RunEvent, RunOutcome, RunStatus,
    RunUpdate,
};
use chrono::{DateTime, Utc};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use super::lanes::{Admit, Lanes, RestoreInfo};
use super::{
    MCP_POLL_INTERVAL, MCP_WAIT_LIMIT, StartRequest, demo, kills, mcp_bridge, mcp_config_directory,
    message_context, open_run_session, raise_notice, store as run_store,
};
use crate::app_state::AppState;
use crate::notice::truncated;
use crate::runtime;

const STEP_LIMIT: usize = 50;
const VISIBLE_STEPS: usize = 6;
const VISIBLE_LOG_LINES: usize = 20;
const TEXT_STEP_CHARS: usize = 80;
const DONE_STEP_CHARS: usize = 60;
const TICK: Duration = Duration::from_secs(1);
const PROGRESS_PERSIST_INTERVAL: Duration = Duration::from_secs(5);
const RESUME_PARALLEL_LIMIT: usize = 3;
const QUEUED_STEP: &str = "Queued";
const OPENED_NOTICE: &str = "Session opened in a terminal - send cancelled";
const RESUMED_STEP: &str = "Resumed after restart";
const RESUME_PROMPT: &str = "The app was restarted. Continue where you stopped.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunState {
    Running,
    Done,
    Failed { message: String, log: Vec<String> },
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: u64,
    pub action_id: String,
    pub action_label: String,
    pub agent_id: String,
    pub agent_name: String,
    pub conversation_id: String,
    pub message_id: String,
    pub chat_title: String,
    pub sender_name: String,
    pub session_id: String,
    pub working_dir: String,
    pub extra: String,
    pub state: RunState,
    pub current_step: String,
    pub steps: Vec<(Duration, String)>,
    pub final_text: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub expanded: bool,
    pub dismissed: bool,
    #[serde(default)]
    pub process_id: Option<u32>,
    #[serde(default)]
    pub pending_prompt: Option<String>,
    #[serde(default)]
    pub sent_to: Option<SessionTarget>,
    #[serde(default)]
    pub launch_session_id: Option<String>,
    #[serde(default = "launched_by_default")]
    pub launched: bool,
}

fn launched_by_default() -> bool {
    true
}

impl AgentRun {
    pub fn is_running(&self) -> bool {
        self.state == RunState::Running
    }

    pub fn is_queued(&self) -> bool {
        self.pending_prompt.is_some()
    }

    pub fn is_finished(&self) -> bool {
        !self.is_running()
    }

    pub fn elapsed(&self) -> Duration {
        let end = self.ended_at.unwrap_or_else(Utc::now);
        (end - self.started_at).to_std().unwrap_or_default()
    }

    pub fn elapsed_text(&self) -> String {
        format_elapsed(self.elapsed())
    }

    pub fn visible_steps(&self) -> &[(Duration, String)] {
        let skipped = self.steps.len().saturating_sub(VISIBLE_STEPS);
        &self.steps[skipped..]
    }

    pub fn log_lines(&self) -> &[String] {
        match &self.state {
            RunState::Failed { log, .. } => {
                let skipped = log.len().saturating_sub(VISIBLE_LOG_LINES);
                &log[skipped..]
            }
            _ => &[],
        }
    }

    pub fn layout_key(&self) -> (usize, bool) {
        let rows = match (&self.state, self.expanded) {
            (_, false) => 0,
            (RunState::Failed { .. }, true) => self.log_lines().len().max(1),
            (_, true) => self.visible_steps().len().max(1),
        };
        (rows, self.is_running())
    }

    fn mark_interrupted(&mut self) {
        let message = "Interrupted - the app was closed during the run".to_owned();
        self.current_step = message.clone();
        self.state = RunState::Failed {
            message,
            log: Vec::new(),
        };
        self.ended_at = Some(Utc::now());
    }

    fn push_step(&mut self, text: String) {
        let elapsed = (Utc::now() - self.started_at).to_std().unwrap_or_default();
        self.steps.push((elapsed, text.clone()));
        if self.steps.len() > STEP_LIMIT {
            self.steps.remove(0);
        }
        self.current_step = text;
    }
}

fn can_resume(settings: &AgentSettings, run: &AgentRun) -> bool {
    settings.resume_interrupted_runs
        && run.launched
        && !run.session_id.is_empty()
        && settings
            .agents
            .iter()
            .find(|agent| agent.id == run.agent_id)
            .is_some_and(agents::supports_resume)
}

fn remove_stale_mcp_configs() {
    let Some(directory) = mcp_config_directory() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}

pub fn format_elapsed(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn api_retry_text(message: &str, retry_in: Option<Duration>) -> String {
    let Some(retry_in) = retry_in else {
        return format!("API not responding - retrying ({message})");
    };
    let seconds = retry_in.as_secs();
    let wait = if seconds >= 60 {
        format!("{} min", seconds.div_ceil(60))
    } else {
        format!("{seconds} s")
    };
    format!("API not responding - retrying in {wait}")
}

fn first_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTarget {
    pub session_id: String,
    pub working_dir: String,
    pub fork: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartOptions {
    pub working_dir: Option<String>,
    pub remember_working_dir: bool,
    pub session: Option<SessionTarget>,
}

fn plan_for(
    profile: &AgentProfile,
    action: &AgentAction,
    prompt: &str,
    session_id: &str,
    target: Option<&SessionTarget>,
    endpoint: Option<&McpEndpoint>,
) -> Result<agents::RunPlan, agents::InvocationError> {
    let directory = mcp_config_directory();
    match target {
        None => agents::build_invocation(
            profile,
            action,
            prompt,
            session_id,
            endpoint,
            directory.as_deref(),
        ),
        Some(target) if target.fork => agents::fork_invocation(
            profile,
            action,
            prompt,
            &target.session_id,
            session_id,
            endpoint,
            directory.as_deref(),
        ),
        Some(_) => agents::resume_invocation(
            profile,
            action,
            prompt,
            session_id,
            endpoint,
            directory.as_deref(),
        ),
    }
}

pub struct AgentRunsHandle(pub Entity<AgentRuns>);

impl Global for AgentRunsHandle {}

pub enum AgentRunsEvent {
    Finished(u64),
    StartRequested(StartRequest),
    OpenSettings,
}

pub struct AgentRuns {
    app: Entity<AppState>,
    settings: AgentSettings,
    runs: Vec<AgentRun>,
    stops: HashMap<u64, oneshot::Sender<()>>,
    resume_queue: VecDeque<u64>,
    lanes: Lanes,
    startup_kills: Vec<(String, agents::Invocation)>,
    resuming: HashSet<u64>,
    stopping: HashSet<u64>,
    open_after_stop: HashSet<u64>,
    open_deferred: HashSet<u64>,
    next_id: u64,
    ticking: bool,
    progress_saved_at: Option<Instant>,
    progress_unsaved: bool,
}

impl EventEmitter<AgentRunsEvent> for AgentRuns {}

impl AgentRuns {
    pub fn new(app: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let store = app.read(cx).store.clone();
        let settings = run_store::load_settings(&store);
        let mut runs = run_store::load_runs(&store);
        let interrupted = runs.iter().any(AgentRun::is_running);
        let infos: Vec<RestoreInfo> = runs
            .iter()
            .filter(|run| run.is_running())
            .map(|run| RestoreInfo {
                id: run.id,
                session_id: run.session_id.clone(),
                queued: run.is_queued(),
                kill: kills::startup_kill(&settings, run),
            })
            .collect();
        let restored = Lanes::restore(&infos, |run_id| {
            runs.iter()
                .find(|run| run.id == run_id)
                .is_some_and(|run| can_resume(&settings, run))
        });
        for run in runs.iter_mut() {
            if restored.resume_queue.contains(&run.id) {
                run.push_step(RESUMED_STEP.to_owned());
            } else if restored.interrupted.contains(&run.id) {
                run.mark_interrupted();
            }
        }
        let (lanes, resume_queue, startup_kills) = (
            restored.lanes,
            VecDeque::from(restored.resume_queue),
            restored.kills,
        );
        if interrupted {
            run_store::save_runs(&store, &runs);
        }
        remove_stale_mcp_configs();
        if runs.is_empty() && app.read(cx).mode.demo {
            runs = demo::runs(app.read(cx));
        }
        let next_id = runs.iter().map(|run| run.id).max().map_or(1, |id| id + 1);
        let mut agent_runs = AgentRuns {
            app,
            settings,
            runs,
            stops: HashMap::new(),
            resume_queue,
            lanes,
            startup_kills,
            resuming: HashSet::new(),
            stopping: HashSet::new(),
            open_after_stop: HashSet::new(),
            open_deferred: HashSet::new(),
            next_id,
            ticking: false,
            progress_saved_at: None,
            progress_unsaved: false,
        };
        agent_runs.ensure_ticking(cx);
        agent_runs.start_resumes(cx);
        agent_runs
    }

    fn start_resumes(&mut self, cx: &mut Context<Self>) {
        if self.lanes.is_empty() {
            return;
        }
        let needs_mcp = self.runs.iter().any(|run| {
            run.is_running()
                && (self.resume_queue.contains(&run.id) || run.is_queued())
                && self
                    .settings
                    .agents
                    .iter()
                    .find(|agent| agent.id == run.agent_id)
                    .is_some_and(|agent| !agent.mcp_config_flag.is_empty())
        });
        cx.spawn(async move |this, cx| {
            let mut waited = Duration::ZERO;
            while needs_mcp
                && waited < MCP_WAIT_LIMIT
                && !cx.update(|cx| mcp_bridge::endpoint(cx).is_some())
            {
                cx.background_executor().timer(MCP_POLL_INTERVAL).await;
                waited += MCP_POLL_INTERVAL;
            }
            this.update(cx, |runs, cx| {
                runs.resume_next(cx);
                for (session_id, kill) in std::mem::take(&mut runs.startup_kills) {
                    runs.run_kill(&session_id, Some(kill), cx);
                }
                for session_id in runs.lanes.idle_sessions() {
                    runs.release(&session_id, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn resume_next(&mut self, cx: &mut Context<Self>) {
        while self.resuming.len() < RESUME_PARALLEL_LIMIT {
            let Some(run_id) = self.resume_queue.pop_front() else {
                return;
            };
            self.begin_resume(run_id, cx);
        }
    }

    fn resume_plan(
        &self,
        run: &AgentRun,
        cx: &App,
    ) -> Result<(agents::RunPlan, agents::OutputFormat), String> {
        let action = self
            .settings
            .actions
            .iter()
            .find(|action| action.id == run.action_id)
            .cloned()
            .ok_or("the action no longer exists")?;
        let profile = self
            .settings
            .agents
            .iter()
            .find(|profile| profile.id == run.agent_id)
            .ok_or("the agent no longer exists")?;
        let action = AgentAction {
            working_dir: run.working_dir.clone(),
            ..action
        };
        let plan = agents::resume_invocation(
            profile,
            &action,
            RESUME_PROMPT,
            &run.session_id,
            mcp_bridge::endpoint(cx).as_ref(),
            mcp_config_directory().as_deref(),
        )
        .map_err(|error| error.to_string())?;
        Ok((plan, profile.output))
    }

    fn begin_resume(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let Some(run) = self.get(run_id).cloned() else {
            return;
        };
        if !run.is_running() {
            self.finish_kill(&run.session_id, cx);
            return;
        }
        self.resuming.insert(run_id);
        let session_id = run.session_id.clone();
        match self.resume_plan(&run, cx) {
            Ok((plan, format)) => {
                let stop = kills::kill_for(&run, &plan, |launched| {
                    self.resume_plan(launched, cx).ok().map(|(plan, _)| plan)
                });
                cx.spawn(async move |this, cx| {
                    if let Some(stop) = stop {
                        kills::execute(stop).await;
                    }
                    this.update(cx, |runs, cx| {
                        let still_resuming = runs.resuming.contains(&run_id)
                            && runs.get(run_id).is_some_and(AgentRun::is_running);
                        runs.finish_kill(&session_id, cx);
                        if still_resuming {
                            runs.spawn(run_id, plan, format, cx);
                        } else {
                            for path in &plan.cleanup {
                                let _ = std::fs::remove_file(path);
                            }
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Err(reason) => {
                let stop = kills::startup_kill(&self.settings, &run);
                self.finish(
                    run_id,
                    Some(RunOutcome {
                        status: RunStatus::Failed {
                            message: format!("Could not resume - {reason}"),
                            stderr_tail: Vec::new(),
                        },
                        exit_code: None,
                    }),
                    cx,
                );
                self.run_kill(&session_id, stop, cx);
            }
        }
    }

    pub fn settings(&self) -> &AgentSettings {
        &self.settings
    }

    pub fn update_settings(
        &mut self,
        change: impl FnOnce(&mut AgentSettings),
        cx: &mut Context<Self>,
    ) {
        change(&mut self.settings);
        run_store::save_settings(&self.app.read(cx).store, &self.settings);
        cx.notify();
    }

    pub fn get(&self, run_id: u64) -> Option<&AgentRun> {
        self.runs.iter().find(|run| run.id == run_id)
    }

    pub fn by_session(&self, session_id: &str) -> Option<&AgentRun> {
        self.runs
            .iter()
            .rev()
            .find(|run| run.session_id == session_id)
    }

    pub fn for_conversation<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> impl Iterator<Item = &'a AgentRun> {
        self.runs
            .iter()
            .filter(move |run| run.conversation_id == conversation_id && !run.dismissed)
    }

    pub fn for_agent(&self, agent_id: &str) -> Vec<&AgentRun> {
        self.runs
            .iter()
            .rev()
            .filter(|run| run.agent_id == agent_id)
            .collect()
    }

    pub fn session_ids(&self) -> HashSet<String> {
        self.runs.iter().map(|run| run.session_id.clone()).collect()
    }

    pub fn running_count(&self) -> usize {
        self.runs.iter().filter(|run| run.is_running()).count()
    }

    pub fn recent(&self, limit: usize) -> Vec<&AgentRun> {
        self.runs
            .iter()
            .rev()
            .filter(|run| !run.dismissed)
            .take(limit)
            .collect()
    }

    fn run_mut(&mut self, run_id: u64) -> Option<&mut AgentRun> {
        self.runs.iter_mut().find(|run| run.id == run_id)
    }

    fn persist(&mut self, cx: &App) {
        run_store::save_runs(&self.app.read(cx).store, &self.runs);
        self.progress_saved_at = Some(Instant::now());
        self.progress_unsaved = false;
    }

    fn persist_progress(&mut self, cx: &App) {
        let due = self
            .progress_saved_at
            .is_none_or(|saved_at| saved_at.elapsed() >= PROGRESS_PERSIST_INTERVAL);
        if !due {
            self.progress_unsaved = true;
            return;
        }
        self.persist(cx);
    }

    pub fn start(
        &mut self,
        action_id: &str,
        conversation_id: &str,
        message_id: &str,
        extra: &str,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        self.start_with(
            action_id,
            conversation_id,
            message_id,
            extra,
            StartOptions::default(),
            cx,
        )
    }

    pub fn start_with(
        &mut self,
        action_id: &str,
        conversation_id: &str,
        message_id: &str,
        extra: &str,
        options: StartOptions,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let mut action = self
            .settings
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .cloned()
            .ok_or("This action no longer exists")?;
        let profile = self
            .settings
            .agents
            .iter()
            .find(|profile| profile.id == action.agent_id)
            .cloned()
            .ok_or("The agent of this action no longer exists")?;
        let context = message_context::build(self.app.read(cx), conversation_id, message_id)
            .ok_or("The message is no longer available")?;
        let chosen_working_dir = options
            .working_dir
            .clone()
            .filter(|_| options.session.is_none() && options.remember_working_dir);
        if let Some(target) = &options.session {
            action.working_dir = target.working_dir.clone();
        } else if let Some(working_dir) = &options.working_dir {
            action.working_dir = working_dir.clone();
        }
        let (session_id, prompt) = match &options.session {
            Some(target) => {
                let session_id = if target.fork {
                    agents::new_session_id()
                } else {
                    target.session_id.clone()
                };
                let prompt =
                    agents::build_session_prompt(&context, extra, &session_id, &profile.id);
                (session_id, prompt)
            }
            None => {
                let session_id = agents::new_session_id();
                let prompt =
                    agents::build_prompt(&action, &context, extra, &session_id, &profile.id);
                (session_id, prompt)
            }
        };
        let run_id = self.next_id;
        self.next_id += 1;
        self.runs.push(AgentRun {
            id: run_id,
            action_id: action.id.clone(),
            action_label: action.label.clone(),
            agent_id: profile.id.clone(),
            agent_name: profile.name.clone(),
            conversation_id: conversation_id.to_owned(),
            message_id: message_id.to_owned(),
            chat_title: context.chat.clone(),
            sender_name: context.sender.clone(),
            session_id,
            working_dir: action.working_dir.clone(),
            extra: extra.to_owned(),
            state: RunState::Running,
            current_step: "Starting".to_owned(),
            steps: Vec::new(),
            final_text: String::new(),
            started_at: Utc::now(),
            ended_at: None,
            expanded: false,
            dismissed: false,
            process_id: None,
            pending_prompt: Some(prompt),
            sent_to: options.session.clone(),
            launch_session_id: None,
            launched: false,
        });
        if mcp_bridge::endpoint(cx).is_none()
            && let Some(run) = self.run_mut(run_id)
        {
            run.push_step("MCP server is off - no reply proposal possible".to_owned());
        }
        self.dispatch(run_id, cx);
        if let Some(working_dir) = chosen_working_dir {
            self.update_settings(
                |settings| settings.remember_working_dir(&profile.id, &working_dir),
                cx,
            );
        }
        self.persist(cx);
        self.ensure_ticking(cx);
        cx.notify();
        Ok(run_id)
    }

    fn dispatch(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let Some(session_id) = self.get(run_id).map(|run| run.session_id.clone()) else {
            return;
        };
        match self.lanes.admit(&session_id, run_id) {
            Admit::Launch => self.launch(run_id, cx),
            Admit::Queued => {
                if let Some(run) = self.run_mut(run_id) {
                    run.push_step(QUEUED_STEP.to_owned());
                }
            }
        }
    }

    fn release(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let runs = &self.runs;
        let next = self.lanes.next(session_id, |run_id| {
            runs.iter().any(|run| run.id == run_id && run.is_queued())
        });
        if let Some(next) = next {
            self.launch(next, cx);
        }
    }

    fn finish_kill(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let session_id = &self.lanes.resolve(session_id);
        self.lanes.kill_done(session_id);
        self.open_deferred_for(session_id, cx);
        self.release(session_id, cx);
    }

    fn open_deferred_for(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if self.lanes.has_kills_pending(session_id) {
            return;
        }
        let ready: Vec<u64> = self
            .open_deferred
            .iter()
            .copied()
            .filter(|run_id| {
                self.get(*run_id)
                    .is_some_and(|run| run.session_id == session_id)
            })
            .collect();
        for run_id in ready {
            self.open_deferred.remove(&run_id);
            if let Some(run) = self.get(run_id).cloned() {
                self.open_and_cancel(&run, cx);
            }
        }
    }

    fn run_kill(
        &mut self,
        session_id: &str,
        stop: Option<agents::Invocation>,
        cx: &mut Context<Self>,
    ) {
        let Some(stop) = stop else {
            self.finish_kill(session_id, cx);
            return;
        };
        let session_id = session_id.to_owned();
        cx.spawn(async move |this, cx| {
            kills::execute(stop).await;
            this.update(cx, |runs, cx| runs.finish_kill(&session_id, cx))
                .ok();
        })
        .detach();
    }

    fn open_and_cancel(&mut self, run: &AgentRun, cx: &mut Context<Self>) {
        open_run_session(run, cx);
        let waiting = self.lanes.drain_waiting(&run.session_id);
        for run_id in &waiting {
            if let Some(waiting_run) = self.run_mut(*run_id) {
                waiting_run.pending_prompt = None;
            }
            self.finish(
                *run_id,
                Some(RunOutcome {
                    status: RunStatus::Stopped,
                    exit_code: None,
                }),
                cx,
            );
            if let Some(waiting_run) = self.run_mut(*run_id) {
                waiting_run.current_step = OPENED_NOTICE.to_owned();
            }
        }
        if !waiting.is_empty() {
            self.persist(cx);
            raise_notice(OPENED_NOTICE.to_owned(), cx);
        }
        self.release(&run.session_id, cx);
    }

    fn launch(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let Some(run) = self.get(run_id).cloned() else {
            return;
        };
        let prompt = run.pending_prompt.clone().unwrap_or_default();
        let planned = self.launch_plan(&run, &prompt, cx);
        if let Some(run) = self.run_mut(run_id) {
            run.pending_prompt = None;
            if run.current_step == QUEUED_STEP {
                run.started_at = Utc::now();
                run.push_step("Starting".to_owned());
            }
        }
        match planned {
            Ok((plan, format)) => self.spawn(run_id, plan, format, cx),
            Err(message) => self.finish(
                run_id,
                Some(RunOutcome {
                    status: RunStatus::Failed {
                        message,
                        stderr_tail: Vec::new(),
                    },
                    exit_code: None,
                }),
                cx,
            ),
        }
        self.persist(cx);
    }

    fn launch_plan(
        &self,
        run: &AgentRun,
        prompt: &str,
        cx: &App,
    ) -> Result<(agents::RunPlan, agents::OutputFormat), String> {
        let action = self
            .settings
            .actions
            .iter()
            .find(|action| action.id == run.action_id)
            .cloned()
            .ok_or("the action no longer exists")?;
        let profile = self
            .settings
            .agents
            .iter()
            .find(|profile| profile.id == run.agent_id)
            .ok_or("the agent no longer exists")?;
        let action = AgentAction {
            working_dir: run.working_dir.clone(),
            ..action
        };
        plan_for(
            profile,
            &action,
            prompt,
            &run.session_id,
            run.sent_to.as_ref(),
            mcp_bridge::endpoint(cx).as_ref(),
        )
        .map(|plan| (plan, profile.output))
        .map_err(|error| error.to_string())
    }

    fn spawn(
        &mut self,
        run_id: u64,
        plan: agents::RunPlan,
        format: agents::OutputFormat,
        cx: &mut Context<Self>,
    ) {
        let (updates_sender, mut updates) = mpsc::unbounded_channel();
        let (stop_sender, stop_receiver) = oneshot::channel();
        self.stops.insert(run_id, stop_sender);
        if let Some(run) = self.run_mut(run_id) {
            run.launch_session_id = Some(run.session_id.clone());
        }
        let outcome = runtime::spawn(agents::run(plan, format, updates_sender, stop_receiver));
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.recv().await {
                if this
                    .update(cx, |runs, cx| runs.apply_update(run_id, update, cx))
                    .is_err()
                {
                    return;
                }
            }
            let outcome = outcome.await.ok();
            this.update(cx, |runs, cx| runs.finish(run_id, outcome, cx))
                .ok();
        })
        .detach();
    }

    fn apply_update(&mut self, run_id: u64, update: RunUpdate, cx: &mut Context<Self>) {
        let event = match update {
            RunUpdate::Event(event) => event,
            RunUpdate::Started(process_id) => {
                if let Some(run) = self.run_mut(run_id) {
                    run.process_id = Some(process_id);
                    run.launched = true;
                    self.persist(cx);
                }
                return;
            }
            RunUpdate::Stderr(_) => return,
        };
        let Some(run) = self.run_mut(run_id) else {
            return;
        };
        match event {
            RunEvent::Step(text) => {
                run.push_step(text);
                self.persist_progress(cx);
            }
            RunEvent::Text(text) => {
                run.current_step = truncated(first_line(&text), TEXT_STEP_CHARS)
            }
            RunEvent::ApiRetry { message, retry_in } => {
                run.current_step = api_retry_text(&message, retry_in)
            }
            RunEvent::SessionId(session_id) if session_id != run.session_id => {
                let previous = std::mem::replace(&mut run.session_id, session_id.clone());
                self.move_lane(&previous, &session_id);
                self.persist(cx);
            }
            _ => return,
        }
        cx.notify();
    }

    fn move_lane(&mut self, from: &str, to: &str) {
        for run_id in self.lanes.rename(from, to) {
            if let Some(run) = self.run_mut(run_id) {
                run.session_id = to.to_owned();
                if let Some(target) = run.sent_to.as_mut().filter(|target| !target.fork) {
                    target.session_id = to.to_owned();
                }
            }
        }
    }

    fn finish(&mut self, run_id: u64, outcome: Option<RunOutcome>, cx: &mut Context<Self>) {
        self.stops.remove(&run_id);
        self.stopping.remove(&run_id);
        let was_resuming = self.resuming.remove(&run_id);
        let Some(run) = self.run_mut(run_id).filter(|run| run.is_running()) else {
            return;
        };
        run.ended_at = Some(Utc::now());
        run.process_id = None;
        match outcome.map(|outcome| outcome.status) {
            Some(RunStatus::Completed { final_text }) => {
                let line = truncated(first_line(&final_text), DONE_STEP_CHARS);
                run.current_step = if line.is_empty() {
                    "Done".to_owned()
                } else {
                    format!("Done - {line}")
                };
                run.final_text = final_text;
                run.state = RunState::Done;
            }
            Some(RunStatus::Failed {
                message,
                stderr_tail,
            }) => {
                let message = if was_resuming && message.starts_with("cannot start") {
                    format!("Could not resume - {message}")
                } else {
                    message
                };
                run.current_step = message.clone();
                run.state = RunState::Failed {
                    message,
                    log: stderr_tail,
                };
            }
            Some(RunStatus::Stopped) => {
                run.current_step = "Stopped".to_owned();
                run.state = RunState::Stopped;
            }
            None => {
                let message = "The run was cancelled".to_owned();
                run.current_step = message.clone();
                run.state = RunState::Failed {
                    message,
                    log: Vec::new(),
                };
            }
        }
        let stopped = run.state == RunState::Stopped;
        let failed_or_done = !stopped;
        let snapshot = run.clone();
        self.prune_finished();
        self.persist(cx);
        if failed_or_done {
            cx.emit(AgentRunsEvent::Finished(run_id));
        }
        let session_id = snapshot.session_id.clone();
        let was_active = self.lanes.finished(&session_id, run_id);
        if self.open_after_stop.remove(&run_id) {
            if self.lanes.has_kills_pending(&session_id) {
                self.open_deferred.insert(run_id);
            } else {
                self.open_and_cancel(&snapshot, cx);
            }
        }
        if was_resuming {
            self.resume_next(cx);
        }
        if was_active {
            self.release(&session_id, cx);
        }
        cx.notify();
    }

    fn prune_finished(&mut self) {
        let finished = self.runs.iter().filter(|run| run.is_finished()).count();
        let mut excess = finished.saturating_sub(run_store::RUNS_HISTORY_LIMIT);
        self.runs.retain(|run| {
            if excess > 0 && run.is_finished() {
                excess -= 1;
                return false;
            }
            true
        });
    }

    fn ensure_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking || self.running_count() == 0 {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let keep_going = this
                    .update(cx, |runs, cx| {
                        runs.ticking = runs.running_count() > 0;
                        if runs.progress_unsaved {
                            runs.persist_progress(cx);
                        }
                        if runs.ticking {
                            cx.notify();
                        }
                        runs.ticking
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn stop(&mut self, run_id: u64, cx: &mut Context<Self>) {
        if self.stopping.contains(&run_id) {
            return;
        }
        if let Some(stop) = self.stops.remove(&run_id) {
            self.stopping.insert(run_id);
            let _ = stop.send(());
            if let Some(run) = self.run_mut(run_id) {
                run.current_step = "Stopping".to_owned();
            }
            cx.notify();
        } else if self.get(run_id).is_some_and(AgentRun::is_queued) {
            let session_id = self.get(run_id).map(|run| run.session_id.clone());
            if let Some(session_id) = session_id {
                self.lanes.remove_waiting(&session_id, run_id);
            }
            if let Some(run) = self.run_mut(run_id) {
                run.pending_prompt = None;
            }
            self.finish(
                run_id,
                Some(RunOutcome {
                    status: RunStatus::Stopped,
                    exit_code: None,
                }),
                cx,
            );
        } else if self.resuming.contains(&run_id) || self.resume_queue.contains(&run_id) {
            let orphan_stop = self
                .resume_queue
                .contains(&run_id)
                .then(|| {
                    self.get(run_id).and_then(|run| {
                        kills::orphan_kill(run, |candidate| {
                            self.resume_plan(candidate, cx).ok().map(|(plan, _)| plan)
                        })
                    })
                })
                .flatten();
            let was_queued_for_resume = self.resume_queue.contains(&run_id);
            let session_id = self.get(run_id).map(|run| run.session_id.clone());
            self.resume_queue.retain(|queued| *queued != run_id);
            self.finish(
                run_id,
                Some(RunOutcome {
                    status: RunStatus::Stopped,
                    exit_code: None,
                }),
                cx,
            );
            if let (true, Some(session_id)) = (was_queued_for_resume, session_id) {
                self.run_kill(&session_id, orphan_stop, cx);
            }
        }
    }

    pub fn stop_and_open_session(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let Some(run) = self.get(run_id).cloned() else {
            return;
        };
        if run.is_finished() {
            if self.lanes.has_kills_pending(&run.session_id) {
                self.open_deferred.insert(run_id);
            } else {
                self.open_and_cancel(&run, cx);
            }
            return;
        }
        if !run.is_queued() {
            self.open_after_stop.insert(run_id);
        }
        self.stop(run_id, cx);
    }

    pub fn retry(&mut self, run_id: u64, cx: &mut Context<Self>) -> Result<u64, String> {
        let run = self
            .get(run_id)
            .cloned()
            .ok_or("This run no longer exists")?;
        let options = StartOptions {
            working_dir: run.sent_to.is_none().then(|| run.working_dir.clone()),
            remember_working_dir: false,
            session: run.sent_to.clone().map(|target| SessionTarget {
                session_id: if target.fork {
                    target.session_id
                } else {
                    run.session_id.clone()
                },
                ..target
            }),
        };
        let started = self.start_with(
            &run.action_id,
            &run.conversation_id,
            &run.message_id,
            &run.extra,
            options,
            cx,
        )?;
        self.dismiss(run_id, cx);
        Ok(started)
    }

    pub fn dismiss(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let Some(run) = self.run_mut(run_id).filter(|run| run.is_finished()) else {
            return;
        };
        run.dismissed = true;
        self.persist(cx);
        cx.notify();
    }

    pub fn toggle_expanded(&mut self, run_id: u64, cx: &mut Context<Self>) {
        if let Some(run) = self.run_mut(run_id) {
            run.expanded = !run.expanded;
            cx.notify();
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use agents::{AgentAction, AgentProfile, AgentSettings, Environment, OutputFormat};
    use chrono::TimeZone;
    use store::{MessageRecord, Store};

    use gpui_kit::{AppContext as _, Entity, TestAppContext};

    use super::{
        AgentRun, AgentRuns, RunState, SessionTarget, StartOptions, Utc, api_retry_text,
        format_elapsed, run_store,
    };
    use crate::app_state::{AppState, Mode};

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

    fn fake_settings(command: &str) -> AgentSettings {
        let profile = AgentProfile {
            id: "fake".to_owned(),
            name: "Fake".to_owned(),
            environment: Environment::Host,
            command: command.to_owned(),
            output: OutputFormat::ClaudeStreamJson,
            open_session_command: String::new(),
            mcp_config_flag: String::new(),
        };
        let action = AgentAction {
            id: "ask".to_owned(),
            label: "Ask".to_owned(),
            agent_id: "fake".to_owned(),
            working_dir: std::env::temp_dir().to_string_lossy().into_owned(),
            prompt_template: "{message}".to_owned(),
            ask_extra_instructions: false,
            extra_args: String::new(),
        };
        AgentSettings {
            agents: vec![profile],
            actions: vec![action],
            resume_interrupted_runs: true,
            ..AgentSettings::default()
        }
    }

    fn setup(cx: &mut TestAppContext, command: &str) -> (Entity<AppState>, Entity<AgentRuns>) {
        cx.executor().allow_parking();
        let store = Arc::new(Store::open_in_memory().unwrap());
        store
            .upsert_messages(&[MessageRecord {
                conversation_id: "19:chat".to_owned(),
                message_id: "m1".to_owned(),
                sender_name: Some("Ada".to_owned()),
                created_at: Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
                body_html: "<p>Where is the report?</p>".to_owned(),
                attachments_json: "[]".to_owned(),
                reactions_json: "[]".to_owned(),
                mentions_json: "[]".to_owned(),
                links_json: "[]".to_owned(),
                ..MessageRecord::default()
            }])
            .unwrap();
        run_store::save_settings(&store, &fake_settings(command));
        cx.update(|cx| {
            let app = cx.new(|_| AppState::new(store, Mode::default()));
            let runs = cx.new(|cx| AgentRuns::new(app.clone(), cx));
            (app, runs)
        })
    }

    fn wait_until_finished(cx: &mut TestAppContext, runs: &Entity<AgentRuns>, run_id: u64) {
        for _ in 0..500 {
            cx.run_until_parked();
            let finished = cx.update(|cx| {
                runs.read(cx)
                    .get(run_id)
                    .is_some_and(|run| run.is_finished())
            });
            if finished {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the run did not finish");
    }

    #[cfg(unix)]
    mod shell_runs {
        use std::time::Duration;

        use gpui_kit::AppContext as _;

        use super::{
            AgentRun, AgentRuns, Entity, RunState, SessionTarget, StartOptions, TestAppContext,
            run_store, setup, wait_until_finished,
        };

        const SESSION_COMMAND: &str = "sh -c 'sleep 1' --session-id {session_id}";

        fn target_of(session_id: String) -> StartOptions {
            StartOptions {
                working_dir: None,
                remember_working_dir: false,
                session: Some(SessionTarget {
                    session_id,
                    working_dir: std::env::temp_dir().to_string_lossy().into_owned(),
                    fork: false,
                }),
            }
        }

        type LaneView = Option<(Option<u64>, Vec<u64>)>;

        fn lane_of(
            cx: &mut TestAppContext,
            runs: &Entity<AgentRuns>,
            session_id: &str,
        ) -> LaneView {
            cx.update(|cx| {
                runs.read(cx).lanes.get(session_id).map(|lane| {
                    (
                        lane.active.first().copied(),
                        lane.waiting.iter().copied().collect::<Vec<u64>>(),
                    )
                })
            })
        }

        fn send_chain(
            cx: &mut TestAppContext,
            runs: &Entity<AgentRuns>,
        ) -> (u64, u64, u64, String) {
            cx.update(|cx| {
                runs.update(cx, |runs, cx| {
                    let first = runs.start("ask", "19:chat", "m1", "", cx).unwrap();
                    let session_id = runs.get(first).unwrap().session_id.clone();
                    let second = runs
                        .start_with(
                            "ask",
                            "19:chat",
                            "m1",
                            "two",
                            target_of(session_id.clone()),
                            cx,
                        )
                        .unwrap();
                    let third = runs
                        .start_with(
                            "ask",
                            "19:chat",
                            "m1",
                            "three",
                            target_of(session_id.clone()),
                            cx,
                        )
                        .unwrap();
                    (first, second, third, session_id)
                })
            })
        }

        fn started_and_ended(
            cx: &mut TestAppContext,
            runs: &Entity<AgentRuns>,
            run_id: u64,
        ) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
            cx.update(|cx| {
                let run = runs.read(cx).get(run_id).unwrap();
                (run.started_at, run.ended_at.unwrap())
            })
        }

        #[gpui_kit::test]
        fn two_sends_to_one_session_run_strictly_one_after_another(cx: &mut TestAppContext) {
            let (_, runs) = setup(cx, SESSION_COMMAND);
            let (first, second, third, session_id) = send_chain(cx, &runs);
            assert_eq!(
                lane_of(cx, &runs, &session_id),
                Some((Some(first), vec![second, third]))
            );
            cx.update(|cx| {
                let queued = runs.read(cx).get(second).unwrap();
                assert_eq!(queued.current_step, "Queued");
                let prompt = queued.pending_prompt.clone().unwrap();
                assert!(prompt.contains("propose_message") && prompt.contains("m1"));
                assert!(prompt.ends_with("\n\ntwo"));
                assert_eq!(queued.extra, "two");
                assert_eq!(queued.session_id, session_id);
            });
            wait_until_finished(cx, &runs, first);
            assert_eq!(
                lane_of(cx, &runs, &session_id),
                Some((Some(second), vec![third]))
            );
            cx.update(|cx| {
                assert!(!runs.read(cx).get(second).unwrap().is_queued());
                assert!(runs.read(cx).get(third).unwrap().is_queued());
            });
            wait_until_finished(cx, &runs, second);
            assert_eq!(lane_of(cx, &runs, &session_id), Some((Some(third), vec![])));
            wait_until_finished(cx, &runs, third);
            assert_eq!(lane_of(cx, &runs, &session_id), None);
            let (_, first_ended) = started_and_ended(cx, &runs, first);
            let (second_started, second_ended) = started_and_ended(cx, &runs, second);
            let (third_started, _) = started_and_ended(cx, &runs, third);
            assert!(second_started >= first_ended);
            assert!(third_started >= second_ended);
        }

        #[gpui_kit::test]
        fn a_reported_session_id_moves_the_whole_lane_and_its_waiting_runs(
            cx: &mut TestAppContext,
        ) {
            let (_, runs) = setup(cx, SESSION_COMMAND);
            let (first, second, third, session_id) = send_chain(cx, &runs);
            cx.update(|cx| {
                runs.update(cx, |runs, cx| {
                    runs.apply_update(
                        first,
                        agents::RunUpdate::Event(agents::RunEvent::SessionId("adopted".to_owned())),
                        cx,
                    )
                });
                let runs = runs.read(cx);
                assert_eq!(
                    runs.get(first).unwrap().launch_session_id,
                    Some(session_id.clone())
                );
                assert_eq!(runs.get(first).unwrap().session_id, "adopted");
                for run_id in [second, third] {
                    assert_eq!(runs.get(run_id).unwrap().session_id, "adopted");
                }
            });
            assert_eq!(lane_of(cx, &runs, &session_id), None);
            assert_eq!(
                lane_of(cx, &runs, "adopted"),
                Some((Some(first), vec![second, third]))
            );
            wait_until_finished(cx, &runs, first);
            assert_eq!(
                lane_of(cx, &runs, "adopted"),
                Some((Some(second), vec![third]))
            );
            wait_until_finished(cx, &runs, second);
            wait_until_finished(cx, &runs, third);
        }

        #[gpui_kit::test]
        fn stopping_a_waiting_send_only_removes_it_from_the_lane(cx: &mut TestAppContext) {
            let (_, runs) = setup(cx, SESSION_COMMAND);
            let (first, second, third, session_id) = send_chain(cx, &runs);
            cx.update(|cx| runs.update(cx, |runs, cx| runs.stop(second, cx)));
            cx.update(|cx| {
                let stopped = runs.read(cx).get(second).unwrap();
                assert_eq!(stopped.state, RunState::Stopped);
                assert!(!stopped.is_queued());
            });
            assert_eq!(
                lane_of(cx, &runs, &session_id),
                Some((Some(first), vec![third]))
            );
            wait_until_finished(cx, &runs, first);
            assert_eq!(lane_of(cx, &runs, &session_id), Some((Some(third), vec![])));
            wait_until_finished(cx, &runs, third);
        }

        #[gpui_kit::test]
        fn stop_and_open_cancels_every_waiting_send_of_the_lane(cx: &mut TestAppContext) {
            let (app, runs) = setup(cx, SESSION_COMMAND);
            let (first, second, third, session_id) = send_chain(cx, &runs);
            cx.update(|cx| runs.update(cx, |runs, cx| runs.stop_and_open_session(first, cx)));
            wait_until_finished(cx, &runs, first);
            cx.update(|cx| {
                for run_id in [second, third] {
                    let run = runs.read(cx).get(run_id).unwrap();
                    assert_eq!(run.state, RunState::Stopped);
                    assert_eq!(
                        run.current_step,
                        "Session opened in a terminal - send cancelled"
                    );
                    assert!(!run.is_queued());
                }
                assert!(runs.read(cx).open_after_stop.is_empty());
            });
            assert_eq!(lane_of(cx, &runs, &session_id), None);
            cx.update(|cx| {
                let stored = run_store::load_runs(&app.read(cx).store);
                let notice = stored.iter().find(|run| run.id == second).unwrap();
                assert_eq!(
                    notice.current_step,
                    "Session opened in a terminal - send cancelled"
                );
            });
        }

        #[gpui_kit::test]
        fn a_requested_open_fires_whatever_the_outcome_of_the_run(cx: &mut TestAppContext) {
            let (_, runs) = setup(cx, "sh -c 'sleep 1; exit 3' --session-id {session_id}");
            let (first, second, _, session_id) = send_chain(cx, &runs);
            cx.update(|cx| runs.update(cx, |runs, _| runs.open_after_stop.insert(first)));
            wait_until_finished(cx, &runs, first);
            cx.update(|cx| {
                assert!(matches!(
                    runs.read(cx).get(first).unwrap().state,
                    RunState::Failed { .. }
                ));
                assert_eq!(runs.read(cx).get(second).unwrap().state, RunState::Stopped);
                assert!(runs.read(cx).open_after_stop.is_empty());
            });
            assert_eq!(lane_of(cx, &runs, &session_id), None);
        }

        #[gpui_kit::test]
        fn a_run_that_never_launched_is_interrupted_not_resumed(cx: &mut TestAppContext) {
            let (app, _) = setup(cx, SESSION_COMMAND);
            let mut never_started = super::running_run(5);
            never_started.launched = false;
            let reloaded = reload(cx, &app, vec![never_started]);
            cx.update(|cx| {
                assert!(reloaded.read(cx).get(5).unwrap().is_finished());
            });
            assert_eq!(lane_of(cx, &reloaded, "session-5"), None);
        }

        #[gpui_kit::test]
        fn stop_and_open_waits_for_the_orphan_kill_before_cancelling_the_lane(
            cx: &mut TestAppContext,
        ) {
            let (app, _) = setup(cx, SESSION_COMMAND);
            let mut orphan = super::running_run(5);
            orphan.session_id = "shared".to_owned();
            orphan.process_id = Some(1);
            let reloaded = reload(cx, &app, vec![orphan, queued_run(7)]);
            cx.update(|cx| {
                reloaded.update(cx, |runs, cx| runs.stop_and_open_session(5, cx));
                let runs = reloaded.read(cx);
                assert!(runs.get(5).unwrap().is_finished());
                assert!(runs.open_deferred.contains(&5));
                assert!(runs.get(7).unwrap().is_queued());
                assert!(runs.lanes.has_kills_pending("shared"));
            });
            for _ in 0..200 {
                cx.run_until_parked();
                if lane_of(cx, &reloaded, "shared").is_none() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            cx.update(|cx| {
                let runs = reloaded.read(cx);
                assert_eq!(runs.get(7).unwrap().state, RunState::Stopped);
                assert!(runs.open_after_stop.is_empty() && runs.open_deferred.is_empty());
            });
        }

        #[gpui_kit::test]
        fn a_send_during_an_in_flight_kill_waits_for_the_resumed_run(cx: &mut TestAppContext) {
            let (app, _) = setup(cx, SESSION_COMMAND);
            let mut orphan = super::running_run(5);
            orphan.session_id = "shared".to_owned();
            orphan.process_id = Some(1);
            let reloaded = reload(cx, &app, vec![orphan]);
            let sent = cx.update(|cx| {
                reloaded.update(cx, |runs, cx| {
                    runs.start_with(
                        "ask",
                        "19:chat",
                        "m1",
                        "hi",
                        target_of("shared".to_owned()),
                        cx,
                    )
                    .unwrap()
                })
            });
            assert_eq!(
                lane_of(cx, &reloaded, "shared"),
                Some((Some(5), vec![sent]))
            );
            wait_until_finished(cx, &reloaded, 5);
            wait_until_finished(cx, &reloaded, sent);
            let (_, resumed_ended) = started_and_ended(cx, &reloaded, 5);
            let (sent_started, _) = started_and_ended(cx, &reloaded, sent);
            assert!(sent_started >= resumed_ended);
        }

        #[test]
        fn old_saved_runs_with_a_chain_id_still_load() {
            let mut value = serde_json::to_value(super::running_run(3)).unwrap();
            let object = value.as_object_mut().unwrap();
            for key in ["sent_to", "launch_session_id", "launched", "pending_prompt"] {
                object.remove(key);
            }
            object.insert("after_run_id".to_owned(), serde_json::json!(2));
            object.insert("pending_prompt".to_owned(), serde_json::json!("saved"));
            let run: AgentRun = serde_json::from_value(value).unwrap();
            assert!(run.is_queued());
            assert!(run.launched);
            assert_eq!(run.sent_to, None);
        }

        #[gpui_kit::test]
        fn a_forked_send_gets_a_fresh_session_id_and_adopts_a_reported_one(
            cx: &mut TestAppContext,
        ) {
            let (app, runs) = setup(cx, SESSION_COMMAND);
            let forked = cx.update(|cx| {
                runs.update(cx, |runs, cx| {
                    let mut options = target_of("live-session".to_owned());
                    options.session.as_mut().unwrap().fork = true;
                    runs.start_with("ask", "19:chat", "m1", "hi", options, cx)
                        .unwrap()
                })
            });
            cx.update(|cx| {
                let run = runs.read(cx).get(forked).unwrap().clone();
                assert_ne!(run.session_id, "live-session");
                assert!(run.sent_to.as_ref().unwrap().fork);
                assert!(!run.is_queued());
                runs.update(cx, |runs, cx| {
                    runs.apply_update(
                        forked,
                        agents::RunUpdate::Event(agents::RunEvent::SessionId(
                            "reported".to_owned(),
                        )),
                        cx,
                    )
                });
                assert_eq!(runs.read(cx).get(forked).unwrap().session_id, "reported");
                let stored = run_store::load_runs(&app.read(cx).store);
                assert_eq!(stored[0].session_id, "reported");
            });
            wait_until_finished(cx, &runs, forked);
        }

        #[gpui_kit::test]
        fn retrying_a_plain_run_keeps_its_chosen_working_dir_without_recording_it(
            cx: &mut TestAppContext,
        ) {
            let (_, runs) = setup(cx, SESSION_COMMAND);
            let started = cx.update(|cx| {
                runs.update(cx, |runs, cx| {
                    runs.start_with(
                        "ask",
                        "19:chat",
                        "m1",
                        "",
                        StartOptions {
                            working_dir: Some("/srv/chosen".to_owned()),
                            remember_working_dir: false,
                            session: None,
                        },
                        cx,
                    )
                    .unwrap()
                })
            });
            wait_until_finished(cx, &runs, started);
            let retried =
                cx.update(|cx| runs.update(cx, |runs, cx| runs.retry(started, cx)).unwrap());
            cx.update(|cx| {
                assert_eq!(
                    runs.read(cx).get(retried).unwrap().working_dir,
                    "/srv/chosen"
                );
                assert!(
                    runs.read(cx)
                        .settings()
                        .recent_working_dirs_of("fake")
                        .is_empty()
                );
            });
            wait_until_finished(cx, &runs, retried);
        }

        #[gpui_kit::test]
        fn retrying_a_session_send_sends_to_the_same_session_again(cx: &mut TestAppContext) {
            let (_, runs) = setup(cx, SESSION_COMMAND);
            let sent = cx.update(|cx| {
                runs.update(cx, |runs, cx| {
                    runs.start_with(
                        "ask",
                        "19:chat",
                        "m1",
                        "again please",
                        target_of("old-session".to_owned()),
                        cx,
                    )
                    .unwrap()
                })
            });
            wait_until_finished(cx, &runs, sent);
            let retried = cx.update(|cx| runs.update(cx, |runs, cx| runs.retry(sent, cx)).unwrap());
            cx.update(|cx| {
                let run = runs.read(cx).get(retried).unwrap();
                assert_eq!(run.session_id, "old-session");
                assert_eq!(run.extra, "again please");
                assert_eq!(run.sent_to.as_ref().unwrap().session_id, "old-session");
            });
            wait_until_finished(cx, &runs, retried);
        }

        fn queued_run(id: u64) -> AgentRun {
            let mut run = super::running_run(id);
            run.session_id = "shared".to_owned();
            run.current_step = "Queued".to_owned();
            run.pending_prompt = Some("saved prompt".to_owned());
            run
        }

        fn reload(
            cx: &mut TestAppContext,
            app: &Entity<crate::app_state::AppState>,
            stored: Vec<AgentRun>,
        ) -> Entity<AgentRuns> {
            cx.update(|cx| {
                run_store::save_runs(&app.read(cx).store, &stored);
                cx.new(|cx| AgentRuns::new(app.clone(), cx))
            })
        }

        #[gpui_kit::test]
        fn a_restart_rebuilds_the_lane_and_never_runs_two_processes_on_one_session(
            cx: &mut TestAppContext,
        ) {
            let (app, _) = setup(cx, SESSION_COMMAND);
            let mut active = super::running_run(5);
            active.session_id = "shared".to_owned();
            let mut duplicate = super::running_run(6);
            duplicate.session_id = "shared".to_owned();
            let reloaded = reload(
                cx,
                &app,
                vec![active, duplicate, queued_run(7), queued_run(8)],
            );
            cx.update(|cx| {
                let runs = reloaded.read(cx);
                assert!(runs.get(6).unwrap().is_finished());
                assert!(runs.get(7).unwrap().is_queued() && runs.get(8).unwrap().is_queued());
            });
            assert_eq!(
                lane_of(cx, &reloaded, "shared"),
                Some((Some(5), vec![7, 8]))
            );
            wait_until_finished(cx, &reloaded, 5);
            assert_eq!(lane_of(cx, &reloaded, "shared"), Some((Some(7), vec![8])));
            wait_until_finished(cx, &reloaded, 7);
            wait_until_finished(cx, &reloaded, 8);
            let (_, ended_5) = started_and_ended(cx, &reloaded, 5);
            let (started_7, ended_7) = started_and_ended(cx, &reloaded, 7);
            let (started_8, _) = started_and_ended(cx, &reloaded, 8);
            assert!(started_7 >= ended_5 && started_8 >= ended_7);
        }

        #[gpui_kit::test]
        fn a_waiting_send_without_a_predecessor_runs_after_a_restart(cx: &mut TestAppContext) {
            let (app, _) = setup(cx, SESSION_COMMAND);
            let reloaded = reload(cx, &app, vec![queued_run(7), queued_run(8)]);
            assert_eq!(lane_of(cx, &reloaded, "shared"), Some((None, vec![7, 8])));
            cx.run_until_parked();
            assert_eq!(lane_of(cx, &reloaded, "shared"), Some((Some(7), vec![8])));
            wait_until_finished(cx, &reloaded, 7);
            wait_until_finished(cx, &reloaded, 8);
            assert_eq!(lane_of(cx, &reloaded, "shared"), None);
        }

        #[gpui_kit::test]
        fn a_message_to_an_idle_session_resumes_it_and_a_chosen_dir_becomes_recent(
            cx: &mut TestAppContext,
        ) {
            let (_, runs) = setup(cx, SESSION_COMMAND);
            let (resumed, other) = cx.update(|cx| {
                runs.update(cx, |runs, cx| {
                    let resumed = runs
                        .start_with(
                            "ask",
                            "19:chat",
                            "m1",
                            "hello",
                            target_of("old-session".to_owned()),
                            cx,
                        )
                        .unwrap();
                    let other = runs
                        .start_with(
                            "ask",
                            "19:chat",
                            "m1",
                            "",
                            StartOptions {
                                working_dir: Some("/srv/other".to_owned()),
                                remember_working_dir: true,
                                session: None,
                            },
                            cx,
                        )
                        .unwrap();
                    (resumed, other)
                })
            });
            cx.update(|cx| {
                let resumed = runs.read(cx).get(resumed).unwrap().clone();
                assert_eq!(resumed.session_id, "old-session");
                assert_ne!(resumed.current_step, "Queued");
                let other = runs.read(cx).get(other).unwrap().clone();
                assert_eq!(other.working_dir, "/srv/other");
                assert_eq!(
                    runs.read(cx).settings().recent_working_dirs_of("fake"),
                    ["/srv/other"]
                );
            });
            wait_until_finished(cx, &runs, resumed);
        }

        #[gpui_kit::test]
        fn a_stream_json_run_ends_done_and_is_persisted(cx: &mut TestAppContext) {
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../agents/tests/fixtures/claude-stream.jsonl"
            );
            let (app, runs) = setup(cx, &format!("sh -c 'cat {fixture}'"));
            let run_id = cx.update(|cx| {
                runs.update(cx, |runs, cx| runs.start("ask", "19:chat", "m1", "", cx))
                    .unwrap()
            });
            wait_until_finished(cx, &runs, run_id);
            cx.update(|cx| {
                let run = runs.read(cx).get(run_id).unwrap().clone();
                assert_eq!(run.state, RunState::Done);
                assert_eq!(
                    run.current_step,
                    "Done - The first word in `/etc/hostname` is `HOST01`."
                );
                assert_eq!(run.steps.len(), 2);
                assert_eq!(
                    run.steps[0].1,
                    "MCP server is off - no reply proposal possible"
                );
                assert_eq!(run.steps[1].1, "Reading hostname");
                assert_eq!(run.chat_title, "Chat");
                assert_eq!(run.sender_name, "Ada");
                let stored = run_store::load_runs(&app.read(cx).store);
                assert_eq!(stored.len(), 1);
                assert_eq!(stored[0].state, RunState::Done);
                assert_eq!(stored[0].session_id, run.session_id);
                assert_eq!(runs.read(cx).running_count(), 0);
            });
        }

        #[gpui_kit::test]
        fn a_resumed_run_continues_and_finishes_done(cx: &mut TestAppContext) {
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../agents/tests/fixtures/claude-stream.jsonl"
            );
            let command = format!("sh -c 'cat {fixture}' --session-id {{session_id}}");
            let (app, runs) = setup(cx, &command);
            cx.update(|cx| {
                runs.update(cx, |runs, cx| {
                    runs.runs.push(super::running_run(5));
                    runs.persist(cx);
                });
            });
            let reloaded = cx.update(|cx| cx.new(|cx| AgentRuns::new(app.clone(), cx)));
            wait_until_finished(cx, &reloaded, 5);
            cx.update(|cx| {
                let run = reloaded.read(cx).get(5).unwrap().clone();
                assert_eq!(run.state, RunState::Done);
                assert_eq!(run.session_id, "3f6a2c1e-6f53-4c8e-9a51-7c2b5d0e9a11");
                assert!(
                    run.steps
                        .iter()
                        .any(|(_, text)| text == "Resumed after restart")
                );
            });
        }

        #[gpui_kit::test]
        fn a_failing_command_ends_failed_with_its_stderr(cx: &mut TestAppContext) {
            let (app, runs) = setup(cx, "sh -c 'echo boom >&2; exit 3'");
            let run_id = cx.update(|cx| {
                runs.update(cx, |runs, cx| runs.start("ask", "19:chat", "m1", "", cx))
                    .unwrap()
            });
            wait_until_finished(cx, &runs, run_id);
            cx.update(|cx| {
                let run = runs.read(cx).get(run_id).unwrap().clone();
                assert_eq!(
                    run.state,
                    RunState::Failed {
                        message: "boom".to_owned(),
                        log: vec!["boom".to_owned()],
                    }
                );
                assert_eq!(run.current_step, "boom");
                let stored = run_store::load_runs(&app.read(cx).store);
                assert_eq!(stored[0].state, run.state);
            });
        }
    }

    #[gpui_kit::test]
    fn a_missing_action_is_an_error_and_starts_nothing(cx: &mut TestAppContext) {
        let (_, runs) = setup(cx, "true");
        cx.update(|cx| {
            let error = runs
                .update(cx, |runs, cx| runs.start("ghost", "19:chat", "m1", "", cx))
                .unwrap_err();
            assert_eq!(error, "This action no longer exists");
            let error = runs
                .update(cx, |runs, cx| runs.start("ask", "19:chat", "gone", "", cx))
                .unwrap_err();
            assert_eq!(error, "The message is no longer available");
            assert_eq!(runs.read(cx).running_count(), 0);
        });
    }

    #[gpui_kit::test]
    fn dismissed_runs_hide_and_stay_dismissed_after_reload(cx: &mut TestAppContext) {
        let (app, runs) = setup(cx, "true");
        cx.update(|cx| {
            runs.update(cx, |runs, cx| {
                runs.runs.push(finished_run(7));
                runs.dismiss(7, cx);
                assert!(runs.for_conversation("19:chat").next().is_none());
                assert!(runs.recent(10).is_empty());
            });
            let reloaded = cx.new(|cx| AgentRuns::new(app.clone(), cx));
            let reloaded = reloaded.read(cx);
            assert!(reloaded.get(7).is_some_and(|run| run.dismissed));
            assert_eq!(reloaded.next_id, 8);
        });
    }

    const RESUMABLE_COMMAND: &str = "sh -c 'sleep 30' --session-id {session_id}";
    const INTERRUPTED: &str = "Interrupted - the app was closed during the run";

    fn running_run(id: u64) -> AgentRun {
        let mut running = finished_run(id);
        running.action_id = "ask".to_owned();
        running.agent_id = "fake".to_owned();
        running.state = RunState::Running;
        running.ended_at = None;
        running
    }

    fn reload_with(
        cx: &mut TestAppContext,
        command: &str,
        run: AgentRun,
        change_settings: impl FnOnce(&mut AgentSettings),
    ) -> AgentRun {
        let (app, runs) = setup(cx, command);
        cx.update(|cx| {
            let mut settings = fake_settings(command);
            change_settings(&mut settings);
            run_store::save_settings(&app.read(cx).store, &settings);
            let run_id = run.id;
            runs.update(cx, |runs, cx| {
                runs.runs.push(run);
                runs.persist(cx);
            });
            let reloaded = cx.new(|cx| AgentRuns::new(app.clone(), cx));
            reloaded.read(cx).get(run_id).unwrap().clone()
        })
    }

    fn assert_interrupted(run: &AgentRun) {
        assert!(matches!(
            run.state,
            RunState::Failed { ref message, .. } if message == INTERRUPTED
        ));
        assert!(run.ended_at.is_some());
    }

    #[gpui_kit::test]
    fn runs_loaded_as_running_become_interrupted(cx: &mut TestAppContext) {
        let run = reload_with(cx, "true", running_run(5), |_| {});
        assert_interrupted(&run);
        assert_eq!(run.session_id, "session-5");
    }

    #[gpui_kit::test]
    fn a_running_run_with_a_session_is_resumed(cx: &mut TestAppContext) {
        let original = running_run(5);
        let run = reload_with(cx, RESUMABLE_COMMAND, original.clone(), |_| {});
        assert_eq!(run.state, RunState::Running);
        assert_eq!(run.ended_at, None);
        assert_eq!(run.started_at, original.started_at);
        assert_eq!(run.current_step, "Resumed after restart");
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[0], original.steps[0]);
        assert_eq!(run.steps[1].1, "Resumed after restart");
    }

    #[gpui_kit::test]
    fn steps_survive_a_restart_while_the_run_is_running(cx: &mut TestAppContext) {
        let (app, runs) = setup(cx, RESUMABLE_COMMAND);
        let reloaded_run = cx.update(|cx| {
            let mut settings = fake_settings(RESUMABLE_COMMAND);
            settings.resume_interrupted_runs = true;
            run_store::save_settings(&app.read(cx).store, &settings);
            let mut run = running_run(5);
            run.steps.clear();
            runs.update(cx, |runs, cx| {
                runs.runs.push(run);
                runs.apply_update(
                    5,
                    agents::RunUpdate::Event(agents::RunEvent::Step("Reading notes".to_owned())),
                    cx,
                );
            });
            let reloaded = cx.new(|cx| AgentRuns::new(app.clone(), cx));
            reloaded.read(cx).get(5).unwrap().clone()
        });
        let step_texts: Vec<&str> = reloaded_run
            .steps
            .iter()
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(step_texts, ["Reading notes", "Resumed after restart"]);
    }

    #[gpui_kit::test]
    fn an_api_retry_shows_as_the_current_step_and_a_text_replaces_it(cx: &mut TestAppContext) {
        let (_, runs) = setup(cx, RESUMABLE_COMMAND);
        cx.update(|cx| {
            runs.update(cx, |runs, cx| {
                runs.runs.push(running_run(5));
                runs.apply_update(
                    5,
                    agents::RunUpdate::Event(agents::RunEvent::ApiRetry {
                        message: "No response from API".to_owned(),
                        retry_in: Some(Duration::from_secs(599)),
                    }),
                    cx,
                );
            });
            let run = runs.read(cx).get(5).unwrap().clone();
            assert_eq!(run.current_step, "API not responding - retrying in 10 min");
            assert_eq!(run.state, RunState::Running);
            runs.update(cx, |runs, cx| {
                runs.apply_update(
                    5,
                    agents::RunUpdate::Event(agents::RunEvent::Text("Back again".to_owned())),
                    cx,
                );
            });
            assert_eq!(runs.read(cx).get(5).unwrap().current_step, "Back again");
        });
    }

    #[test]
    fn api_retry_text_formats_minutes_and_seconds() {
        assert_eq!(
            api_retry_text("x", Some(Duration::from_secs(45))),
            "API not responding - retrying in 45 s"
        );
        assert_eq!(
            api_retry_text("x", Some(Duration::from_secs(120))),
            "API not responding - retrying in 2 min"
        );
    }

    fn reload_many(cx: &mut TestAppContext, run_count: u64) -> (Entity<AgentRuns>, Vec<u64>) {
        let (app, runs) = setup(cx, RESUMABLE_COMMAND);
        let ids: Vec<u64> = (1..=run_count).collect();
        let reloaded = cx.update(|cx| {
            runs.update(cx, |runs, cx| {
                runs.runs.extend(ids.iter().map(|id| running_run(*id)));
                runs.persist(cx);
            });
            cx.new(|cx| AgentRuns::new(app.clone(), cx))
        });
        (reloaded, ids)
    }

    fn state_of(cx: &mut TestAppContext, runs: &Entity<AgentRuns>, run_id: u64) -> RunState {
        cx.update(|cx| runs.read(cx).get(run_id).unwrap().state.clone())
    }

    #[gpui_kit::test]
    fn at_most_three_resumes_run_and_the_fourth_starts_when_one_finishes(cx: &mut TestAppContext) {
        let (runs, _) = reload_many(cx, 4);
        cx.run_until_parked();
        cx.update(|cx| {
            let runs = runs.read(cx);
            assert_eq!(runs.stops.len(), 3);
            assert_eq!(runs.resume_queue, [4]);
        });
        cx.update(|cx| runs.update(cx, |runs, cx| runs.stop(1, cx)));
        wait_until_finished(cx, &runs, 1);
        cx.run_until_parked();
        cx.update(|cx| {
            let runs_ref = runs.read(cx);
            assert!(runs_ref.resume_queue.is_empty());
            assert!(runs_ref.stops.contains_key(&4));
            runs.update(cx, |runs, cx| {
                for run_id in [2, 3, 4] {
                    runs.stop(run_id, cx);
                }
            });
        });
        wait_until_finished(cx, &runs, 4);
    }

    #[gpui_kit::test]
    fn stopping_a_queued_resume_ends_it_without_spawning(cx: &mut TestAppContext) {
        let (runs, _) = reload_many(cx, 4);
        cx.update(|cx| runs.update(cx, |runs, cx| runs.stop(4, cx)));
        cx.run_until_parked();
        assert_eq!(state_of(cx, &runs, 4), RunState::Stopped);
        cx.update(|cx| {
            let runs_ref = runs.read(cx);
            assert!(!runs_ref.stops.contains_key(&4));
            assert_eq!(runs_ref.stops.len(), 3);
            runs.update(cx, |runs, cx| {
                for run_id in [1, 2, 3] {
                    runs.stop(run_id, cx);
                }
            });
        });
        wait_until_finished(cx, &runs, 3);
    }

    #[gpui_kit::test]
    fn stopping_during_the_stop_wait_ends_the_run_without_spawning(cx: &mut TestAppContext) {
        let (runs, _) = reload_many(cx, 1);
        cx.update(|cx| {
            runs.update(cx, |runs, cx| {
                runs.resume_queue.clear();
                runs.begin_resume(1, cx);
                runs.stop(1, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(state_of(cx, &runs, 1), RunState::Stopped);
        cx.update(|cx| assert!(runs.read(cx).stops.is_empty()));
    }

    #[gpui_kit::test]
    fn a_running_run_without_a_session_is_interrupted(cx: &mut TestAppContext) {
        let mut original = running_run(5);
        original.session_id = String::new();
        let run = reload_with(cx, RESUMABLE_COMMAND, original, |_| {});
        assert_interrupted(&run);
    }

    #[gpui_kit::test]
    fn a_profile_that_cannot_resume_is_interrupted(cx: &mut TestAppContext) {
        let run = reload_with(cx, "sh -c 'sleep 30'", running_run(5), |_| {});
        assert_interrupted(&run);
    }

    #[gpui_kit::test]
    fn switching_resume_off_interrupts_the_run(cx: &mut TestAppContext) {
        let run = reload_with(cx, RESUMABLE_COMMAND, running_run(5), |settings| {
            settings.resume_interrupted_runs = false
        });
        assert_interrupted(&run);
    }

    #[test]
    fn layout_key_counts_visible_detail_rows() {
        let mut run = finished_run(1);
        assert_eq!(run.layout_key(), (0, false));
        run.expanded = true;
        run.steps = (0..9)
            .map(|index| (Duration::from_secs(index), index.to_string()))
            .collect();
        assert_eq!(run.layout_key(), (6, false));
        assert_eq!(run.visible_steps()[0].1, "3");
        run.state = RunState::Running;
        assert_eq!(run.layout_key(), (6, true));
    }

    #[test]
    fn elapsed_reads_as_minutes_and_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(42)), "0:42");
        assert_eq!(format_elapsed(Duration::from_secs(631)), "10:31");
        assert_eq!(finished_run(1).elapsed_text(), "1:00");
    }
}
