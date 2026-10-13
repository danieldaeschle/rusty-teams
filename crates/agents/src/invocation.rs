use std::path::{Path, PathBuf};

use crate::action::AgentAction;
use crate::profile::{AgentProfile, Environment};
use crate::template::{UnknownPlaceholder, Values, placeholders_in, render, split_command};

const WSL_PROGRAM: &str = "wsl.exe";
const APP_STOP_SCRIPT: &str = r#"for p in $(pgrep -f -- "$1"); do c=$(tr '\0' ' ' </proc/$p/cmdline 2>/dev/null) || continue; case "$c" in *app-stop-marker*) continue;; esac; case "$c" in *" -p "*|*" --print "*) ;; *) continue;; esac; case "$c" in *--output-format*) ;; *) continue;; esac; case "$c" in *"--session-id $1 "*) kill -9 "$p";; *"--resume $1 "*) case "$c" in *--fork-session*) ;; *) kill -9 "$p";; esac;; esac; done"#;
const SESSION_FLAG: &str = "--session-id {session_id}";
const RESUME_FLAG: &str = "--resume {session_id}";
const WSL_SCRIPT: &str = r#"case "$1" in "~"*) dir="$HOME${1#\~}";; *) dir="$1";; esac; cd "$dir" && shift && exec "$@""#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub stdin: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpEndpoint {
    pub url: String,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunPlan {
    pub run: Invocation,
    pub stop: Option<Invocation>,
    pub cleanup: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvocationError {
    #[error("the command is empty")]
    EmptyCommand,
    #[error(transparent)]
    UnknownPlaceholder(#[from] UnknownPlaceholder),
    #[error("cannot write the MCP config: {0}")]
    McpConfig(String),
    #[error("the agent command has no \"{SESSION_FLAG}\" to resume with")]
    CannotResume,
    #[error("the session id is not valid")]
    InvalidSessionId,
}

fn expand_home(path: &str) -> String {
    let remainder = match path.strip_prefix('~') {
        Some(remainder) if remainder.is_empty() || remainder.starts_with(['/', '\\']) => remainder,
        _ => return path.to_string(),
    };
    match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(home) => format!("{home}{remainder}"),
        Err(_) => path.to_string(),
    }
}

fn mcp_config_json(endpoint: &McpEndpoint) -> String {
    serde_json::json!({
        "mcpServers": {
            "teams": {
                "type": "http",
                "url": endpoint.url,
                "headers": {"Authorization": format!("Bearer {}", endpoint.token)},
            }
        }
    })
    .to_string()
}

fn wsl_unc_rest(path: &str) -> Option<&str> {
    path.strip_prefix(r"\\wsl.localhost\")
        .or_else(|| path.strip_prefix(r"\\wsl$\"))
}

pub fn wsl_unc_distro(path: &str) -> Option<&str> {
    wsl_unc_rest(path).map(|rest| rest.split('\\').next().unwrap_or(rest))
}

pub fn linux_path(path: &str) -> String {
    match wsl_unc_rest(path) {
        Some(rest) => match rest.split_once('\\') {
            Some((_, relative)) => format!("/{}", relative.replace('\\', "/")),
            None => "/".to_owned(),
        },
        None => wsl_path(path),
    }
}

pub fn wsl_path(path: &str) -> String {
    let mut characters = path.chars();
    match (characters.next(), characters.next(), characters.next()) {
        (Some(drive), Some(':'), Some('/' | '\\')) if drive.is_ascii_alphabetic() => format!(
            "/mnt/{}/{}",
            drive.to_ascii_lowercase(),
            path[3..].replace('\\', "/")
        ),
        _ => path.to_string(),
    }
}

fn write_mcp_config(
    directory: &Path,
    session_id: &str,
    endpoint: &McpEndpoint,
) -> Result<PathBuf, InvocationError> {
    let to_error = |error: std::io::Error| InvocationError::McpConfig(error.to_string());
    std::fs::create_dir_all(directory).map_err(to_error)?;
    let path = directory.join(format!("mcp-{session_id}.json"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(to_error)?;
    std::io::Write::write_all(&mut file, mcp_config_json(endpoint).as_bytes()).map_err(to_error)?;
    Ok(path)
}

fn mcp_config_argument(flag: &str, path: &Path, environment: &Environment) -> String {
    let text = path.to_string_lossy();
    let text = match environment {
        Environment::Wsl { .. } if cfg!(windows) => wsl_path(&text),
        _ => text.into_owned(),
    };
    if flag.ends_with("additional-mcp-config") {
        format!("@{text}")
    } else {
        text
    }
}

fn wsl_prefix(environment: &Environment) -> Vec<String> {
    match environment {
        Environment::Wsl {
            distro: Some(distro),
        } => vec!["-d".to_string(), distro.clone()],
        _ => Vec::new(),
    }
}

fn working_directory(profile: &AgentProfile, action: &AgentAction) -> String {
    match profile.environment {
        Environment::Host => expand_home(&action.working_dir),
        Environment::Wsl { .. } => action.working_dir.clone(),
    }
}

pub fn build_invocation(
    profile: &AgentProfile,
    action: &AgentAction,
    prompt: &str,
    session_id: &str,
    mcp: Option<&McpEndpoint>,
    mcp_config_directory: Option<&Path>,
) -> Result<RunPlan, InvocationError> {
    let working_dir = working_directory(profile, action);
    let values = Values {
        session_id: session_id.to_string(),
        agent: profile.id.clone(),
        prompt: prompt.to_string(),
        cwd: working_dir.clone(),
        ..Values::default()
    };
    let raw_tokens: Vec<String> = split_command(&profile.command)
        .into_iter()
        .chain(split_command(&action.extra_args))
        .collect();
    if raw_tokens.is_empty() {
        return Err(InvocationError::EmptyCommand);
    }
    let uses_prompt_placeholder = raw_tokens
        .iter()
        .any(|token| placeholders_in(token).iter().any(|name| name == "prompt"));
    let mut tokens = raw_tokens
        .iter()
        .map(|token| render(token, &values))
        .collect::<Result<Vec<_>, _>>()?;
    let mut cleanup = Vec::new();
    if let Some(endpoint) = mcp
        && let Some(directory) = mcp_config_directory
        && !profile.mcp_config_flag.is_empty()
    {
        let path = write_mcp_config(directory, session_id, endpoint)?;
        tokens.push(profile.mcp_config_flag.clone());
        tokens.push(mcp_config_argument(
            &profile.mcp_config_flag,
            &path,
            &profile.environment,
        ));
        cleanup.push(path);
    }
    let stdin = (!uses_prompt_placeholder).then(|| prompt.to_string());

    match &profile.environment {
        Environment::Host => {
            let mut tokens = tokens.into_iter();
            let program = tokens.next().ok_or(InvocationError::EmptyCommand)?;
            Ok(RunPlan {
                run: Invocation {
                    program,
                    args: tokens.collect(),
                    cwd: Some(PathBuf::from(working_dir)),
                    stdin,
                },
                stop: None,
                cleanup,
            })
        }
        Environment::Wsl { .. } => {
            let mut args = wsl_prefix(&profile.environment);
            args.extend(
                ["-e", "sh", "-lc", WSL_SCRIPT, "sh"]
                    .into_iter()
                    .map(String::from),
            );
            args.push(working_dir);
            args.extend(tokens);
            Ok(RunPlan {
                run: Invocation {
                    program: WSL_PROGRAM.to_string(),
                    args,
                    cwd: None,
                    stdin,
                },
                stop: Some(app_stop_invocation(&profile.environment, session_id)),
                cleanup,
            })
        }
    }
}

fn app_stop_invocation(environment: &Environment, session_id: &str) -> Invocation {
    let mut args = wsl_prefix(environment);
    args.extend(["-e", "sh", "-c", APP_STOP_SCRIPT, "sh", session_id].map(String::from));
    Invocation {
        program: WSL_PROGRAM.to_string(),
        args,
        cwd: None,
        stdin: None,
    }
}

pub fn wsl_stop_invocation(environment: &Environment, session_id: &str) -> Option<Invocation> {
    (matches!(environment, Environment::Wsl { .. }) && is_plain_session_id(session_id))
        .then(|| app_stop_invocation(environment, session_id))
}

fn is_plain_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

pub fn kill_process_invocation(process_id: u32, session_id: &str) -> Option<Invocation> {
    if !is_plain_session_id(session_id) {
        return None;
    }
    if cfg!(windows) {
        let script = format!(
            "$p = Get-CimInstance Win32_Process -Filter 'ProcessId={process_id}'; if ($p -and $p.CommandLine -like '*{session_id}*') {{ taskkill /PID {process_id} /T /F }}"
        );
        return Some(Invocation {
            program: "powershell".to_string(),
            args: ["-NoProfile", "-Command", &script]
                .map(String::from)
                .to_vec(),
            cwd: None,
            stdin: None,
        });
    }
    Some(Invocation {
        program: "sh".to_string(),
        args: [
            "-c",
            r#"grep -qaF -- "$2" "/proc/$1/cmdline" && kill -9 "$1""#,
            "sh",
            &process_id.to_string(),
            session_id,
        ]
        .map(String::from)
        .to_vec(),
        cwd: None,
        stdin: None,
    })
}

pub fn supports_resume(profile: &AgentProfile) -> bool {
    profile.command.contains(SESSION_FLAG)
}

pub fn resume_invocation(
    profile: &AgentProfile,
    action: &AgentAction,
    prompt: &str,
    session_id: &str,
    mcp: Option<&McpEndpoint>,
    mcp_config_directory: Option<&Path>,
) -> Result<RunPlan, InvocationError> {
    resume_with_flag(
        profile,
        action,
        prompt,
        session_id,
        RESUME_FLAG,
        mcp,
        mcp_config_directory,
    )
}

pub fn fork_invocation(
    profile: &AgentProfile,
    action: &AgentAction,
    prompt: &str,
    original_session_id: &str,
    fork_session_id: &str,
    mcp: Option<&McpEndpoint>,
    mcp_config_directory: Option<&Path>,
) -> Result<RunPlan, InvocationError> {
    if !is_plain_session_id(original_session_id) {
        return Err(InvocationError::InvalidSessionId);
    }
    resume_with_flag(
        profile,
        action,
        prompt,
        fork_session_id,
        &format!("--resume {original_session_id} --fork-session {SESSION_FLAG}"),
        mcp,
        mcp_config_directory,
    )
}

fn resume_with_flag(
    profile: &AgentProfile,
    action: &AgentAction,
    prompt: &str,
    session_id: &str,
    flag: &str,
    mcp: Option<&McpEndpoint>,
    mcp_config_directory: Option<&Path>,
) -> Result<RunPlan, InvocationError> {
    if !supports_resume(profile) {
        return Err(InvocationError::CannotResume);
    }
    if !is_plain_session_id(session_id) {
        return Err(InvocationError::InvalidSessionId);
    }
    let resuming_profile = AgentProfile {
        command: profile.command.replace(SESSION_FLAG, flag),
        ..profile.clone()
    };
    build_invocation(
        &resuming_profile,
        action,
        prompt,
        session_id,
        mcp,
        mcp_config_directory,
    )
}

pub fn open_session_invocation(
    profile: &AgentProfile,
    action: &AgentAction,
    session_id: &str,
) -> Result<Invocation, InvocationError> {
    let values = Values {
        session_id: session_id.to_string(),
        agent: profile.id.clone(),
        cwd: working_directory(profile, action),
        ..Values::default()
    };
    let mut tokens = split_command(&profile.open_session_command)
        .iter()
        .map(|token| render(token, &values))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let program = tokens.next().ok_or(InvocationError::EmptyCommand)?;
    Ok(Invocation {
        program,
        args: tokens.collect(),
        cwd: None,
        stdin: None,
    })
}

pub fn version_check_invocation(profile: &AgentProfile) -> Invocation {
    let program = split_command(&profile.command)
        .into_iter()
        .next()
        .unwrap_or_default();
    match &profile.environment {
        Environment::Host => Invocation {
            program,
            args: vec!["--version".to_string()],
            cwd: None,
            stdin: None,
        },
        Environment::Wsl { .. } => {
            let mut args = wsl_prefix(&profile.environment);
            args.extend(
                ["-e", "sh", "-lc", WSL_SCRIPT, "sh", "~"]
                    .into_iter()
                    .map(String::from),
            );
            args.push(program);
            args.push("--version".to_string());
            Invocation {
                program: WSL_PROGRAM.to_string(),
                args,
                cwd: None,
                stdin: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_invocation_has_exact_args() {
        let mut profile = AgentProfile::claude_wsl();
        profile.environment = Environment::Wsl {
            distro: Some("Ubuntu".to_string()),
        };
        let action = AgentAction::answer_this("claude");
        let plan = build_invocation(&profile, &action, "hi there", "sid-1", None, None).unwrap();
        let expected_args: Vec<String> = [
            "-d",
            "Ubuntu",
            "-e",
            "sh",
            "-lc",
            WSL_SCRIPT,
            "sh",
            "~",
            "claude",
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--session-id",
            "sid-1",
            "--allowedTools",
            "WebSearch,WebFetch,Read,Grep,Glob,mcp__teams",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(plan.run.program, "wsl.exe");
        assert_eq!(plan.run.args, expected_args);
        assert_eq!(plan.run.cwd, None);
        assert_eq!(plan.run.stdin.as_deref(), Some("hi there"));
        let stop = plan.stop.unwrap();
        assert_eq!(stop.program, "wsl.exe");
        assert_eq!(
            stop.args,
            [
                "-d",
                "Ubuntu",
                "-e",
                "sh",
                "-c",
                APP_STOP_SCRIPT,
                "sh",
                "sid-1"
            ]
            .map(String::from)
        );
    }

    #[test]
    fn prompt_placeholder_goes_in_args_without_stdin() {
        let profile = AgentProfile::copilot_wsl();
        let action = AgentAction::answer_this("copilot");
        let plan = build_invocation(&profile, &action, "a b\nc", "sid-2", None, None).unwrap();
        assert_eq!(plan.run.stdin, None);
        assert_eq!(plan.run.args[..2], ["-e".to_string(), "sh".to_string()]);
        let prompt_position = plan.run.args.iter().position(|arg| arg == "-p").unwrap() + 1;
        assert_eq!(plan.run.args[prompt_position], "a b\nc");
    }

    #[test]
    fn host_invocation_expands_home() {
        let mut action = AgentAction::build_this("claude-host");
        action.working_dir = "~/work".to_string();
        let plan = build_invocation(
            &AgentProfile::claude_host(),
            &action,
            "p",
            "sid-3",
            None,
            None,
        )
        .unwrap();
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap();
        assert_eq!(plan.run.program, "claude");
        assert_eq!(plan.run.cwd, Some(PathBuf::from(format!("{home}/work"))));
        assert_eq!(plan.run.stdin.as_deref(), Some("p"));
        assert!(
            plan.run
                .args
                .ends_with(&["--permission-mode".to_string(), "auto".to_string()])
        );
        assert_eq!(plan.stop, None);
    }

    fn endpoint() -> McpEndpoint {
        McpEndpoint {
            url: "http://127.0.0.1:9/mcp".to_string(),
            token: "tok".to_string(),
        }
    }

    #[test]
    fn mcp_endpoint_writes_a_config_file_and_passes_its_path() {
        let directory = tempfile::tempdir().unwrap();
        let action = AgentAction::answer_this("claude");
        let plan = build_invocation(
            &AgentProfile::claude_host(),
            &action,
            "p",
            "sid",
            Some(&endpoint()),
            Some(directory.path()),
        )
        .unwrap();
        let flag_position = plan
            .run
            .args
            .iter()
            .position(|arg| arg == "--mcp-config")
            .unwrap();
        let expected_path = directory.path().join("mcp-sid.json");
        assert_eq!(
            plan.run.args[flag_position + 1],
            expected_path.to_string_lossy()
        );
        assert_eq!(plan.cleanup, vec![expected_path.clone()]);
        assert!(!plan.run.args.iter().any(|arg| arg.contains("tok")));
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&expected_path).unwrap()).unwrap();
        let server = &json["mcpServers"]["teams"];
        assert_eq!(server["type"], "http");
        assert_eq!(server["url"], "http://127.0.0.1:9/mcp");
        assert_eq!(server["headers"]["Authorization"], "Bearer tok");
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(&expected_path).unwrap().permissions()
            ) & 0o777,
            0o600
        );
    }

    #[test]
    fn mcp_flag_is_omitted_without_endpoint_or_directory() {
        let directory = tempfile::tempdir().unwrap();
        let action = AgentAction::answer_this("claude");
        let profile = AgentProfile::claude_host();
        let without_endpoint =
            build_invocation(&profile, &action, "p", "sid", None, Some(directory.path())).unwrap();
        assert!(
            !without_endpoint
                .run
                .args
                .iter()
                .any(|arg| arg == "--mcp-config")
        );
        let without_directory =
            build_invocation(&profile, &action, "p", "sid", Some(&endpoint()), None).unwrap();
        assert!(
            !without_directory
                .run
                .args
                .iter()
                .any(|arg| arg == "--mcp-config")
        );
        assert!(without_directory.cleanup.is_empty());
    }

    #[test]
    fn copilot_takes_the_config_path_with_an_at_prefix() {
        let directory = tempfile::tempdir().unwrap();
        let action = AgentAction::answer_this("copilot");
        let plan = build_invocation(
            &AgentProfile::copilot_wsl(),
            &action,
            "p",
            "sid",
            Some(&endpoint()),
            Some(directory.path()),
        )
        .unwrap();
        let flag_position = plan
            .run
            .args
            .iter()
            .position(|arg| arg == "--additional-mcp-config")
            .unwrap();
        assert_eq!(
            plan.run.args[flag_position + 1],
            format!(
                "@{}",
                directory.path().join("mcp-sid.json").to_string_lossy()
            )
        );
    }

    #[test]
    fn windows_paths_translate_to_the_wsl_mount() {
        assert_eq!(
            wsl_path(r"C:\Users\x\AppData\mcp-1.json"),
            "/mnt/c/Users/x/AppData/mcp-1.json"
        );
        assert_eq!(wsl_path("D:/data/mcp.json"), "/mnt/d/data/mcp.json");
        assert_eq!(wsl_path("/tmp/mcp.json"), "/tmp/mcp.json");
        assert_eq!(wsl_path("relative/mcp.json"), "relative/mcp.json");
    }

    #[test]
    fn agent_placeholder_renders_profile_id() {
        let mut profile = AgentProfile::claude_host();
        profile.command = "tool --agent {agent}".to_string();
        let mut action = AgentAction::build_this("x");
        action.extra_args = String::new();
        let plan = build_invocation(&profile, &action, "p", "s", None, None).unwrap();
        assert_eq!(plan.run.args, ["--agent", "claude-host"].map(String::from));
    }

    #[test]
    fn empty_command_is_an_error() {
        let mut profile = AgentProfile::claude_host();
        profile.command = String::new();
        let mut action = AgentAction::build_this("x");
        action.extra_args = String::new();
        assert_eq!(
            build_invocation(&profile, &action, "p", "s", None, None),
            Err(InvocationError::EmptyCommand)
        );
    }

    #[test]
    fn open_session_renders_cwd_and_session() {
        let action = AgentAction::build_this("claude");
        let invocation =
            open_session_invocation(&AgentProfile::claude_wsl(), &action, "sid-4").unwrap();
        assert_eq!(invocation.program, "wt.exe");
        assert_eq!(
            invocation.args,
            [
                "wsl.exe",
                "--cd",
                "~",
                "-e",
                "sh",
                "-lc",
                "exec claude --resume sid-4"
            ]
            .map(String::from)
        );
    }

    #[test]
    fn version_check_runs_the_first_token_in_the_same_environment() {
        let host = version_check_invocation(&AgentProfile::claude_host());
        assert_eq!(host.program, "claude");
        assert_eq!(host.args, ["--version"].map(String::from));
        let mut wsl_profile = AgentProfile::claude_wsl();
        wsl_profile.environment = Environment::Wsl {
            distro: Some("Ubuntu".to_string()),
        };
        let wsl = version_check_invocation(&wsl_profile);
        assert_eq!(wsl.program, "wsl.exe");
        assert_eq!(
            wsl.args,
            [
                "-d",
                "Ubuntu",
                "-e",
                "sh",
                "-lc",
                WSL_SCRIPT,
                "sh",
                "~",
                "claude",
                "--version"
            ]
            .map(String::from)
        );
    }

    #[test]
    fn resume_swaps_the_session_flag_and_keeps_the_extra_args() {
        let mut profile = AgentProfile::claude_wsl();
        profile.environment = Environment::Wsl {
            distro: Some("Ubuntu".to_string()),
        };
        let action = AgentAction::answer_this("claude");
        let plan = resume_invocation(&profile, &action, "go on", "sid-9", None, None).unwrap();
        let expected_args: Vec<String> = [
            "-d",
            "Ubuntu",
            "-e",
            "sh",
            "-lc",
            WSL_SCRIPT,
            "sh",
            "~",
            "claude",
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--resume",
            "sid-9",
            "--allowedTools",
            "WebSearch,WebFetch,Read,Grep,Glob,mcp__teams",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(plan.run.args, expected_args);
        assert_eq!(plan.run.stdin.as_deref(), Some("go on"));
        assert_eq!(
            plan.stop.unwrap().args,
            [
                "-d",
                "Ubuntu",
                "-e",
                "sh",
                "-c",
                APP_STOP_SCRIPT,
                "sh",
                "sid-9"
            ]
            .map(String::from)
        );
    }

    #[test]
    fn resume_on_the_host_keeps_the_permission_mode() {
        let action = AgentAction::build_this("claude-host");
        let plan = resume_invocation(
            &AgentProfile::claude_host(),
            &action,
            "go on",
            "sid-8",
            None,
            None,
        )
        .unwrap();
        assert_eq!(plan.run.program, "claude");
        assert!(!plan.run.args.contains(&"--session-id".to_string()));
        assert!(
            plan.run
                .args
                .windows(2)
                .any(|pair| pair == ["--resume", "sid-8"])
        );
        assert!(
            plan.run
                .args
                .ends_with(&["--permission-mode".to_string(), "auto".to_string()])
        );
        assert_eq!(plan.stop, None);
    }

    #[test]
    fn fork_resumes_the_original_into_a_fresh_session_id_and_keeps_the_stop() {
        let action = AgentAction::answer_this("claude");
        let plan = fork_invocation(
            &AgentProfile::claude_wsl(),
            &action,
            "go on",
            "old-6",
            "new-6",
            None,
            None,
        )
        .unwrap();
        assert!(plan.run.args.windows(5).any(|window| window
            == [
                "--resume",
                "old-6",
                "--fork-session",
                "--session-id",
                "new-6"
            ]));
        assert_eq!(plan.run.stdin.as_deref(), Some("go on"));
        assert_eq!(
            plan.stop.unwrap().args,
            ["-e", "sh", "-c", APP_STOP_SCRIPT, "sh", "new-6"].map(String::from)
        );
        assert!(
            fork_invocation(
                &AgentProfile::claude_wsl(),
                &action,
                "x",
                "bad id; rm",
                "new",
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn resume_rejects_a_session_id_that_is_not_plain() {
        let action = AgentAction::answer_this("claude");
        assert!(matches!(
            resume_invocation(
                &AgentProfile::claude_wsl(),
                &action,
                "x",
                "a b; rm",
                None,
                None
            ),
            Err(InvocationError::InvalidSessionId)
        ));
    }

    #[test]
    fn linux_path_maps_drives_and_wsl_unc_paths() {
        assert_eq!(linux_path(r"C:\Users\dd\repo"), "/mnt/c/Users/dd/repo");
        assert_eq!(
            linux_path(r"\\wsl.localhost\Ubuntu\home\daniel\repo"),
            "/home/daniel/repo"
        );
        assert_eq!(linux_path(r"\\wsl$\Ubuntu\home"), "/home");
        assert_eq!(linux_path(r"\\wsl.localhost\Ubuntu"), "/");
        assert_eq!(
            wsl_unc_distro(r"\\wsl.localhost\Ubuntu\home"),
            Some("Ubuntu")
        );
        assert_eq!(wsl_unc_distro(r"\\wsl$\Debian"), Some("Debian"));
        assert_eq!(wsl_unc_distro(r"C:\Users"), None);
        assert_eq!(linux_path("/home/daniel"), "/home/daniel");
    }

    #[test]
    fn copilot_resumes_with_the_prompt_in_its_args() {
        let action = AgentAction::answer_this("copilot");
        let plan = resume_invocation(
            &AgentProfile::copilot_wsl(),
            &action,
            "go on",
            "sid-7",
            None,
            None,
        )
        .unwrap();
        assert_eq!(plan.run.stdin, None);
        assert!(
            plan.run
                .args
                .windows(2)
                .any(|pair| pair == ["--resume", "sid-7"])
        );
        assert!(plan.run.args.contains(&"go on".to_string()));
    }

    #[test]
    fn kill_refuses_a_session_id_that_is_not_plain() {
        assert_eq!(kill_process_invocation(1, ""), None);
        assert_eq!(kill_process_invocation(1, "a'; rm -rf /; '"), None);
        assert_eq!(kill_process_invocation(1, "a b"), None);
    }

    #[cfg(windows)]
    #[test]
    fn kill_checks_the_command_line_before_taskkill() {
        let invocation = kill_process_invocation(4242, "abc-123").unwrap();
        assert_eq!(invocation.program, "powershell");
        assert_eq!(
            invocation.args[..2],
            ["-NoProfile", "-Command"].map(String::from)
        );
        let script = &invocation.args[2];
        assert!(script.contains("ProcessId=4242"));
        assert!(script.contains("-like '*abc-123*'"));
        assert!(script.contains("taskkill /PID 4242 /T /F"));
    }

    #[cfg(unix)]
    mod unix_kill {
        use super::super::*;

        fn spawn_agent_like(arguments: &[&str]) -> std::process::Child {
            std::process::Command::new("sh")
                .args(["-c", "sleep 30", "claude"])
                .args(arguments)
                .spawn()
                .unwrap()
        }

        fn run_app_stop(session_id: &str) {
            std::process::Command::new("sh")
                .args(["-c", APP_STOP_SCRIPT, "sh", session_id])
                .status()
                .unwrap();
        }

        fn is_running(child: &mut std::process::Child) -> bool {
            child.try_wait().unwrap().is_none()
        }

        #[test]
        fn the_app_stop_matches_by_the_role_of_the_session_id() {
            let session = uuid::Uuid::new_v4().to_string();
            let fork_id = uuid::Uuid::new_v4().to_string();
            let mut headless = spawn_agent_like(&[
                "--print",
                "--resume",
                &session,
                "--output-format",
                "stream-json",
            ]);
            let mut fork = spawn_agent_like(&[
                "-p",
                "--output-format",
                "stream-json",
                "--resume",
                &session,
                "--fork-session",
                "--session-id",
                &fork_id,
            ]);
            let mut terminal = spawn_agent_like(&["--resume", &session]);
            std::thread::sleep(std::time::Duration::from_millis(200));
            run_app_stop(&session);
            assert!(headless.wait().is_ok_and(|status| !status.success()));
            assert!(is_running(&mut fork));
            assert!(is_running(&mut terminal));
            run_app_stop(&fork_id);
            assert!(fork.wait().is_ok_and(|status| !status.success()));
            assert!(is_running(&mut terminal));
            terminal.kill().unwrap();
            terminal.wait().unwrap();
        }

        #[test]
        fn a_running_stop_script_is_never_killed_by_another() {
            let session = uuid::Uuid::new_v4().to_string();
            let mut lookalike = std::process::Command::new("sh")
                .args([
                    "-c",
                    &format!("sleep 30 # {APP_STOP_SCRIPT}"),
                    "sh",
                    "-p",
                    "--output-format",
                    "--resume",
                    &session,
                ])
                .spawn()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(200));
            run_app_stop(&session);
            assert!(is_running(&mut lookalike));
            lookalike.kill().unwrap();
            lookalike.wait().unwrap();
        }

        fn spawn_sleeper(label: &str) -> std::process::Child {
            std::process::Command::new("sh")
                .args(["-c", "sleep 30", label])
                .spawn()
                .unwrap()
        }

        fn run_kill(process_id: u32, session_id: &str) {
            let invocation = kill_process_invocation(process_id, session_id).unwrap();
            std::process::Command::new(invocation.program)
                .args(invocation.args)
                .status()
                .unwrap();
        }

        #[test]
        fn kills_a_process_whose_command_line_has_the_session_id() {
            let mut child = spawn_sleeper("fake-session-1");
            run_kill(child.id(), "fake-session-1");
            let status = child.wait().unwrap();
            assert!(!status.success());
        }

        #[test]
        fn leaves_a_process_without_the_session_id_alone() {
            let mut child = spawn_sleeper("other-session");
            run_kill(child.id(), "fake-session-2");
            assert!(child.try_wait().unwrap().is_none());
            child.kill().unwrap();
            child.wait().unwrap();
        }
    }

    #[test]
    fn a_command_without_the_session_flag_cannot_resume() {
        let mut profile = AgentProfile::claude_host();
        profile.command = "tool -p".to_string();
        let action = AgentAction::build_this("claude-host");
        assert!(!supports_resume(&profile));
        assert_eq!(
            resume_invocation(&profile, &action, "go on", "sid", None, None),
            Err(InvocationError::CannotResume)
        );
    }
}
