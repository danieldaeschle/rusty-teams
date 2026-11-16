use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use directories::ProjectDirs;
use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::migrations;

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

fn thread_label() -> String {
    let thread = std::thread::current();
    thread
        .name()
        .map_or_else(|| format!("{:?}", thread.id()), str::to_owned)
}

pub(crate) fn json_ids(ids: &[String]) -> String {
    serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_owned())
}
