use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::error::InstallError;

pub(super) fn read_if_exists(path: &Path) -> Result<Option<String>, InstallError> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(InstallError::io(path, error)),
    }
}

fn sibling(path: &Path, prefix: &str, suffix: &str) -> PathBuf {
    let mut name = OsString::from(prefix);
    name.push(path.file_name().unwrap_or_default());
    name.push(suffix);
    path.with_file_name(name)
}

pub(super) fn write_atomic(path: &Path, contents: &str) -> Result<(), InstallError> {
    let existing = fs::metadata(path).ok();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| InstallError::io(parent, error))?;
    }
    if existing.is_some() {
        let backup = sibling(path, "", ".bak-rusty-teams");
        if !backup.exists() {
            fs::copy(path, &backup).map_err(|error| InstallError::io(&backup, error))?;
        }
    }
    let temporary = sibling(path, ".", ".rusty-teams.tmp");
    let result = write_temporary(&temporary, contents, existing.as_ref())
        .and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(InstallError::io(path, error));
    }
    Ok(())
}

fn write_temporary(
    temporary: &Path,
    contents: &str,
    existing: Option<&fs::Metadata>,
) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(temporary)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    if let Some(metadata) = existing {
        fs::set_permissions(temporary, metadata.permissions())?;
    }
    Ok(())
}
