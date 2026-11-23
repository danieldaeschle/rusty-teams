use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use directories::ProjectDirs;
use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::migrations;
use crate::vault::SecretVault;

pub const DATA_DIR_NAME: &str = "ms-teams-linux";
const DATABASE_FILE: &str = "cache.sqlite3";
const SLOW_WAIT: Duration = Duration::from_millis(50);
const SLOW_HOLD: Duration = Duration::from_millis(100);

static SLOW_LOCK_REPORTER: OnceLock<fn(String)> = OnceLock::new();

pub fn report_slow_locks_to(reporter: fn(String)) {
    let _ = SLOW_LOCK_REPORTER.set(reporter);
}

fn report_slow_lock(message: String) {
    if let Some(reporter) = SLOW_LOCK_REPORTER.get() {
        reporter(message);
    }
}

pub fn default_database_path() -> Result<PathBuf> {
    let directories = ProjectDirs::from("", "", DATA_DIR_NAME).ok_or(Error::NoDataDirectory)?;
    Ok(directories.data_dir().join(DATABASE_FILE))
}

pub struct Store {
    connection: Mutex<Connection>,
    pub(crate) vault: Option<Arc<dyn SecretVault>>,
}

impl Store {
    pub fn open_default() -> Result<Self> {
        Self::open(&default_database_path()?)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|source| Error::DataDirectory {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let connection = Connection::open(path)?;
        restrict_to_owner(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        migrations::migrate(&mut connection)?;
        Ok(Store {
            connection: Mutex::new(connection),
            vault: None,
        })
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .lock()?
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    #[track_caller]
    pub(crate) fn lock(&self) -> Result<StoreGuard<'_>> {
        let location = Location::caller();
        let asked = Instant::now();
        let connection = self.connection.lock().map_err(|_| Error::Poisoned)?;
        let waited = asked.elapsed();
        if waited >= SLOW_WAIT {
            report_slow_lock(format!(
                "store lock waited {} ms at {location} on {}",
                waited.as_millis(),
                thread_label()
            ));
        }
        Ok(StoreGuard {
            connection,
            location,
            taken: Instant::now(),
        })
    }
}

pub(crate) struct StoreGuard<'a> {
    connection: MutexGuard<'a, Connection>,
    location: &'static Location<'static>,
    taken: Instant,
}

impl Deref for StoreGuard<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.connection
    }
}

impl DerefMut for StoreGuard<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

impl Drop for StoreGuard<'_> {
    fn drop(&mut self) {
        let held = self.taken.elapsed();
        if held >= SLOW_HOLD {
            report_slow_lock(format!(
                "store lock held {} ms at {} on {}",
                held.as_millis(),
                self.location,
                thread_label()
            ));
        }
    }
}

/// The cache holds messages and, without a vault, secrets: owner-only, also for the WAL files
/// SQLite created before (new ones copy the database file's mode).
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    const OWNER_ONLY: u32 = 0o600;
    const SIDE_FILES: [&str; 2] = ["-wal", "-shm"];
    let mut files = vec![path.to_path_buf()];
    files.extend(SIDE_FILES.iter().map(|suffix| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    }));
    for file in files {
        match std::fs::set_permissions(&file, std::fs::Permissions::from_mode(OWNER_ONLY)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(Error::Permissions {
                    path: file.display().to_string(),
                    source: error,
                });
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<()> {
    Ok(())
}

fn thread_label() -> String {
    let thread = std::thread::current();
    thread
        .name()
        .map_or_else(|| format!("{:?}", thread.id()), str::to_owned)
}

pub(crate) fn json_ids(ids: &[String]) -> String {
    serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_owned())
}

#[cfg(all(test, unix))]
mod permission_tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    /// Regression: the cache database and its WAL file were readable by other local users.
    #[test]
    fn the_database_and_its_wal_files_are_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.sqlite3");
        let store = Store::open(&path).unwrap();
        store.set_meta("k", "v").unwrap();
        for suffix in ["", "-wal", "-shm"] {
            let file = PathBuf::from(format!("{}{suffix}", path.display()));
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", file.display());
        }
    }

    #[test]
    fn reopening_restricts_files_left_readable_by_an_older_version() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.sqlite3");
        drop(Store::open(&path).unwrap());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        drop(Store::open(&path).unwrap());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
