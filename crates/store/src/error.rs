pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cannot create the data directory {path}: {source}")]
    DataDirectory {
        path: String,
        source: std::io::Error,
    },
    #[error("no platform data directory available")]
    NoDataDirectory,
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("image file cache: {0}")]
    ImageFiles(#[from] std::io::Error),
    #[error("database lock poisoned")]
    Poisoned,
}
