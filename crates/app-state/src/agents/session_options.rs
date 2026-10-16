use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agents::{DesktopSession, Environment, RunningSession};
use chrono::{DateTime, Local, Utc};
use mcp::install::{Homes, Location};

use super::message_context;
use super::runs::{AgentRun, InboxTarget, RunState};
use crate::agent_api::agent_homes;
use crate::app_state::AppState;
use crate::notice::truncated;

pub const SESSION_LIMIT: usize = 10;
const LABEL_CHARS: usize = 60;
const WEEK_DAYS: i64 = 7;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionOption {
    pub session_id: String,
    pub working_dir: String,
    pub label: String,
    pub meta: String,
    pub running: bool,
    pub inbox: Option<InboxTarget>,
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
                inbox: None,
            }
        })
        .collect()
}

pub fn merged_options(
    desktop: &[DesktopSession],
    running: &[RunningSession],
    sessions_dir: &Path,
    known_session_ids: &HashSet<String>,
    now: DateTime<Utc>,
) -> Vec<SessionOption> {
    let mut seen = known_session_ids.clone();
    let mut entries: Vec<(SessionOption, i64)> = Vec::new();
    for session in running {
        if !seen.insert(session.session_id.clone()) {
            continue;
        }
        let desktop_session = desktop
            .iter()
            .find(|candidate| candidate.session_id == session.session_id);
        let title = desktop_session
            .map(|candidate| candidate.title.clone())
            .filter(|title| !title.is_empty())
            .or_else(|| session.name.clone())
            .unwrap_or_else(|| session.session_id.clone());
        let working_dir = desktop_session
            .map(|candidate| candidate.working_dir.clone())
            .filter(|working_dir| !working_dir.is_empty())
            .unwrap_or_else(|| session.working_dir.clone());
        entries.push((
            SessionOption {
                session_id: session.session_id.clone(),
                working_dir,
                label: truncated(&title, LABEL_CHARS),
                meta: "Running".to_owned(),
                running: true,
                inbox: Some(InboxTarget {
                    session: session.clone(),
                    sessions_dir: sessions_dir.to_path_buf(),
                }),
            },
            desktop_session.map_or(0, |candidate| candidate.last_activity_millis),
        ));
    }
    for session in desktop {
        if !seen.insert(session.session_id.clone()) {
            continue;
        }
        let activity = DateTime::<Utc>::from_timestamp_millis(session.last_activity_millis);
        entries.push((
            SessionOption {
                session_id: session.session_id.clone(),
                working_dir: session.working_dir.clone(),
                label: truncated(&session.title, LABEL_CHARS),
                meta: activity.map_or_else(
                    || "Idle".to_owned(),
                    |activity| format!("Idle - {}", relative_age(now, activity)),
                ),
                running: false,
                inbox: None,
            },
            session.last_activity_millis,
        ));
    }
    entries.sort_by(|left, right| {
        right
            .0
            .running
            .cmp(&left.0.running)
            .then(right.1.cmp(&left.1))
    });
    entries
        .into_iter()
        .take(SESSION_LIMIT)
        .map(|(option, _)| option)
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

fn registry_dir(environment: &Environment, homes: &Homes) -> Option<PathBuf> {
    Some(
        pick_home(environment, homes)?
            .join(".claude")
            .join("sessions"),
    )
}

fn desktop_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("Claude"))
}

#[derive(Default)]
pub struct SessionLoad {
    pub desktop: Vec<DesktopSession>,
    pub running: Vec<RunningSession>,
    pub sessions_dir: PathBuf,
}

pub fn load_sessions(environment: &Environment, demo: bool) -> SessionLoad {
    let Some(sessions_dir) = registry_dir(environment, &agent_homes(demo)) else {
        return SessionLoad::default();
    };
    SessionLoad {
        desktop: if demo {
            Vec::new()
        } else {
            desktop_dir()
                .map(|directory| agents::desktop_sessions(&directory))
                .unwrap_or_default()
        },
        running: agents::running_sessions(&sessions_dir),
        sessions_dir,
    }
}

pub fn attach_inboxes(options: &mut [SessionOption], load: &SessionLoad) {
    for option in options.iter_mut().filter(|option| !option.running) {
        if let Some(session) = load
            .running
            .iter()
            .find(|session| session.session_id == option.session_id)
        {
            option.running = true;
            option.meta = "Running".to_owned();
            option.inbox = Some(InboxTarget {
                session: session.clone(),
                sessions_dir: load.sessions_dir.clone(),
            });
        }
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
    use chrono::TimeZone;

    use super::*;

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
    fn runs_that_never_created_a_session_are_left_out() {
        let state = AppState::new(
            std::sync::Arc::new(store::Store::open_in_memory().unwrap()),
            crate::app_state::Mode::default(),
        );
        let mut created = crate::agents::fixtures::finished_run(1);
        created.launched = true;
        let mut never_started = crate::agents::fixtures::finished_run(2);
        never_started.launched = false;
        let mut sent = crate::agents::fixtures::finished_run(3);
        sent.launched = false;
        sent.sent_to = Some(crate::agents::runs::SessionTarget {
            session_id: sent.session_id.clone(),
            working_dir: "~".to_owned(),
            inbox: None,
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

    fn desktop(session_id: &str, last_activity_millis: i64) -> DesktopSession {
        DesktopSession {
            session_id: session_id.to_owned(),
            working_dir: format!("/srv/{session_id}"),
            title: format!("Title {session_id}"),
            last_activity_millis,
        }
    }

    fn running(session_id: &str) -> RunningSession {
        RunningSession {
            session_id: session_id.to_owned(),
            name: Some(format!("Name {session_id}")),
            working_dir: "/registry".to_owned(),
            socket_path: Some("/run/s.sock".to_owned()),
            process_id: 7,
        }
    }

    fn ids(options: &[SessionOption]) -> Vec<&str> {
        options
            .iter()
            .map(|option| option.session_id.as_str())
            .collect()
    }

    #[test]
    fn running_sessions_come_first_then_the_newest_activity() {
        let options = merged_options(
            &[
                desktop("old", 1_000),
                desktop("live", 500),
                desktop("new", 9_000),
            ],
            &[running("live"), running("registry-only")],
            Path::new("/sessions"),
            &HashSet::new(),
            Utc::now(),
        );
        assert_eq!(ids(&options), ["live", "registry-only", "new", "old"]);
        assert!(options[0].running && options[0].inbox.is_some());
        assert_eq!(options[0].label, "Title live");
        assert_eq!(options[0].working_dir, "/srv/live");
        assert_eq!(options[1].label, "Name registry-only");
        assert_eq!(options[1].working_dir, "/registry");
        assert!(!options[2].running && options[2].inbox.is_none());
    }

    #[test]
    fn known_run_ids_are_excluded_and_the_limit_applies() {
        let sessions: Vec<DesktopSession> = (0..15)
            .map(|index| desktop(&format!("s{index}"), index))
            .collect();
        let known = HashSet::from(["s14".to_owned(), "live".to_owned()]);
        let options = merged_options(
            &sessions,
            &[running("live")],
            Path::new("/sessions"),
            &known,
            Utc::now(),
        );
        assert_eq!(options.len(), SESSION_LIMIT);
        assert_eq!(options[0].session_id, "s13");
        assert!(!ids(&options).contains(&"live"));
    }

    #[test]
    fn a_session_known_to_the_registry_gets_an_inbox_on_a_run_option() {
        let option = |session_id: &str, running: bool| SessionOption {
            session_id: session_id.to_owned(),
            working_dir: "/repo".to_owned(),
            label: String::new(),
            meta: String::new(),
            running,
            inbox: None,
        };
        let mut options = vec![option("a", false), option("b", false), option("c", true)];
        let load = SessionLoad {
            desktop: Vec::new(),
            running: vec![running("a"), running("c")],
            sessions_dir: PathBuf::from("/sessions"),
        };
        attach_inboxes(&mut options, &load);
        let inboxes: Vec<bool> = options
            .iter()
            .map(|option| option.inbox.is_some())
            .collect();
        assert_eq!(inboxes, [true, false, false]);
        assert!(options[0].running);
    }
}
