use std::collections::BTreeSet;

use rand::RngCore;
use store::Store;

use crate::policy::{Access, AgentPolicy, DEFAULT_PORT};

const SETTINGS_KEY: &str = "agents.settings";
/// Where versions before the secret vault kept the token, in plain text.
const TOKEN_KEY: &str = "agents.token";
/// Vault key of the bearer token agents send to the MCP server.
const TOKEN_SECRET: &str = "mcp/token";
const TOKEN_BYTES: usize = 32;
const ALL_KEYWORD: &str = "all";
const SELECTED_PREFIX: &str = "selected:";

pub fn load(store: &Store) -> AgentPolicy {
    store
        .meta(SETTINGS_KEY)
        .ok()
        .flatten()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save(store: &Store, policy: &AgentPolicy) {
    let _ = store.set_meta(SETTINGS_KEY, &serialize(policy));
}

pub fn load_or_create_token(store: &Store) -> store::Result<String> {
    if let Some(token) = store
        .secret(TOKEN_SECRET)?
        .filter(|token| !token.is_empty())
    {
        return Ok(token);
    }
    if let Some(token) = store.meta(TOKEN_KEY)?.filter(|token| !token.is_empty()) {
        // Same token, so agents configured before the move keep working.
        store.set_secret(TOKEN_SECRET, &token)?;
        store.delete_meta(TOKEN_KEY)?;
        return Ok(token);
    }
    regenerate_token(store)
}

pub fn regenerate_token(store: &Store) -> store::Result<String> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    store.set_secret(TOKEN_SECRET, &token)?;
    store.delete_meta(TOKEN_KEY)?;
    Ok(token)
}

fn serialize_access(access: &Access) -> String {
    match access {
        Access::None => String::new(),
        Access::All => ALL_KEYWORD.to_owned(),
        Access::Selected(ids) => format!(
            "{SELECTED_PREFIX}{}",
            ids.iter().cloned().collect::<Vec<_>>().join(",")
        ),
    }
}

fn parse_access(value: &str) -> Access {
    if value == ALL_KEYWORD {
        return Access::All;
    }
    let Some(list) = value.strip_prefix(SELECTED_PREFIX) else {
        return Access::None;
    };
    let ids: BTreeSet<String> = list
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    Access::Selected(ids)
}

fn serialize(policy: &AgentPolicy) -> String {
    [
        ("enabled", policy.enabled.to_string()),
        ("port", policy.port.to_string()),
        ("chats", serialize_access(&policy.chats)),
        ("channels", serialize_access(&policy.channels)),
        ("write", policy.write.to_string()),
        ("review", policy.review.to_string()),
        ("calendar", policy.calendar.to_string()),
        ("transcripts", policy.transcripts.to_string()),
        ("meetings", policy.meetings.to_string()),
    ]
    .iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect::<Vec<_>>()
    .join("\n")
}

fn parse(text: &str) -> AgentPolicy {
    let mut policy = AgentPolicy::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let flag = value == "true";
        match key {
            "enabled" => policy.enabled = flag,
            "port" => policy.port = value.parse().unwrap_or(DEFAULT_PORT),
            "chats" => policy.chats = parse_access(value),
            "channels" => policy.channels = parse_access(value),
            "write" | "propose" => policy.write = flag,
            "review" => policy.review = flag,
            "calendar" => policy.calendar = flag,
            "transcripts" => policy.transcripts = flag,
            "meetings" => policy.meetings = flag,
            _ => {}
        }
    }
    policy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_field() {
        let policy = AgentPolicy {
            enabled: true,
            port: 50123,
            chats: Access::Selected(
                ["19:abc@thread.v2", "19:def@unq.gbl.spaces"]
                    .iter()
                    .map(|id| (*id).to_owned())
                    .collect(),
            ),
            channels: Access::All,
            write: true,
            review: false,
            calendar: true,
            transcripts: true,
            meetings: true,
        };
        assert_eq!(parse(&serialize(&policy)), policy);
    }

    #[test]
    fn empty_selection_round_trips() {
        let policy = AgentPolicy {
            chats: Access::Selected(BTreeSet::new()),
            ..AgentPolicy::default()
        };
        assert_eq!(parse(&serialize(&policy)), policy);
    }

    #[test]
    fn older_settings_keep_writing_and_review_it() {
        let policy = parse("enabled=true\npropose=true");
        assert!(policy.write && policy.review);
    }

    #[test]
    fn garbage_falls_back_to_default() {
        assert_eq!(parse("\u{0}\u{1}nonsense\n==\n"), AgentPolicy::default());
        let policy = parse("port=banana\nchats=everything\nenabled=maybe");
        assert_eq!(policy, AgentPolicy::default());
    }

    #[test]
    fn saves_and_loads_through_the_store() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load(&store), AgentPolicy::default());
        let policy = AgentPolicy {
            enabled: true,
            write: true,
            ..AgentPolicy::default()
        };
        save(&store, &policy);
        assert_eq!(load(&store), policy);
    }

    #[test]
    fn token_is_created_once_and_regenerated_on_demand() {
        let store = Store::open_in_memory().unwrap();
        let first = load_or_create_token(&store).unwrap();
        assert_eq!(first.len(), TOKEN_BYTES * 2);
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
        assert_eq!(load_or_create_token(&store).unwrap(), first);
        let second = regenerate_token(&store).unwrap();
        assert_ne!(second, first);
        assert_eq!(load_or_create_token(&store).unwrap(), second);
    }

    #[derive(Default)]
    struct MemoryVault(std::sync::Mutex<std::collections::HashMap<String, String>>);

    impl store::SecretVault for MemoryVault {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_owned(), value.to_owned());
        }
        fn delete(&self, key: &str) {
            self.0.lock().unwrap().remove(key);
        }
    }

    struct LockedVault;

    impl store::SecretVault for LockedVault {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Err("collection is locked".to_owned())
        }
        fn set(&self, _: &str, _: &str) {
            panic!("a locked keyring must not get a new token");
        }
        fn delete(&self, _: &str) {}
    }

    /// Regression (review): an unreadable keyring looked empty, so a new token replaced the one agents use.
    #[test]
    fn an_unreadable_token_is_an_error_not_a_new_token() {
        let store = Store::open_in_memory()
            .unwrap()
            .with_vault(std::sync::Arc::new(LockedVault));
        assert!(load_or_create_token(&store).is_err());
    }

    /// Regression: the token sat in plain text in the cache database.
    #[test]
    fn a_plain_token_moves_into_the_vault_and_stays_the_same() {
        let vault = std::sync::Arc::new(MemoryVault::default());
        let store = Store::open_in_memory().unwrap().with_vault(vault.clone());
        store.set_meta(TOKEN_KEY, "legacy-token").unwrap();
        assert_eq!(load_or_create_token(&store).unwrap(), "legacy-token");
        assert_eq!(store.meta(TOKEN_KEY).unwrap(), None);
        assert_eq!(
            vault
                .0
                .lock()
                .unwrap()
                .get(TOKEN_SECRET)
                .map(String::as_str),
            Some("legacy-token")
        );
        let fresh = regenerate_token(&store).unwrap();
        assert_eq!(store.meta(TOKEN_KEY).unwrap(), None);
        assert_eq!(load_or_create_token(&store).unwrap(), fresh);
    }
}
