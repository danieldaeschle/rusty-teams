use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use agents::{AgentAction, AgentSettings, RunEvent, RunOutcome, RunStatus, RunUpdate};
use chrono::{DateTime, Utc};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use super::{
    MCP_POLL_INTERVAL, MCP_WAIT_LIMIT, StartRequest, demo, mcp_bridge, mcp_config_directory,
    message_context, open_run_session, store as run_store,
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
const RESUME_PARALLEL_LIMIT: usize = 3;
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
}

impl AgentRun {
    pub fn is_running(&self) -> bool {
        self.state == RunState::Running
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

fn orphan_stop(run: &AgentRun, plan: &agents::RunPlan) -> Option<agents::Invocation> {
    plan.stop.clone().or_else(|| {
        run.process_id
            .and_then(|process_id| agents::kill_process_invocation(process_id, &run.session_id))
    })
}

fn can_resume(settings: &AgentSettings, run: &AgentRun) -> bool {
    settings.resume_interrupted_runs
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

fn first_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
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
    resuming: HashSet<u64>,
    stopping: HashSet<u64>,
    open_after_stop: HashSet<u64>,
    next_id: u64,
    ticking: bool,
}

impl EventEmitter<AgentRunsEvent> for AgentRuns {}

impl AgentRuns {
    pub fn new(app: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let store = app.read(cx).store.clone();
        let settings = run_store::load_settings(&store);
        let mut runs = run_store::load_runs(&store);
        let interrupted = runs.iter().any(AgentRun::is_running);
        let mut resume_queue = VecDeque::new();
        for run in runs.iter_mut().filter(|run| run.is_running()) {
            if can_resume(&settings, run) {
                run.push_step(RESUMED_STEP.to_owned());
                resume_queue.push_back(run.id);
            } else {
                run.mark_interrupted();
            }
        }
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
            resuming: HashSet::new(),
            stopping: HashSet::new(),
            open_after_stop: HashSet::new(),
            next_id,
            ticking: false,
        };
        agent_runs.ensure_ticking(cx);
        agent_runs.start_resumes(cx);
        agent_runs
    }

    fn start_resumes(&mut self, cx: &mut Context<Self>) {
        if self.resume_queue.is_empty() {
            return;
        }
        let needs_mcp = self.resume_queue.iter().any(|run_id| {
            self.get(*run_id)
                .and_then(|run| {
                    self.settings
                        .agents
                        .iter()
                        .find(|agent| agent.id == run.agent_id)
                })
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
            this.update(cx, |runs, cx| runs.resume_next(cx)).ok();
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
        let Some(run) = self.get(run_id).filter(|run| run.is_running()).cloned() else {
            return;
        };
        self.resuming.insert(run_id);
        match self.resume_plan(&run, cx) {
            Ok((plan, format)) => {
                let stop = orphan_stop(&run, &plan);
                cx.spawn(async move |this, cx| {
                    if let Some(stop) = stop {
                        let _ =
                            runtime::spawn(async move { agents::run_stop_invocation(&stop).await })
                                .await;
                    }
                    this.update(cx, |runs, cx| {
                        let still_resuming = runs.resuming.contains(&run_id)
                            && runs.get(run_id).is_some_and(AgentRun::is_running);
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
            Err(reason) => self.finish(
                run_id,
                Some(RunOutcome {
                    status: RunStatus::Failed {
                        message: format!("Could not resume - {reason}"),
                        stderr_tail: Vec::new(),
                    },
                    exit_code: None,
                }),
                cx,
            ),
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

    fn persist(&self, cx: &App) {
        run_store::save_runs(&self.app.read(cx).store, &self.runs);
    }

    pub fn start(
        &mut self,
        action_id: &str,
        conversation_id: &str,
        message_id: &str,
        extra: &str,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let action = self
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
        let session_id = agents::new_session_id();
        let prompt = agents::build_prompt(&action, &context, extra, &session_id, &profile.id);
        let endpoint = mcp_bridge::endpoint(cx);
        let plan = agents::build_invocation(
            &profile,
            &action,
            &prompt,
            &session_id,
            endpoint.as_ref(),
            mcp_config_directory().as_deref(),
        );
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
        });
        if endpoint.is_none()
            && let Some(run) = self.run_mut(run_id)
        {
            run.push_step("MCP server is off - no reply proposal possible".to_owned());
        }
        match plan {
            Ok(plan) => self.spawn(run_id, plan, profile.output, cx),
            Err(error) => self.finish(
                run_id,
                Some(RunOutcome {
                    status: RunStatus::Failed {
                        message: error.to_string(),
                        stderr_tail: Vec::new(),
                    },
                    exit_code: None,
                }),
                cx,
            ),
        }
        self.persist(cx);
        self.ensure_ticking(cx);
        cx.notify();
        Ok(run_id)
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
            RunEvent::Step(text) => run.push_step(text),
            RunEvent::Text(text) => {
                run.current_step = truncated(first_line(&text), TEXT_STEP_CHARS)
            }
            _ => return,
        }
        cx.notify();
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
        if self.open_after_stop.remove(&run_id) && stopped {
            open_run_session(&snapshot, cx);
        }
        if was_resuming {
            self.resume_next(cx);
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
        } else if self.resuming.contains(&run_id) || self.resume_queue.contains(&run_id) {
            if self.resume_queue.contains(&run_id) {
                self.stop_orphan(run_id, cx);
            }
            self.resume_queue.retain(|queued| *queued != run_id);
            self.finish(
                run_id,
                Some(RunOutcome {
                    status: RunStatus::Stopped,
                    exit_code: None,
                }),
                cx,
            );
        }
    }

    fn stop_orphan(&self, run_id: u64, cx: &App) {
        let Some(run) = self.get(run_id) else {
            return;
        };
        let Ok((plan, _)) = self.resume_plan(run, cx) else {
            return;
        };
        for path in &plan.cleanup {
            let _ = std::fs::remove_file(path);
        }
        if let Some(stop) = orphan_stop(run, &plan) {
            drop(runtime::spawn(async move {
                agents::run_stop_invocation(&stop).await
            }));
        }
    }

    pub fn stop_and_open_session(&mut self, run_id: u64, cx: &mut Context<Self>) {
        self.open_after_stop.insert(run_id);
        self.stop(run_id, cx);
    }

    pub fn retry(&mut self, run_id: u64, cx: &mut Context<Self>) -> Result<u64, String> {
        let run = self
            .get(run_id)
            .cloned()
            .ok_or("This run no longer exists")?;
        let started = self.start(
            &run.action_id,
            &run.conversation_id,
            &run.message_id,
            &run.extra,
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

    use super::{AgentRun, AgentRuns, RunState, Utc, format_elapsed, run_store};
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
        use gpui_kit::AppContext as _;

        use super::{AgentRuns, RunState, TestAppContext, run_store, setup, wait_until_finished};

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
                assert_eq!(run.session_id, "session-5");
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
