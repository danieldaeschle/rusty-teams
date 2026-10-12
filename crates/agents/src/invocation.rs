use std::path::{Path, PathBuf};

use crate::action::AgentAction;
use crate::profile::{AgentProfile, Environment};
use crate::template::{UnknownPlaceholder, Values, placeholders_in, render, split_command};

const WSL_PROGRAM: &str = "wsl.exe";
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
            let mut stop_args = args.clone();
            args.extend(
                ["-e", "sh", "-lc", WSL_SCRIPT, "sh"]
                    .into_iter()
                    .map(String::from),
            );
            args.push(working_dir);
            args.extend(tokens);
            stop_args.extend(["-e", "pkill", "-f", session_id].map(String::from));
            Ok(RunPlan {
                run: Invocation {
                    program: WSL_PROGRAM.to_string(),
                    args,
                    cwd: None,
                    stdin,
                },
                stop: Some(Invocation {
                    program: WSL_PROGRAM.to_string(),
                    args: stop_args,
                    cwd: None,
                    stdin: None,
                }),
                cleanup,
            })
        }
    }
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
            ["-d", "Ubuntu", "-e", "pkill", "-f", "sid-1"].map(String::from)
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
            ["wsl.exe", "--cd", "~", "-e", "claude", "--resume", "sid-4"].map(String::from)
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
}
