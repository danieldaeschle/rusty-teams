use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agents::{Environment, ExternalSession};
use chrono::{DateTime, Local, Utc};
use mcp::install::{Homes, Location};

use super::message_context;
use super::runs::{AgentRun, RunState};
use crate::agent_api::agent_homes;
use crate::app_state::AppState;
use crate::notice::truncated;

pub const SESSION_LIMIT: usize = 10;
const ACTIVE_WINDOW: Duration = Duration::from_secs(120);
const LABEL_CHARS: usize = 60;
const WEEK_DAYS: i64 = 7;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionOption {
    pub session_id: String,
    pub working_dir: String,
    pub label: String,
    pub meta: String,
    pub running: bool,
    pub fork: bool,
}

pub fn relative_age(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
    let age = now - then;
    if age.num_minutes() < 1 {
        "now".to_owned()
    } else if age.num_hours() < 1 {
        format!("{}m", age.num_minutes())
    } else if age.num_days() < 1 {
        format!("{}h", age.num_hours())
    } else if age.num_days() < WEEK_DAYS {
        then.with_timezone(&Local).format("%a").to_string()
    } else {
        then.with_timezone(&Local).format("%b %-d").to_string()
    }
}

fn run_meta(run: &AgentRun, now: DateTime<Utc>) -> String {
    let age = relative_age(now, run.ended_at.unwrap_or(run.started_at));
    match run.state {
        RunState::Running => "Running".to_owned(),
        RunState::Done => format!("Done - {age}"),
        RunState::Failed { .. } => format!("Failed - {age}"),
        RunState::Stopped => format!("Stopped - {age}"),
    }
}

pub fn run_options(runs: &[&AgentRun], state: &AppState, now: DateTime<Utc>) -> Vec<SessionOption> {
    let mut seen = HashSet::new();
    runs.iter()
        .filter(|run| {
            !run.session_id.is_empty()
                && (run.launched || run.sent_to.is_some())
                && seen.insert(run.session_id.clone())
        })
        .take(SESSION_LIMIT)
        .map(|run| {
            let message =
                message_context::message_line(state, &run.conversation_id, &run.message_id)
                    .unwrap_or_else(|| run.chat_title.clone());
            SessionOption {
                session_id: run.session_id.clone(),
                working_dir: run.working_dir.clone(),
                label: format!(
                    "{} - {}",
                    run.action_label,
                    truncated(&message, LABEL_CHARS)
                ),
                meta: run_meta(run, now),
                running: run.is_running(),
                fork: false,
            }
        })
        .collect()
}

pub fn external_options(
    sessions: &[ExternalSession],
    working_dir: &str,
    now: DateTime<Utc>,
) -> Vec<SessionOption> {
    sessions
        .iter()
        .map(|session| {
            let modified = DateTime::<Utc>::from(session.modified);
            let active = session.live
                || modified > now
                || (now - modified)
                    .to_std()
                    .is_ok_and(|age| age < ACTIVE_WINDOW);
            SessionOption {
                session_id: session.session_id.clone(),
                working_dir: working_dir.to_owned(),
                label: truncated(&session.title, LABEL_CHARS),
                meta: if active {
                    "Active elsewhere - sends to a copy".to_owned()
                } else {
                    format!("Idle - {}", relative_age(now, modified))
                },
                running: false,
                fork: active,
            }
        })
        .collect()
}

fn pick_home<'a>(environment: &Environment, homes: &'a Homes) -> Option<&'a PathBuf> {
    homes
        .entries
        .iter()
        .find(|(location, _)| match (environment, location) {
            (Environment::Host, Location::Windows | Location::Native) => true,
            (
                Environment::Wsl {
                    distro: Some(wanted),
                },
                Location::Wsl { distro },
            ) => wanted == distro,
            (Environment::Wsl { distro: None }, Location::Wsl { .. }) => true,
            (Environment::Wsl { .. }, Location::Native) => true,
            _ => false,
        })
        .map(|(_, home)| home)
}

fn resolve_working_dir(environment: &Environment, working_dir: &str, home: &Path) -> String {
    let home_text = home.to_string_lossy();
    let home_text = match environment {
        Environment::Host => home_text.into_owned(),
        Environment::Wsl { .. } => agents::linux_path(&home_text),
    };
    match working_dir.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            format!("{home_text}{rest}")
        }
        _ => working_dir.to_owned(),
    }
}

fn claude_dirs(
    environment: &Environment,
    working_dir: &str,
    homes: &Homes,
) -> Option<(PathBuf, PathBuf)> {
    let home = pick_home(environment, homes)?;
    let resolved = resolve_working_dir(environment, working_dir, home);
    let claude = home.join(".claude");
    Some((
        agents::resolve_project_dir(&claude.join("projects"), &resolved),
        claude.join("sessions"),
    ))
}

#[derive(Default)]
pub struct ExternalLoad {
    pub sessions: Vec<ExternalSession>,
    pub live_session_ids: HashSet<String>,
}

pub fn load_external(
    environment: &Environment,
    working_dir: &str,
    known_session_ids: &HashSet<String>,
    demo: bool,
) -> ExternalLoad {
    let Some((project_dir, sessions_dir)) =
        claude_dirs(environment, working_dir, &agent_homes(demo))
    else {
        return ExternalLoad::default();
    };
    let live = agents::live_session_names(&sessions_dir);
    ExternalLoad {
        sessions: agents::scan_external_sessions(
            &project_dir,
            &live,
            known_session_ids,
            SESSION_LIMIT,
        ),
        live_session_ids: live.into_keys().collect(),
    }
}

pub fn mark_live_runs(options: &mut [SessionOption], live_session_ids: &HashSet<String>) {
    for option in options {
        option.fork = !option.running && live_session_ids.contains(&option.session_id);
    }
}

pub fn default_wsl_distro(demo: bool) -> Option<String> {
    agent_homes(demo)
        .entries
        .into_iter()
        .find_map(|(location, _)| match location {
            Location::Wsl { distro } => Some(distro),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use chrono::TimeZone;

    use super::*;

    fn external(age: Duration) -> ExternalSession {
        ExternalSession {
            session_id: "abc".to_owned(),
            title: "Fix the build".to_owned(),
            modified: SystemTime::now() - age,
            live: false,
        }
    }

    #[test]
    fn relative_age_uses_minutes_hours_weekday_and_date() {
        let now = Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap();
        let ago = |seconds: i64| relative_age(now, now - chrono::Duration::seconds(seconds));
        assert_eq!(ago(20), "now");
        assert_eq!(ago(5 * 60), "5m");
        assert_eq!(ago(3 * 3600), "3h");
        assert_eq!(ago(2 * 86400).len(), 3);
        assert!(ago(10 * 86400).contains(char::is_whitespace));
    }

    #[test]
    fn external_sessions_are_active_under_two_minutes_and_fork() {
        let now = Utc::now();
        let options = external_options(
            &[
                external(Duration::from_secs(30)),
                external(Duration::from_secs(3 * 3600 + 30)),
            ],
            "/repo",
            now,
        );
        assert_eq!(options[0].meta, "Active elsewhere - sends to a copy");
        assert!(options[0].fork);
        assert_eq!(options[1].meta, "Idle - 3h");
        assert!(!options[1].fork);
        assert_eq!(options[1].label, "Fix the build");
        assert_eq!(options[1].working_dir, "/repo");
        let live = ExternalSession {
            live: true,
            ..external(Duration::from_secs(3 * 3600))
        };
        let options = external_options(&[live], "/repo", now);
        assert_eq!(options[0].meta, "Active elsewhere - sends to a copy");
        assert!(options[0].fork);
    }

    #[test]
    fn app_sessions_open_in_a_terminal_are_forked_unless_running_here() {
        let option = |session_id: &str, running: bool| SessionOption {
            session_id: session_id.to_owned(),
            working_dir: "/repo".to_owned(),
            label: String::new(),
            meta: String::new(),
            running,
            fork: false,
        };
        let mut options = vec![option("a", false), option("b", false), option("c", true)];
        mark_live_runs(
            &mut options,
            &HashSet::from(["a".to_owned(), "c".to_owned()]),
        );
        let forks: Vec<bool> = options.iter().map(|option| option.fork).collect();
        assert_eq!(forks, [true, false, false]);
    }

    #[test]
    fn runs_that_never_created_a_session_are_left_out() {
        let state = AppState::new(
            std::sync::Arc::new(store::Store::open_in_memory().unwrap()),
            crate::app_state::Mode::default(),
        );
        let mut created = crate::agents::runs::tests::finished_run(1);
        created.launched = true;
        let mut never_started = crate::agents::runs::tests::finished_run(2);
        never_started.launched = false;
        let mut sent = crate::agents::runs::tests::finished_run(3);
        sent.launched = false;
        sent.sent_to = Some(crate::agents::runs::SessionTarget {
            session_id: sent.session_id.clone(),
            working_dir: "~".to_owned(),
            fork: false,
        });
        let options = run_options(&[&created, &never_started, &sent], &state, Utc::now());
        let ids: Vec<&str> = options
            .iter()
            .map(|option| option.session_id.as_str())
            .collect();
        assert_eq!(ids, ["session-1", "session-3"]);
    }

    #[test]
    fn the_home_matches_the_agent_environment() {
        let homes = Homes {
            entries: vec![
                (Location::Windows, PathBuf::from(r"C:\Users\dd")),
                (
                    Location::Wsl {
                        distro: "Ubuntu".to_owned(),
                    },
                    PathBuf::from(r"\\wsl.localhost\Ubuntu\home\daniel"),
                ),
            ],
        };
        let wsl = |distro: Option<&str>| Environment::Wsl {
            distro: distro.map(str::to_owned),
        };
        assert_eq!(
            pick_home(&Environment::Host, &homes),
            Some(&PathBuf::from(r"C:\Users\dd"))
        );
        assert!(pick_home(&wsl(None), &homes).is_some());
        assert!(pick_home(&wsl(Some("Ubuntu")), &homes).is_some());
        assert_eq!(pick_home(&wsl(Some("Debian")), &homes), None);
    }

    #[test]
    fn a_wsl_home_encodes_the_linux_path_of_the_working_dir() {
        let homes = Homes {
            entries: vec![(
                Location::Wsl {
                    distro: "Ubuntu".to_owned(),
                },
                PathBuf::from(r"\\wsl.localhost\Ubuntu\home\daniel"),
            )],
        };
        let environment = Environment::Wsl { distro: None };
        let (directory, sessions) = claude_dirs(&environment, "~/Projects/x", &homes).unwrap();
        assert!(sessions.ends_with(".claude/sessions"));
        assert_eq!(
            directory.file_name().unwrap().to_string_lossy(),
            "-home-daniel-Projects-x"
        );
        assert_eq!(
            resolve_working_dir(&environment, "/srv/app", Path::new("/home/daniel")),
            "/srv/app"
        );
    }

    #[test]
    fn a_host_home_expands_the_tilde_before_encoding() {
        let homes = Homes {
            entries: vec![(Location::Native, PathBuf::from("/home/daniel"))],
        };
        let (directory, _) = claude_dirs(&Environment::Host, "~", &homes).unwrap();
        assert!(directory.ends_with(".claude/projects/-home-daniel"));
    }
}
