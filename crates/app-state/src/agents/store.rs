use agents::{AgentSettings, SETTINGS_KEY};
use store::Store;

use super::runs::AgentRun;

const RUNS_KEY: &str = "agents.runs";
pub const RUNS_HISTORY_LIMIT: usize = 50;

pub fn load_settings(store: &Store) -> AgentSettings {
    store
        .meta(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|json| AgentSettings::from_json(&json).ok())
        .unwrap_or_default()
}

pub fn save_settings(store: &Store, settings: &AgentSettings) {
    let _ = store.set_meta(SETTINGS_KEY, &settings.to_json());
}

pub fn load_runs(store: &Store) -> Vec<AgentRun> {
    store
        .meta(RUNS_KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub fn save_runs(store: &Store, runs: &[AgentRun]) {
    let finished = runs.iter().filter(|run| run.is_finished()).count();
    let mut skipped = finished.saturating_sub(RUNS_HISTORY_LIMIT);
    let kept: Vec<&AgentRun> = runs
        .iter()
        .filter(|run| {
            if skipped > 0 && run.is_finished() {
                skipped -= 1;
                return false;
            }
            true
        })
        .collect();
    if let Ok(json) = serde_json::to_string(&kept) {
        let _ = store.set_meta(RUNS_KEY, &json);
    }
}

#[cfg(test)]
mod tests {
    use agents::AgentAction;

    use super::*;
    use crate::agents::fixtures::finished_run;

    #[test]
    fn settings_round_trip_and_default_when_missing() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load_settings(&store), AgentSettings::default());
        let mut settings = AgentSettings::default();
        settings.actions.push(AgentAction::build_this("copilot"));
        settings.actions[0].label = "Ask".to_owned();
        save_settings(&store, &settings);
        assert_eq!(load_settings(&store), settings);
    }

    #[test]
    fn broken_settings_json_falls_back_to_defaults() {
        let store = Store::open_in_memory().unwrap();
        store.set_meta(SETTINGS_KEY, "{not json").unwrap();
        assert_eq!(load_settings(&store), AgentSettings::default());
    }

    #[test]
    fn runs_round_trip_and_keep_the_last_fifty_finished() {
        let store = Store::open_in_memory().unwrap();
        assert!(load_runs(&store).is_empty());
        let runs: Vec<AgentRun> = (0..60).map(finished_run).collect();
        save_runs(&store, &runs);
        let loaded = load_runs(&store);
        assert_eq!(loaded.len(), RUNS_HISTORY_LIMIT);
        assert_eq!(loaded[0].id, 10);
        assert_eq!(loaded[49], runs[59]);
    }

    #[test]
    fn running_runs_are_persisted_and_load_as_interrupted() {
        let store = Store::open_in_memory().unwrap();
        let mut running = finished_run(1);
        running.state = crate::agents::runs::RunState::Running;
        running.ended_at = None;
        save_runs(&store, &[running, finished_run(2)]);
        let stored = load_runs(&store);
        assert_eq!(stored.len(), 2);
        assert!(stored[0].is_running());
    }
}
