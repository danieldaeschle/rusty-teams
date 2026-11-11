use std::path::{Path, PathBuf};

use crate::profile::{AgentProfile, Environment};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliHit {
    pub profile: AgentProfile,
    pub binary: PathBuf,
}

struct Template {
    names: &'static [&'static str],
    build: fn() -> AgentProfile,
    cursor: bool,
}

pub fn scan_clis() -> Vec<CliHit> {
    let templates = [
        Template {
            names: &["claude"],
            build: AgentProfile::claude_host,
            cursor: false,
        },
        Template {
            names: &["copilot"],
            build: AgentProfile::copilot_host,
            cursor: false,
        },
        Template {
            names: &["codex"],
            build: AgentProfile::codex_host,
            cursor: false,
        },
        Template {
            names: &["cursor-agent", "agent"],
            build: AgentProfile::cursor_host,
            cursor: true,
        },
        Template {
            names: &["agy"],
            build: AgentProfile::agy_host,
            cursor: false,
        },
    ];
    templates
        .into_iter()
        .filter_map(|template| {
            let binary = find_binary(template.names, template.cursor)?;
            let program = binary
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(template.names[0]);
            let mut profile = with_program((template.build)(), program);
            if !cfg!(windows) {
                profile.open_session_command.clear();
            }
            profile.environment = Environment::Host;
            Some(CliHit { profile, binary })
        })
        .collect()
}

/// A bare program name is looked up on `PATH` and in `~/.local/bin`.
/// A desktop session often omits the user bin directory, so `claude` and
/// `cursor-agent` exist for the shell but not for the app.
pub fn resolve_program(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() || program.contains('/') || program.contains('\\') {
        return path.is_file().then(|| path.to_path_buf());
    }
    for directory in candidate_dirs() {
        if let Some(found) = executable(&directory, program) {
            return Some(found);
        }
    }
    None
}

pub fn already_installed(existing: &[AgentProfile], hit: &AgentProfile) -> bool {
    let program = hit.command_program();
    existing.iter().any(|agent| {
        agent.is_command()
            && agent.environment == hit.environment
            && agent.command_program() == program
    })
}

fn with_program(mut profile: AgentProfile, program: &str) -> AgentProfile {
    let rest = profile.command.split_once(' ').map(|(_, rest)| rest);
    profile.command = match rest {
        Some(rest) => format!("{program} {rest}"),
        None => program.to_owned(),
    };
    profile
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local").join("bin"));
    }
    dirs
}

fn find_binary(names: &[&str], cursor: bool) -> Option<PathBuf> {
    for directory in candidate_dirs() {
        for name in names {
            if let Some(path) = executable(&directory, name) {
                if cursor && *name == "agent" && !looks_like_cursor(&path) {
                    continue;
                }
                return Some(path);
            }
        }
    }
    None
}

fn executable(directory: &Path, name: &str) -> Option<PathBuf> {
    ["", ".exe", ".cmd"].into_iter().find_map(|extension| {
        let path = directory.join(format!("{name}{extension}"));
        path.is_file().then_some(path)
    })
}

fn looks_like_cursor(path: &Path) -> bool {
    let linked = std::fs::read_link(path).unwrap_or_else(|_| path.to_path_buf());
    let text = format!("{} {}", path.display(), linked.display());
    text.contains("cursor-agent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wsl_claude_does_not_hide_the_host_binary() {
        let wsl = AgentProfile::claude_wsl();
        let host = AgentProfile::claude_host();
        assert!(!already_installed(&[wsl], &host));
        assert!(already_installed(&[host.clone()], &host));
    }

    #[test]
    fn an_absolute_program_is_used_and_a_missing_name_is_not() {
        let directory = std::env::temp_dir().join(format!("agents-resolve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let binary = directory.join("claude");
        std::fs::write(&binary, b"").unwrap();
        assert_eq!(
            resolve_program(binary.to_str().unwrap()).as_deref(),
            Some(binary.as_path())
        );
        assert!(resolve_program("definitely-not-a-real-program").is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_program_token_is_replaced_and_the_rest_stays() {
        let profile = with_program(AgentProfile::cursor_host(), "cursor-agent");
        assert_eq!(
            profile.command,
            "cursor-agent -p --trust --approve-mcps {prompt}"
        );
    }
}
