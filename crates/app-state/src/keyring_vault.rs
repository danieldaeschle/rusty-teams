//! The OS keyring (Secret Service, Keychain, Credential Manager) as the store's secret vault.
//!
//! Secrets live under service `rusty-teams`, user `<database file>/<key>`, so each tenant
//! database keeps its own. When the keyring fails, a value stays in memory for this session
//! only: it is never written to the database instead (ADR 0019, ADR 0023).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use store::SecretVault;

const SERVICE: &str = "rusty-teams";

/// One keyring call. [`OsKeyring`] is the real one; tests inject their own.
pub trait KeyringBackend: Send + Sync + 'static {
    fn get(&self, user: &str) -> Result<Option<String>, String>;
    fn set(&self, user: &str, value: &str) -> Result<(), String>;
    fn delete(&self, user: &str) -> Result<(), String>;
}

pub struct OsKeyring;

impl KeyringBackend for OsKeyring {
    fn get(&self, user: &str) -> Result<Option<String>, String> {
        match entry(user)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, user: &str, value: &str) -> Result<(), String> {
        entry(user)?
            .set_password(value)
            .map_err(|error| error.to_string())
    }

    fn delete(&self, user: &str) -> Result<(), String> {
        match entry(user)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

fn entry(user: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, user).map_err(|error| error.to_string())
}

pub struct KeyringVault {
    namespace: String,
    backend: Arc<dyn KeyringBackend>,
    /// Read once per key: callers load secrets often, and a locked keyring may prompt.
    cache: Mutex<HashMap<String, Option<String>>>,
    /// Keys whose read failed this session: their stored value is unknown, so a delete
    /// (a save that saw an empty value) must not wipe it.
    unreadable: Mutex<HashSet<String>>,
}

impl KeyringVault {
    /// The vault of the database file named `namespace`, e.g. `cache.sqlite3`.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self::with_backend(namespace, Arc::new(OsKeyring))
    }

    pub fn with_backend(namespace: impl Into<String>, backend: Arc<dyn KeyringBackend>) -> Self {
        KeyringVault {
            namespace: namespace.into(),
            backend,
            cache: Mutex::new(HashMap::new()),
            unreadable: Mutex::new(HashSet::new()),
        }
    }

    fn unreadable(&self) -> MutexGuard<'_, HashSet<String>> {
        self.unreadable
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn cache(&self) -> MutexGuard<'_, HashMap<String, Option<String>>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The keyring crate warns that calls on a thread inside the tokio runtime can deadlock
    /// with the async Secret Service backend, so each call runs on a thread of its own.
    fn call<T: Send + 'static>(
        &self,
        key: &str,
        operation: impl FnOnce(&dyn KeyringBackend, &str) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let backend = self.backend.clone();
        let user = format!("{}/{key}", self.namespace);
        let outcome = std::thread::spawn(move || operation(backend.as_ref(), &user))
            .join()
            .unwrap_or_else(|_| Err("the keyring call panicked".to_owned()));
        if let Err(error) = &outcome {
            log::warn!("keyring unavailable for secret {key}: {error}");
        }
        outcome
    }
}

impl SecretVault for KeyringVault {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        if let Some(cached) = self.cache().get(key) {
            return Ok(cached.clone());
        }
        match self.call(key, |backend, user| backend.get(user)) {
            Ok(value) => {
                self.unreadable().remove(key);
                self.cache().insert(key.to_owned(), value.clone());
                Ok(value)
            }
            // Not cached: the next read asks the keyring again (it may have been unlocked).
            Err(error) => {
                self.unreadable().insert(key.to_owned());
                Err(error)
            }
        }
    }

    fn set(&self, key: &str, value: &str) {
        self.unreadable().remove(key);
        self.cache().insert(key.to_owned(), Some(value.to_owned()));
        let value = value.to_owned();
        let _ = self.call(key, move |backend, user| backend.set(user, &value));
    }

    fn delete(&self, key: &str) {
        if self.unreadable().contains(key) {
            log::warn!(
                "keeping secret {key}: it could not be read, so it is not known to be stale"
            );
            return;
        }
        self.cache().insert(key.to_owned(), None);
        let _ = self.call(key, |backend, user| backend.delete(user));
    }
}

#[cfg(test)]
mod tests {
    use std::thread::ThreadId;

    use super::*;

    #[derive(Default)]
    struct FakeKeyring {
        entries: Mutex<HashMap<String, String>>,
        threads: Mutex<Vec<ThreadId>>,
    }

    impl FakeKeyring {
        fn record_thread(&self) {
            self.threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
        }
    }

    impl KeyringBackend for FakeKeyring {
        fn get(&self, user: &str) -> Result<Option<String>, String> {
            self.record_thread();
            Ok(self.entries.lock().unwrap().get(user).cloned())
        }
        fn set(&self, user: &str, value: &str) -> Result<(), String> {
            self.record_thread();
            self.entries
                .lock()
                .unwrap()
                .insert(user.to_owned(), value.to_owned());
            Ok(())
        }
        fn delete(&self, user: &str) -> Result<(), String> {
            self.record_thread();
            self.entries.lock().unwrap().remove(user);
            Ok(())
        }
    }

    struct BrokenKeyring;

    impl KeyringBackend for BrokenKeyring {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Err("no Secret Service".to_owned())
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Err("no Secret Service".to_owned())
        }
        fn delete(&self, _: &str) -> Result<(), String> {
            Err("no Secret Service".to_owned())
        }
    }

    #[test]
    fn secrets_persist_in_the_keyring_per_database() {
        let keyring = Arc::new(FakeKeyring::default());
        let home = KeyringVault::with_backend("cache.sqlite3", keyring.clone());
        home.set("mcp/token", "abc");
        let reopened = KeyringVault::with_backend("cache.sqlite3", keyring.clone());
        assert_eq!(reopened.get("mcp/token").unwrap().as_deref(), Some("abc"));
        let guest = KeyringVault::with_backend("cache-t2.sqlite3", keyring.clone());
        assert_eq!(guest.get("mcp/token").unwrap(), None);
        reopened.delete("mcp/token");
        assert!(keyring.entries.lock().unwrap().is_empty());
    }

    #[test]
    fn keyring_calls_run_off_the_calling_thread() {
        let keyring = Arc::new(FakeKeyring::default());
        let vault = KeyringVault::with_backend("cache.sqlite3", keyring.clone());
        vault.set("k", "v");
        let caller = std::thread::current().id();
        assert!(
            keyring
                .threads
                .lock()
                .unwrap()
                .iter()
                .all(|thread| *thread != caller)
        );
    }

    #[test]
    fn a_broken_keyring_keeps_the_value_for_this_session_only() {
        let vault = KeyringVault::with_backend("cache.sqlite3", Arc::new(BrokenKeyring));
        assert!(vault.get("google-calendar/g1").is_err());
        vault.set("google-calendar/g1", "tokens");
        assert_eq!(
            vault.get("google-calendar/g1").unwrap().as_deref(),
            Some("tokens")
        );
        let next_start = KeyringVault::with_backend("cache.sqlite3", Arc::new(BrokenKeyring));
        assert!(next_start.get("google-calendar/g1").is_err());
    }

    /// A keyring that fails reads until unlocked, like a locked Secret Service collection.
    #[derive(Default)]
    struct LockedKeyring {
        inner: FakeKeyring,
        unlocked: std::sync::atomic::AtomicBool,
    }

    impl KeyringBackend for LockedKeyring {
        fn get(&self, user: &str) -> Result<Option<String>, String> {
            if self.unlocked.load(std::sync::atomic::Ordering::SeqCst) {
                self.inner.get(user)
            } else {
                Err("collection is locked".to_owned())
            }
        }
        fn set(&self, user: &str, value: &str) -> Result<(), String> {
            self.inner.set(user, value)
        }
        fn delete(&self, user: &str) -> Result<(), String> {
            self.inner.delete(user)
        }
    }

    /// Regression (review): a failed read was cached as "no secret" and the next save deleted the real one.
    #[test]
    fn a_failed_read_neither_sticks_nor_lets_a_save_delete_the_secret() {
        let keyring = Arc::new(LockedKeyring::default());
        keyring
            .inner
            .set("cache.sqlite3/google-calendar/g1", "tokens")
            .unwrap();
        let vault = KeyringVault::with_backend("cache.sqlite3", keyring.clone());
        assert!(vault.get("google-calendar/g1").is_err());
        vault.delete("google-calendar/g1");
        assert_eq!(
            keyring
                .inner
                .entries
                .lock()
                .unwrap()
                .get("cache.sqlite3/google-calendar/g1")
                .map(String::as_str),
            Some("tokens")
        );
        keyring
            .unlocked
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            vault.get("google-calendar/g1").unwrap().as_deref(),
            Some("tokens")
        );
    }

    #[test]
    fn a_store_with_the_vault_never_writes_secrets_to_the_database() {
        let keyring = Arc::new(FakeKeyring::default());
        let store = store::Store::open_in_memory().unwrap().with_vault(Arc::new(
            KeyringVault::with_backend("cache.sqlite3", keyring.clone()),
        ));
        store.set_secret("mcp/token", "abc").unwrap();
        assert_eq!(store.meta("secret:mcp/token").unwrap(), None);
        assert_eq!(
            keyring
                .entries
                .lock()
                .unwrap()
                .get("cache.sqlite3/mcp/token")
                .map(String::as_str),
            Some("abc")
        );
    }
}
