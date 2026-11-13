use chrono::Local;

use super::*;

const WAKEUPS_KEY: &str = "agents.wakeups";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWakeup {
    pub session_id: String,
    pub due_at: DateTime<Utc>,
    pub prompt: String,
    pub reason: String,
}

impl PendingWakeup {
    fn step(&self) -> String {
        format!(
            "Wakes up at {} - {}",
            self.due_at
                .with_timezone(&Local)
                .format(app_base::format::clock_format()),
            self.reason
        )
    }

    fn prompt_text(&self) -> String {
        format!("Scheduled wake-up ({}): {}", self.reason, self.prompt)
    }
}

pub(super) fn load(store: &store::Store) -> Vec<PendingWakeup> {
    store
        .meta(WAKEUPS_KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save(store: &store::Store, wakeups: &[PendingWakeup]) {
    if let Ok(json) = serde_json::to_string(wakeups) {
        let _ = store.set_meta(WAKEUPS_KEY, &json);
    }
}

impl AgentRuns {
    pub fn schedule_wakeup(&mut self, wakeup: mcp::Wakeup, cx: &mut Context<Self>) {
        let pending = PendingWakeup {
            session_id: wakeup.session_id,
            due_at: wakeup.due_at,
            prompt: wakeup.prompt,
            reason: wakeup.reason,
        };
        self.wakeups
            .retain(|existing| existing.session_id != pending.session_id);
        self.wakeups.push(pending.clone());
        self.save_wakeups(cx);
        if let Some(run_id) = self.latest_run_of(&pending.session_id)
            && let Some(run) = self.run_mut(run_id)
        {
            run.push_step(pending.step());
        }
        self.arm(pending, cx);
        self.persist(cx);
        cx.notify();
    }

    pub fn wakeup_of(&self, session_id: &str) -> Option<&PendingWakeup> {
        self.wakeups
            .iter()
            .find(|wakeup| wakeup.session_id == session_id)
    }

    pub(super) fn wakeup_step(&self, session_id: &str) -> Option<String> {
        self.wakeup_of(session_id).map(PendingWakeup::step)
    }

    pub(super) fn app_session_ids(&self) -> HashSet<String> {
        self.runs
            .iter()
            .filter(|run| run.launched)
            .map(|run| run.session_id.clone())
            .collect()
    }

    pub(super) fn arm_wakeups(&mut self, cx: &mut Context<Self>) {
        for wakeup in self.wakeups.clone() {
            self.arm(wakeup, cx);
        }
    }

    pub(super) fn cancel_wakeup(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let before = self.wakeups.len();
        self.wakeups
            .retain(|wakeup| wakeup.session_id != session_id);
        if self.wakeups.len() != before {
            self.save_wakeups(cx);
        }
    }

    fn arm(&self, wakeup: PendingWakeup, cx: &mut Context<Self>) {
        let delay = (wakeup.due_at - Utc::now()).to_std().unwrap_or_default();
        let needs_mcp = self.uses_mcp(&wakeup.session_id);
        cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let mut waited = Duration::ZERO;
            while needs_mcp
                && waited < MCP_WAIT_LIMIT
                && !cx.update(|cx| mcp_bridge::endpoint(cx).is_some())
            {
                cx.background_executor().timer(MCP_POLL_INTERVAL).await;
                waited += MCP_POLL_INTERVAL;
            }
            this.update(cx, |runs, cx| runs.fire(&wakeup, cx)).ok();
        })
        .detach();
    }

    fn fire(&mut self, wakeup: &PendingWakeup, cx: &mut Context<Self>) {
        if !self.wakeups.contains(wakeup) {
            return;
        }
        self.cancel_wakeup(&wakeup.session_id, cx);
        let Some(origin) = self
            .latest_run_of(&wakeup.session_id)
            .and_then(|run_id| self.get(run_id).cloned())
        else {
            return;
        };
        let run_id = self.next_id;
        self.next_id += 1;
        let mut run = AgentRun {
            id: run_id,
            state: RunState::Running,
            current_step: String::new(),
            steps: Vec::new(),
            final_text: String::new(),
            started_at: Utc::now(),
            ended_at: None,
            expanded: false,
            dismissed: false,
            process_id: None,
            pending_prompt: Some(wakeup.prompt_text()),
            sent_to: Some(SessionTarget {
                session_id: origin.session_id.clone(),
                working_dir: origin.working_dir.clone(),
                inbox: None,
            }),
            launch_session_id: None,
            launched: false,
            ..origin
        };
        run.push_step(format!("Woke up - {}", wakeup.reason));
        self.runs.push(run);
        self.dispatch(run_id, cx);
        self.persist(cx);
        self.ensure_ticking(cx);
        cx.notify();
    }

    fn uses_mcp(&self, session_id: &str) -> bool {
        self.latest_run_of(session_id)
            .and_then(|run_id| self.get(run_id))
            .and_then(|run| {
                self.settings
                    .agents
                    .iter()
                    .find(|agent| agent.id == run.agent_id)
            })
            .is_some_and(|agent| !agent.mcp_config_flag.is_empty())
    }

    fn latest_run_of(&self, session_id: &str) -> Option<u64> {
        self.runs
            .iter()
            .filter(|run| run.session_id == session_id)
            .map(|run| run.id)
            .max()
    }

    pub(super) fn save_wakeups(&self, cx: &App) {
        save(&self.app.read(cx).store, &self.wakeups);
    }
}
