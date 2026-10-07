use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use directories::ProjectDirs;
use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::migrations;

pub const DATA_DIR_NAME: &str = "ms-teams-linux";
const DATABASE_FILE: &str = "cache.sqlite3";

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

    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| Error::Poisoned)
    }
}

pub(crate) fn json_ids(ids: &[String]) -> String {
    serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_owned())
}
