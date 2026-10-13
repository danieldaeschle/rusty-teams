use agents::{AgentSettings, Invocation, RunPlan};

use super::runs::AgentRun;
use crate::runtime;

pub fn orphan_stop(run: &AgentRun, plan: &RunPlan) -> Option<Invocation> {
    plan.stop.clone().or_else(|| pid_kill(run))
}

pub fn pid_kill(run: &AgentRun) -> Option<Invocation> {
    let launched_as = run.launch_session_id.as_ref().unwrap_or(&run.session_id);
    run.process_id
        .and_then(|process_id| agents::kill_process_invocation(process_id, launched_as))
}

pub fn startup_kill(settings: &AgentSettings, run: &AgentRun) -> Option<Invocation> {
    let launched_as = run.launch_session_id.as_ref().unwrap_or(&run.session_id);
    settings
        .agents
        .iter()
        .find(|agent| agent.id == run.agent_id)
        .and_then(|agent| agents::wsl_stop_invocation(&agent.environment, launched_as))
        .or_else(|| pid_kill(run))
}

pub fn kill_for(
    run: &AgentRun,
    plan: &RunPlan,
    plan_for_launch_id: impl Fn(&AgentRun) -> Option<RunPlan>,
) -> Option<Invocation> {
    let Some(launch_session_id) = run
        .launch_session_id
        .clone()
        .filter(|launch| *launch != run.session_id)
    else {
        return orphan_stop(run, plan);
    };
    let launched = AgentRun {
        session_id: launch_session_id,
        ..run.clone()
    };
    let launched_plan = plan_for_launch_id(&launched)?;
    for path in &launched_plan.cleanup {
        let _ = std::fs::remove_file(path);
    }
    orphan_stop(&launched, &launched_plan)
}

pub fn orphan_kill(
    run: &AgentRun,
    plan_for_run: impl Fn(&AgentRun) -> Option<RunPlan>,
) -> Option<Invocation> {
    let plan = plan_for_run(run)?;
    for path in &plan.cleanup {
        let _ = std::fs::remove_file(path);
    }
    kill_for(run, &plan, plan_for_run)
}

pub async fn execute(stop: Invocation) {
    let _ = runtime::spawn(async move { agents::run_stop_invocation(&stop).await }).await;
}
