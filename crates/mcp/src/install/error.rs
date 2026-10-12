use std::path::PathBuf;

use super::Target;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{} is not installed on this machine", .0.label())]
    NotAvailable(Target),
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid: {message}")]
    Malformed { path: PathBuf, message: String },
}

impl InstallError {
    pub(super) fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        InstallError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(super) fn malformed(path: &std::path::Path, message: impl ToString) -> Self {
        InstallError::Malformed {
            path: path.to_path_buf(),
            message: message.to_string(),
        }
    }
}
