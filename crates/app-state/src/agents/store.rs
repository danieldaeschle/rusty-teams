use agents::{AgentSettings, SETTINGS_KEY};
use store::Store;

use super::runs::AgentRun;

const RUNS_KEY: &str = "agents.runs";
pub const RUNS_HISTORY_LIMIT: usize = 50;

/// The settings with the API keys of chat profiles, which live in the store's secret vault.
pub fn load_settings(store: &Store) -> AgentSettings {
    let Some(mut settings) = stored_settings(store) else {
        return AgentSettings::default();
    };
    let has_plain_keys = settings
        .agents
        .iter_mut()
        .any(|agent| agent.api_key_mut().is_some_and(|key| !key.is_empty()));
    if has_plain_keys {
        // Written before the vault: move the keys out of the database once.
        save_settings(store, &settings);
        return settings;
    }
    for agent in &mut settings.agents {
        let secret = store.secret(&api_key_secret(&agent.id)).ok().flatten();
        if let Some(key) = agent.api_key_mut() {
            *key = secret.unwrap_or_default();
        }
    }
    settings
}

/// Saves the settings without API keys; keys go to the secret vault, and the keys of removed
/// or emptied profiles are deleted from it.
pub fn save_settings(store: &Store, settings: &AgentSettings) {
    let previous_ids: Vec<String> = stored_settings(store)
        .map(|stored| stored.agents.into_iter().map(|agent| agent.id).collect())
        .unwrap_or_default();
    let mut without_keys = settings.clone();
    for agent in &mut without_keys.agents {
        let secret = api_key_secret(&agent.id);
        let Some(key) = agent.api_key_mut() else {
            let _ = store.delete_secret(&secret);
            continue;
        };
        let stored = if key.is_empty() {
            store.delete_secret(&secret)
        } else {
            store.set_secret(&secret, key)
        };
        if let Err(error) = stored {
            log::warn!("agent API key for {secret} not saved: {error}");
        }
        key.clear();
    }
    for id in previous_ids
        .iter()
        .filter(|id| !settings.agents.iter().any(|agent| &agent.id == *id))
    {
        let _ = store.delete_secret(&api_key_secret(id));
    }
    let _ = store.set_meta(SETTINGS_KEY, &without_keys.to_json());
}

fn stored_settings(store: &Store) -> Option<AgentSettings> {
    store
        .meta(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|json| AgentSettings::from_json(&json).ok())
}

/// Vault key of a chat profile's API key.
fn api_key_secret(profile_id: &str) -> String {
    format!("agents/{profile_id}/api-key")
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

    fn chat_profile(id: &str, api_key: &str) -> agents::AgentProfile {
        agents::AgentProfile {
            id: id.to_owned(),
            backend: agents::AgentBackend::Anthropic {
                model: "m".to_owned(),
                api_key: api_key.to_owned(),
            },
            ..agents::AgentProfile::default()
        }
    }

    /// Regression: provider API keys were stored in plain text in the cache database.
    #[test]
    fn api_keys_are_kept_out_of_the_settings_json() {
        let store = Store::open_in_memory().unwrap();
        let mut settings = AgentSettings::default();
        settings.agents.push(chat_profile("mine", "sk-secret"));
        save_settings(&store, &settings);
        let raw = store.meta(SETTINGS_KEY).unwrap().unwrap();
        assert!(!raw.contains("sk-secret"), "{raw}");
        assert_eq!(
            store.secret("agents/mine/api-key").unwrap().as_deref(),
            Some("sk-secret")
        );
        assert_eq!(load_settings(&store), settings);
    }

    #[test]
    fn plain_api_keys_from_an_older_version_move_out_on_load() {
        let store = Store::open_in_memory().unwrap();
        let mut legacy = AgentSettings::default();
        legacy.agents.push(chat_profile("mine", "sk-legacy"));
        store.set_meta(SETTINGS_KEY, &legacy.to_json()).unwrap();
        assert_eq!(load_settings(&store), legacy);
        let raw = store.meta(SETTINGS_KEY).unwrap().unwrap();
        assert!(!raw.contains("sk-legacy"), "{raw}");
        assert_eq!(load_settings(&store), legacy);
    }

    #[test]
    fn removing_a_profile_deletes_its_api_key() {
        let store = Store::open_in_memory().unwrap();
        let mut settings = AgentSettings::default();
        settings.agents.push(chat_profile("mine", "sk-secret"));
        save_settings(&store, &settings);
        settings.agents.retain(|agent| agent.id != "mine");
        save_settings(&store, &settings);
        assert_eq!(store.secret("agents/mine/api-key").unwrap(), None);
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
