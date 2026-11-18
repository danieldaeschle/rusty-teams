//! Inside a Flatpak the host's browsers are neither visible nor startable from the sandbox, so
//! lookups and processes go to the host through `flatpak-spawn --host`. The debug port and the
//! profile under `~/.var/app` are the same on both sides.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const FLATPAK_INFO: &str = "/.flatpak-info";
const LIST_DIRECTORIES: &str = r#"for directory in "$@"; do for entry in "$directory"/*; do [ -f "$entry" ] && printf '%s\n' "$entry"; done; done"#;

pub(crate) fn in_flatpak() -> bool {
    static IN_FLATPAK: OnceLock<bool> = OnceLock::new();
    *IN_FLATPAK.get_or_init(|| Path::new(FLATPAK_INFO).exists())
}

/// `program` on the host: wrapped in `flatpak-spawn --host` inside a Flatpak.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    if in_flatpak() {
        let mut command = Command::new("flatpak-spawn");
        command.arg("--host").arg(program);
        command
    } else {
        Command::new(program)
    }
}

/// The host's `PATH`, read once inside a Flatpak.
pub(crate) fn path_variable() -> OsString {
    if !in_flatpak() {
        return std::env::var_os("PATH").unwrap_or_default();
    }
    static HOST_PATH: OnceLock<OsString> = OnceLock::new();
    HOST_PATH
        .get_or_init(|| {
            command("printenv")
                .arg("PATH")
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().into())
                .unwrap_or_default()
        })
        .clone()
}

/// Whether `path` is a file on the host. Files in the host's `PATH` directories are listed in
/// one call, so probing every browser name in every directory stays cheap.
pub(crate) fn is_file(path: &Path) -> bool {
    if !in_flatpak() {
        return path.is_file();
    }
    let (directories, files) = host_path_files();
    if path
        .parent()
        .is_some_and(|parent| directories.contains(parent))
    {
        return files.contains(path);
    }
    command("test")
        .arg("-f")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

fn host_path_files() -> &'static (HashSet<PathBuf>, HashSet<PathBuf>) {
    static LISTING: OnceLock<(HashSet<PathBuf>, HashSet<PathBuf>)> = OnceLock::new();
    LISTING.get_or_init(|| {
        let directories: HashSet<PathBuf> = std::env::split_paths(&path_variable()).collect();
        let output = command("sh")
            .args(["-c", LIST_DIRECTORIES, "sh"])
            .args(&directories)
            .output();
        let files = output
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        (directories, files)
    })
}
