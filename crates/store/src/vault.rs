//! Secrets (OAuth tokens, API keys, the MCP token) kept apart from the cache database.
//!
//! The app installs an OS keyring vault with [`Store::with_vault`]. A store without one (tests,
//! the CLI, demo mode) keeps a secret in the `meta` table under a `secret:` key, as before.

use std::sync::Arc;

use crate::error::Result;
use crate::store::Store;

const META_PREFIX: &str = "secret:";

/// Where a store keeps its secrets. An implementation handles its own backend failures,
/// e.g. by keeping a value in memory for the session, and never writes a secret into the database.
pub trait SecretVault: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
    fn delete(&self, key: &str);
}

impl Store {
    /// Keeps secrets in `vault` instead of the `meta` table.
    pub fn with_vault(mut self, vault: Arc<dyn SecretVault>) -> Self {
        self.vault = Some(vault);
        self
    }

    pub fn secret(&self, key: &str) -> Result<Option<String>> {
        match &self.vault {
            Some(vault) => Ok(vault.get(key)),
            None => self.meta(&meta_key(key)),
        }
    }

    pub fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        match &self.vault {
            Some(vault) => {
                vault.set(key, value);
                Ok(())
            }
            None => self.set_meta(&meta_key(key), value),
        }
    }

    pub fn delete_secret(&self, key: &str) -> Result<()> {
        match &self.vault {
            Some(vault) => {
                vault.delete(key);
                Ok(())
            }
            None => self.delete_meta(&meta_key(key)),
        }
    }
}

fn meta_key(key: &str) -> String {
    format!("{META_PREFIX}{key}")
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// A vault in memory, shared with the test through `Arc`.
    #[derive(Default)]
    pub(crate) struct MemoryVault(pub(crate) Mutex<HashMap<String, String>>);

    impl SecretVault for MemoryVault {
        fn get(&self, key: &str) -> Option<String> {
            self.0.lock().unwrap().get(key).cloned()
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

    #[test]
    fn without_a_vault_secrets_round_trip_through_meta() {
        let store = Store::open_in_memory().unwrap();
        store.set_secret("mcp/token", "abc").unwrap();
        assert_eq!(store.secret("mcp/token").unwrap().as_deref(), Some("abc"));
        store.delete_secret("mcp/token").unwrap();
        assert_eq!(store.secret("mcp/token").unwrap(), None);
    }

    #[test]
    fn with_a_vault_secrets_never_reach_the_database() {
        let vault = Arc::new(MemoryVault::default());
        let store = Store::open_in_memory().unwrap().with_vault(vault.clone());
        store.set_secret("mcp/token", "abc").unwrap();
        assert_eq!(store.secret("mcp/token").unwrap().as_deref(), Some("abc"));
        assert_eq!(store.meta("secret:mcp/token").unwrap(), None);
        assert_eq!(
            vault.0.lock().unwrap().get("mcp/token").map(String::as_str),
            Some("abc")
        );
        store.delete_secret("mcp/token").unwrap();
        assert!(vault.0.lock().unwrap().is_empty());
    }
}
