use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use directories::ProjectDirs;
use rusqlite::{Connection, OpenFlags};

use crate::error::{Error, Result};
use crate::migrations;

pub const DATA_DIR_NAME: &str = "ms-teams-linux";
const DATABASE_FILE: &str = "cache.sqlite3";
const SLOW_WAIT: Duration = Duration::from_millis(50);
const SLOW_HOLD: Duration = Duration::from_millis(100);
const READERS: usize = 3;

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
    /// WAL lets these read while the writer holds `connection`; empty for an in-memory store.
    readers: Vec<Mutex<Connection>>,
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
        let mut store = Self::from_connection(connection)?;
        store.readers = (0..READERS)
            .map(|_| open_reader(path).map(Mutex::new))
            .collect::<Result<_>>()?;
        Ok(store)
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
            readers: Vec::new(),
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
        guard(connection, location, asked)
    }

    /// A read-only connection, so a long write elsewhere does not block the caller.
    #[track_caller]
    pub(crate) fn read(&self) -> Result<StoreGuard<'_>> {
        let location = Location::caller();
        let asked = Instant::now();
        let Some(first) = self.readers.first() else {
            let connection = self.connection.lock().map_err(|_| Error::Poisoned)?;
            return guard(connection, location, asked);
        };
        let free = self
            .readers
            .iter()
            .find_map(|reader| reader.try_lock().ok());
        let connection = match free {
            Some(connection) => connection,
            None => first.lock().map_err(|_| Error::Poisoned)?,
        };
        guard(connection, location, asked)
    }
}

fn open_reader(path: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

fn guard<'a>(
    connection: MutexGuard<'a, Connection>,
    location: &'static Location<'static>,
    asked: Instant,
) -> Result<StoreGuard<'a>> {
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

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn a_read_does_not_wait_for_a_held_write_lock() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("cache.sqlite3")).unwrap();
        store.set_meta("key", "before").unwrap();
        let writer = store.lock().unwrap();
        writer
            .execute_batch("BEGIN IMMEDIATE; UPDATE meta SET value = 'during' WHERE key = 'key';")
            .unwrap();
        assert_eq!(store.meta("key").unwrap().as_deref(), Some("before"));
        writer.execute_batch("COMMIT;").unwrap();
        drop(writer);
        assert_eq!(store.meta("key").unwrap().as_deref(), Some("during"));
    }
}
