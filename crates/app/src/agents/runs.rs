use std::collections::{HashMap, HashSet};
use std::time::Duration;

use agents::{AgentSettings, RunEvent, RunOutcome, RunStatus, RunUpdate};
use chrono::{DateTime, Utc};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use super::{
    StartRequest, demo, mcp_bridge, mcp_config_directory, message_context, open_run_session,
    store as run_store,
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
    open_after_stop: HashSet<u64>,
    next_id: u64,
    ticking: bool,
}

impl EventEmitter<AgentRunsEvent> for AgentRuns {}

impl AgentRuns {
    pub fn new(app: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let store = app.read(cx).store.clone();
        let mut runs = run_store::load_runs(&store);
        let interrupted = runs.iter().any(AgentRun::is_running);
        for run in runs.iter_mut().filter(|run| run.is_running()) {
            run.mark_interrupted();
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
            settings: run_store::load_settings(&store),
            runs,
            stops: HashMap::new(),
            open_after_stop: HashSet::new(),
            next_id,
            ticking: false,
        };
        agent_runs.ensure_ticking(cx);
        agent_runs
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
        let RunUpdate::Event(event) = update else {
            return;
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
        let Some(run) = self.run_mut(run_id).filter(|run| run.is_running()) else {
            return;
        };
        run.ended_at = Some(Utc::now());
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
        if let Some(stop) = self.stops.remove(&run_id) {
            let _ = stop.send(());
            if let Some(run) = self.run_mut(run_id) {
                run.current_step = "Stopping".to_owned();
            }
            cx.notify();
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
        use super::{RunState, TestAppContext, run_store, setup, wait_until_finished};

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

    #[gpui_kit::test]
    fn runs_loaded_as_running_become_interrupted(cx: &mut TestAppContext) {
        let (app, runs) = setup(cx, "true");
        cx.update(|cx| {
            let mut running = finished_run(5);
            running.state = RunState::Running;
            running.ended_at = None;
            runs.update(cx, |runs, cx| {
                runs.runs.push(running);
                runs.persist(cx);
            });
            let reloaded = cx.new(|cx| AgentRuns::new(app.clone(), cx));
            let run = reloaded.read(cx).get(5).unwrap().clone();
            assert!(matches!(
                run.state,
                RunState::Failed { ref message, .. } if message == "Interrupted - the app was closed during the run"
            ));
            assert!(run.ended_at.is_some());
            assert_eq!(run.session_id, "session-5");
            assert_eq!(reloaded.read(cx).running_count(), 0);
        });
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
