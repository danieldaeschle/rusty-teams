#![cfg_attr(not(windows), allow(dead_code))]

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const POLL_INTERVAL: Duration = Duration::from_secs(15);

const VERSION_MARKER: &str = concat!("teams-build:", env!("TEAMS_BUILD_VERSION"));
const UPDATE_DIRECTORY: &str = "update";
const EXE_NAME: &str = "teams.exe";
const OLD_EXE_NAME: &str = "teams.old.exe";
const VERSION_FILE_NAME: &str = "version.txt";
const CLEANUP_ATTEMPTS: u32 = 30;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(1);

pub fn running_version() -> &'static str {
    VERSION_MARKER.trim_start_matches("teams-build:")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Copy)]
pub struct IdleInputs {
    pub window_active: bool,
    pub composer_empty: bool,
    pub send_in_flight: bool,
}

pub fn is_idle(inputs: IdleInputs) -> bool {
    !inputs.window_active && inputs.composer_empty && !inputs.send_in_flight
}

pub fn update_is_ready(running: &str, published: Option<&str>, update_exe_exists: bool) -> bool {
    update_exe_exists
        && published.is_some_and(|version| {
            let version = version.trim();
            !version.is_empty() && version != running
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePaths {
    pub exe: PathBuf,
    pub old_exe: PathBuf,
    pub update_exe: PathBuf,
    pub version_file: PathBuf,
}

impl UpdatePaths {
    pub fn beside(exe: &Path) -> UpdatePaths {
        let directory = exe.parent().unwrap_or_else(|| Path::new("."));
        let update_directory = directory.join(UPDATE_DIRECTORY);
        UpdatePaths {
            exe: exe.to_owned(),
            old_exe: directory.join(OLD_EXE_NAME),
            update_exe: update_directory.join(EXE_NAME),
            version_file: update_directory.join(VERSION_FILE_NAME),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOperation {
    RemoveIfExists(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
}

pub trait FileSystem {
    fn exists(&self, path: &Path) -> bool;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
}

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
}

pub fn swap_plan(paths: &UpdatePaths) -> Vec<FileOperation> {
    vec![
        FileOperation::RemoveIfExists(paths.old_exe.clone()),
        FileOperation::Rename {
            from: paths.exe.clone(),
            to: paths.old_exe.clone(),
        },
        FileOperation::Rename {
            from: paths.update_exe.clone(),
            to: paths.exe.clone(),
        },
        FileOperation::RemoveIfExists(paths.version_file.clone()),
    ]
}

fn apply_operation(files: &dyn FileSystem, operation: &FileOperation) -> io::Result<()> {
    match operation {
        FileOperation::RemoveIfExists(path) => {
            if files.exists(path) {
                files.remove_file(path)?;
            }
            Ok(())
        }
        FileOperation::Rename { from, to } => files.rename(from, to),
    }
}

/// Runs the plan. On failure undoes the completed renames and returns the error.
pub fn apply_plan(
    files: &dyn FileSystem,
    plan: &[FileOperation],
) -> Result<Vec<FileOperation>, String> {
    let mut completed = Vec::new();
    for operation in plan {
        if let Err(error) = apply_operation(files, operation) {
            roll_back(files, &completed);
            return Err(format!("{operation:?}: {error}"));
        }
        completed.push(operation.clone());
    }
    Ok(completed)
}

pub fn roll_back(files: &dyn FileSystem, completed: &[FileOperation]) {
    for operation in completed.iter().rev() {
        if let FileOperation::Rename { from, to } = operation {
            let _ = files.rename(to, from);
        }
    }
}

pub fn check(paths: &UpdatePaths) -> bool {
    let published = std::fs::read_to_string(&paths.version_file).ok();
    update_is_ready(
        running_version(),
        published.as_deref(),
        paths.update_exe.exists(),
    )
}

pub fn current_paths() -> Option<UpdatePaths> {
    std::env::current_exe()
        .ok()
        .map(|exe| UpdatePaths::beside(&exe))
}

pub fn install_and_relaunch(paths: &UpdatePaths) -> Result<(), String> {
    let files = RealFileSystem;
    let completed = apply_plan(&files, &swap_plan(paths))?;
    match Command::new(&paths.exe)
        .args(std::env::args_os().skip(1))
        .spawn()
    {
        Ok(_) => Ok(()),
        Err(error) => {
            roll_back(&files, &completed);
            Err(format!("Start: {error}"))
        }
    }
}

pub fn clean_up_old_binary() {
    let Some(paths) = current_paths() else {
        return;
    };
    if !paths.old_exe.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..CLEANUP_ATTEMPTS {
            if std::fs::remove_file(&paths.old_exe).is_ok() || !paths.old_exe.exists() {
                return;
            }
            std::thread::sleep(CLEANUP_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn idle() -> IdleInputs {
        IdleInputs {
            window_active: false,
            composer_empty: true,
            send_in_flight: false,
        }
    }

    #[test]
    fn idle_needs_all_three_conditions() {
        assert!(is_idle(idle()));
        assert!(!is_idle(IdleInputs {
            window_active: true,
            ..idle()
        }));
        assert!(!is_idle(IdleInputs {
            composer_empty: false,
            ..idle()
        }));
        assert!(!is_idle(IdleInputs {
            send_in_flight: true,
            ..idle()
        }));
    }

    #[test]
    fn update_needs_a_different_version_and_an_exe() {
        assert!(update_is_ready("a-1", Some("b-2\n"), true));
        assert!(!update_is_ready("a-1", Some("a-1\n"), true));
        assert!(!update_is_ready("a-1", Some("b-2"), false));
        assert!(!update_is_ready("a-1", None, true));
        assert!(!update_is_ready("a-1", Some("  \n"), true));
    }

    #[test]
    fn plan_lists_the_operations_in_order() {
        let paths = UpdatePaths::beside(Path::new("/app/teams.exe"));
        assert_eq!(paths.update_exe, Path::new("/app/update/teams.exe"));
        let plan = swap_plan(&paths);
        assert_eq!(plan.len(), 4);
        assert_eq!(
            plan[1],
            FileOperation::Rename {
                from: paths.exe.clone(),
                to: paths.old_exe.clone()
            }
        );
        assert_eq!(
            plan[2],
            FileOperation::Rename {
                from: paths.update_exe.clone(),
                to: paths.exe.clone()
            }
        );
    }

    fn install(directory: &Path) -> UpdatePaths {
        let paths = UpdatePaths::beside(&directory.join("teams.exe"));
        fs::create_dir_all(paths.update_exe.parent().unwrap()).unwrap();
        fs::write(&paths.exe, "old").unwrap();
        fs::write(&paths.update_exe, "new").unwrap();
        fs::write(&paths.version_file, "b-2").unwrap();
        paths
    }

    #[test]
    fn swap_replaces_the_exe_and_keeps_the_old_one() {
        let directory = tempfile::tempdir().unwrap();
        let paths = install(directory.path());
        apply_plan(&RealFileSystem, &swap_plan(&paths)).unwrap();
        assert_eq!(fs::read_to_string(&paths.exe).unwrap(), "new");
        assert_eq!(fs::read_to_string(&paths.old_exe).unwrap(), "old");
        assert!(!paths.update_exe.exists());
        assert!(!paths.version_file.exists());
    }

    #[test]
    fn swap_replaces_a_leftover_old_exe() {
        let directory = tempfile::tempdir().unwrap();
        let paths = install(directory.path());
        fs::write(&paths.old_exe, "stale").unwrap();
        apply_plan(&RealFileSystem, &swap_plan(&paths)).unwrap();
        assert_eq!(fs::read_to_string(&paths.old_exe).unwrap(), "old");
    }

    struct FailingRename {
        fail_on: PathBuf,
    }

    impl FileSystem for FailingRename {
        fn exists(&self, path: &Path) -> bool {
            path.exists()
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if from == self.fail_on {
                return Err(io::Error::other("locked"));
            }
            std::fs::rename(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_file(path)
        }
    }

    #[test]
    fn failed_step_rolls_back_so_teams_exe_survives() {
        let directory = tempfile::tempdir().unwrap();
        let paths = install(directory.path());
        let files = FailingRename {
            fail_on: paths.update_exe.clone(),
        };
        let error = apply_plan(&files, &swap_plan(&paths)).unwrap_err();
        assert!(error.contains("locked"));
        assert_eq!(fs::read_to_string(&paths.exe).unwrap(), "old");
        assert!(!paths.old_exe.exists());
        assert_eq!(fs::read_to_string(&paths.update_exe).unwrap(), "new");
        assert!(paths.version_file.exists());
    }

    #[test]
    fn running_version_has_no_marker_prefix() {
        assert!(!running_version().starts_with("teams-build:"));
        assert!(!running_version().is_empty());
    }
}
