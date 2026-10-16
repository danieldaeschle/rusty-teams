use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

const TITLE_READ_LIMIT: u64 = 1024 * 1024;
const PROJECT_DIR_NAME_LIMIT: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalSession {
    pub session_id: String,
    pub title: String,
    pub modified: SystemTime,
    pub live: bool,
}

pub fn encode_project_dir(working_dir: &str) -> String {
    working_dir
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

pub fn resolve_project_dir(projects_dir: &Path, working_dir: &str) -> PathBuf {
    let trimmed = working_dir.trim_end_matches(['/', '\\']);
    let encoded = encode_project_dir(if trimmed.is_empty() {
        working_dir
    } else {
        trimmed
    });
    let exact = projects_dir.join(&encoded);
    if exact.exists() || encoded.len() <= PROJECT_DIR_NAME_LIMIT {
        return exact;
    }
    let prefix = &encoded[..PROJECT_DIR_NAME_LIMIT];
    std::fs::read_dir(projects_dir)
        .into_iter()
        .flatten()
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .map_or(exact, |entry| entry.path())
}

fn proc_root(sessions_dir: &Path) -> Option<PathBuf> {
    let text = sessions_dir.to_string_lossy();
    let unc = text
        .strip_prefix(r"\\wsl.localhost\")
        .or_else(|| text.strip_prefix(r"\\wsl$\"));
    match unc {
        Some(rest) => {
            let distro = rest.split('\\').next()?;
            let prefix = &text[..text.len() - rest.len()];
            Some(PathBuf::from(format!("{prefix}{distro}\\proc")))
        }
        None if cfg!(unix) => Some(PathBuf::from("/proc")),
        None => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningSession {
    pub session_id: String,
    pub name: Option<String>,
    pub working_dir: String,
    pub socket_path: Option<String>,
    pub process_id: u32,
}

pub fn running_sessions(sessions_dir: &Path) -> Vec<RunningSession> {
    running_sessions_in(sessions_dir, proc_root(sessions_dir).as_deref())
}

pub fn live_session_names(sessions_dir: &Path) -> HashMap<String, Option<String>> {
    running_sessions(sessions_dir)
        .into_iter()
        .map(|session| (session.session_id, session.name))
        .collect()
}

fn running_sessions_in(sessions_dir: &Path, proc_root: Option<&Path>) -> Vec<RunningSession> {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return Vec::new();
    };
    let checkable_root = proc_root.filter(|root| root.is_dir());
    entries
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            path.extension()
                .is_some_and(|extension| extension == "json")
                && checkable_root
                    .is_none_or(|root| path.file_stem().is_some_and(|pid| root.join(pid).exists()))
        })
        .filter_map(|entry| {
            let value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()?;
            let text = |key: &str| {
                value[key]
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            let process_id = entry
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse().ok())
                .or_else(|| {
                    value["pid"]
                        .as_u64()
                        .and_then(|pid| u32::try_from(pid).ok())
                })?;
            Some(RunningSession {
                session_id: value["sessionId"].as_str()?.to_owned(),
                name: text("name"),
                working_dir: text("cwd").unwrap_or_default(),
                socket_path: text("messagingSocketPath"),
                process_id,
            })
        })
        .collect()
}

fn is_session_file_stem(stem: &str) -> bool {
    stem.len() == 36
        && stem.char_indices().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

fn is_lone_tag(line: &str) -> bool {
    let Some(inner) = line
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    else {
        return false;
    };
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    let (name, attributes) = inner.split_once(' ').unwrap_or((inner, ""));
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_' || character == '-')
        && !attributes.contains('>')
}

fn open_tag_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('<')?;
    let name_end = rest
        .find(|character: char| {
            !(character.is_ascii_lowercase() || character == '_' || character == '-')
        })
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(name_end);
    (!name.is_empty() && after.starts_with(['>', ' '])).then_some(name)
}

fn plain_line(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if line.is_empty() {
            index += 1;
            continue;
        }
        let closing = open_tag_name(line).and_then(|name| {
            let close = format!("</{name}>");
            lines[index..]
                .iter()
                .enumerate()
                .position(|(offset, later)| {
                    if offset == 0 {
                        later.ends_with(&close)
                    } else {
                        later.contains(&close)
                    }
                })
        });
        if let Some(offset) = closing {
            index += offset + 1;
        } else if is_lone_tag(line) {
            index += 1;
        } else {
            return Some(line.to_owned());
        }
    }
    None
}

fn text_parts(content: &serde_json::Value) -> Vec<&str> {
    match content {
        serde_json::Value::String(text) => vec![text],
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect(),
        _ => Vec::new(),
    }
}

fn first_user_message(path: &Path) -> Option<String> {
    let mut reader = BufReader::new(File::open(path).ok()?.take(TITLE_READ_LIMIT));
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).ok()? == 0 {
            return None;
        }
        let Ok(entry) = serde_json::from_slice::<serde_json::Value>(&line) else {
            continue;
        };
        if entry["type"] != "user" || entry["isMeta"] == true {
            continue;
        }
        if let Some(title) = text_parts(&entry["message"]["content"])
            .into_iter()
            .find_map(plain_line)
        {
            return Some(title);
        }
    }
}

pub fn scan_external_sessions(
    project_dir: &Path,
    live_sessions: &HashMap<String, Option<String>>,
    known_session_ids: &HashSet<String>,
    limit: usize,
) -> Vec<ExternalSession> {
    let Ok(entries) = std::fs::read_dir(project_dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, SystemTime)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()? != "jsonl" {
                return None;
            }
            let session_id = path.file_stem()?.to_str()?.to_owned();
            if !is_session_file_stem(&session_id) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            (!known_session_ids.contains(&session_id)).then_some((session_id, modified))
        })
        .collect();
    files.sort_by(|left, right| right.1.cmp(&left.1));
    files
        .into_iter()
        .filter_map(|(session_id, modified)| {
            let live_name = live_sessions.get(&session_id);
            let title = live_name
                .cloned()
                .flatten()
                .or_else(|| first_user_message(&project_dir.join(format!("{session_id}.jsonl"))))?;
            Some(ExternalSession {
                live: live_name.is_some(),
                session_id,
                title,
                modified,
            })
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const PLAIN: &str =
        r#"{"type":"user","message":{"role":"user","content":"Fix the build\nplease"}}"#;

    fn id(number: u32) -> String {
        format!("00000000-0000-0000-0000-{number:012}")
    }

    fn user_line(content: &str) -> String {
        serde_json::json!({"type": "user", "message": {"role": "user", "content": content}})
            .to_string()
    }

    fn write_session(directory: &Path, session_id: &str, body: &str, age: Duration) {
        let path = directory.join(format!("{session_id}.jsonl"));
        std::fs::write(&path, body).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - age)
            .unwrap();
    }

    fn scan(directory: &Path, limit: usize) -> Vec<ExternalSession> {
        scan_external_sessions(directory, &HashMap::new(), &HashSet::new(), limit)
    }

    #[test]
    fn encoding_replaces_every_non_alphanumeric_character() {
        assert_eq!(
            encode_project_dir("/home/daniel/Projects/ms-teams-linux"),
            "-home-daniel-Projects-ms-teams-linux"
        );
        assert_eq!(
            encode_project_dir("/home/daniel/repo/.claude/worktrees/x_y"),
            "-home-daniel-repo--claude-worktrees-x-y"
        );
        assert_eq!(encode_project_dir(r"C:\Users\dd"), "C--Users-dd");
    }

    #[test]
    fn trailing_separators_are_dropped_before_encoding() {
        let projects = tempfile::tempdir().unwrap();
        for working_dir in ["/home/x/repo/", "/home/x/repo", r"C:\x\repo\"] {
            let resolved = resolve_project_dir(projects.path(), working_dir);
            assert!(!resolved.to_string_lossy().ends_with("--"));
        }
        assert_eq!(
            resolve_project_dir(projects.path(), "/home/x/repo/"),
            projects.path().join("-home-x-repo")
        );
        assert_eq!(
            resolve_project_dir(projects.path(), "/"),
            projects.path().join("-")
        );
    }

    #[test]
    fn a_long_working_dir_falls_back_to_the_prefix_folder() {
        let projects = tempfile::tempdir().unwrap();
        let long_dir = format!("/home/daniel/{}", "deep/".repeat(45));
        let encoded = encode_project_dir(long_dir.trim_end_matches('/'));
        assert!(encoded.len() > PROJECT_DIR_NAME_LIMIT);
        let truncated = projects
            .path()
            .join(format!("{}-abc123", &encoded[..PROJECT_DIR_NAME_LIMIT]));
        std::fs::create_dir(&truncated).unwrap();
        assert_eq!(resolve_project_dir(projects.path(), &long_dir), truncated);
        std::fs::create_dir(projects.path().join(&encoded)).unwrap();
        assert_eq!(
            resolve_project_dir(projects.path(), &long_dir),
            projects.path().join(&encoded)
        );
        assert_eq!(
            resolve_project_dir(projects.path(), "/short"),
            projects.path().join("-short")
        );
    }

    #[test]
    fn scan_takes_titles_from_the_first_plain_user_line_and_skips_known_ids() {
        let directory = tempfile::tempdir().unwrap();
        let meta = r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#;
        let command = user_line("<command-name>/clear</command-name>");
        let assistant = r#"{"type":"assistant","message":{"role":"assistant","content":"hi"}}"#;
        let tagged = user_line("<pasted_content id=\"1\">\n<system-reminder>\nReal question here");
        let parts = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"x"},{"type":"text","text":"Parts title"}]}}"#;
        write_session(
            directory.path(),
            &id(1),
            &format!("{meta}\n{command}\n{assistant}\n{PLAIN}\n"),
            Duration::from_secs(60),
        );
        write_session(
            directory.path(),
            &id(2),
            &format!("{tagged}\n"),
            Duration::from_secs(120),
        );
        write_session(
            directory.path(),
            &id(3),
            &format!("not json\n{parts}\n"),
            Duration::from_secs(3600),
        );
        write_session(directory.path(), &id(4), "", Duration::from_secs(7200));
        write_session(
            directory.path(),
            &id(5),
            assistant,
            Duration::from_secs(7300),
        );
        write_session(directory.path(), &id(6), PLAIN, Duration::from_secs(10));
        write_session(
            directory.path(),
            "agent-notes",
            PLAIN,
            Duration::from_secs(5),
        );
        std::fs::write(directory.path().join("notes.txt"), "x").unwrap();
        let known = HashSet::from([id(6)]);
        let sessions = scan_external_sessions(directory.path(), &HashMap::new(), &known, 10);
        let summary: Vec<(String, &str)> = sessions
            .iter()
            .map(|session| (session.session_id.clone(), session.title.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (id(1), "Fix the build"),
                (id(2), "Real question here"),
                (id(3), "Parts title"),
            ]
        );
        assert!(sessions.iter().all(|session| !session.live));
        let age = SystemTime::now()
            .duration_since(sessions[0].modified)
            .unwrap();
        assert!(age >= Duration::from_secs(60) && age < Duration::from_secs(120));
    }

    #[test]
    fn enclosed_blocks_are_skipped_across_lines_and_parts() {
        assert_eq!(
            plain_line("<system-reminder>\nIDE note\nmore\n</system-reminder>\nFix it").as_deref(),
            Some("Fix it")
        );
        assert_eq!(
            plain_line("<ide_opened_file>The user opened a.rs</ide_opened_file>"),
            None
        );
        assert_eq!(
            plain_line("<b>bold</b> text").as_deref(),
            Some("<b>bold</b> text")
        );
        let directory = tempfile::tempdir().unwrap();
        let body = serde_json::json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "text", "text": "<ide_opened_file>The user opened a.rs</ide_opened_file>"},
            {"type": "text", "text": "<ide_selection>\nlet x = 1;\n</ide_selection>"},
            {"type": "text", "text": "Refactor this"}
        ]}})
        .to_string();
        write_session(directory.path(), &id(1), &body, Duration::ZERO);
        assert_eq!(scan(directory.path(), 1)[0].title, "Refactor this");
    }

    #[test]
    fn lone_tags_are_recognised() {
        assert!(is_lone_tag("<system-reminder>"));
        assert!(is_lone_tag("</system-reminder>"));
        assert!(is_lone_tag(r#"<pasted_content id="1" size="9">"#));
        assert!(!is_lone_tag("<b>bold</b> text"));
        assert!(!is_lone_tag("a < b > c"));
        assert!(!is_lone_tag("<>"));
    }

    #[test]
    fn a_title_past_the_first_user_line_is_still_found_and_the_read_is_capped() {
        let directory = tempfile::tempdir().unwrap();
        let filler = r#"{"type":"assistant","message":{"role":"assistant","content":"x"}}"#;
        let body = format!("{}\n{PLAIN}\n", vec![filler; 2000].join("\n"));
        assert!(body.len() as u64 > 64 * 1024);
        write_session(directory.path(), &id(1), &body, Duration::ZERO);
        assert_eq!(scan(directory.path(), 1)[0].title, "Fix the build");
        let late = format!("{}\n{PLAIN}\n", " ".repeat(TITLE_READ_LIMIT as usize));
        write_session(directory.path(), &id(2), &late, Duration::ZERO);
        let found: Vec<String> = scan(directory.path(), 10)
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert_eq!(found, [id(1)]);
    }

    #[test]
    fn scan_keeps_the_newest_ten() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..12u32 {
            write_session(
                directory.path(),
                &id(index),
                PLAIN,
                Duration::from_secs(u64::from(index) * 100),
            );
        }
        let sessions = scan(directory.path(), 10);
        assert_eq!(sessions.len(), 10);
        assert_eq!(sessions[0].session_id, id(0));
        assert_eq!(sessions[9].session_id, id(9));
    }

    fn names(sessions: Vec<RunningSession>) -> HashMap<String, Option<String>> {
        sessions
            .into_iter()
            .map(|session| (session.session_id, session.name))
            .collect()
    }

    #[test]
    fn running_sessions_carry_their_socket_directory_and_process_id() {
        let sessions_dir = tempfile::tempdir().unwrap();
        let proc_root = tempfile::tempdir().unwrap();
        std::fs::create_dir(proc_root.path().join("205357")).unwrap();
        std::fs::write(
            sessions_dir.path().join("205357.json"),
            r#"{"pid":205357,"sessionId":"abc","name":" Review ","cwd":"/srv/app","status":"busy","messagingSocketPath":"/run/user/1000/cc-socks/205357.sock"}"#,
        )
        .unwrap();
        std::fs::write(
            sessions_dir.path().join("999.json"),
            r#"{"pid":999,"sessionId":"dead","cwd":"/srv/gone"}"#,
        )
        .unwrap();
        assert_eq!(
            running_sessions_in(sessions_dir.path(), Some(proc_root.path())),
            [RunningSession {
                session_id: "abc".to_owned(),
                name: Some("Review".to_owned()),
                working_dir: "/srv/app".to_owned(),
                socket_path: Some("/run/user/1000/cc-socks/205357.sock".to_owned()),
                process_id: 205357,
            }]
        );
    }

    #[test]
    fn live_sessions_need_a_running_pid_and_use_their_name_as_title() {
        let sessions_dir = tempfile::tempdir().unwrap();
        let proc_root = tempfile::tempdir().unwrap();
        std::fs::create_dir(proc_root.path().join("111")).unwrap();
        std::fs::create_dir(proc_root.path().join("222")).unwrap();
        let file = |pid: u32, session_id: &str, name: &str| {
            std::fs::write(
                sessions_dir.path().join(format!("{pid}.json")),
                format!(r#"{{"pid":{pid},"sessionId":"{session_id}","name":"{name}"}}"#),
            )
            .unwrap();
        };
        file(111, &id(1), "Refactor auth");
        file(222, &id(2), "");
        file(333, &id(3), "Dead one");
        std::fs::write(sessions_dir.path().join("444.json"), "garbage").unwrap();
        let live = names(running_sessions_in(
            sessions_dir.path(),
            Some(proc_root.path()),
        ));
        assert_eq!(live.len(), 2);
        assert!(!live.contains_key(&id(3)));
        assert_eq!(
            running_sessions_in(sessions_dir.path(), Some(Path::new("/nonexistent"))).len(),
            3
        );
        let directory = tempfile::tempdir().unwrap();
        write_session(directory.path(), &id(1), PLAIN, Duration::from_secs(5000));
        write_session(directory.path(), &id(2), PLAIN, Duration::from_secs(6000));
        write_session(directory.path(), &id(7), PLAIN, Duration::from_secs(7000));
        let sessions = scan_external_sessions(directory.path(), &live, &HashSet::new(), 10);
        let summary: Vec<(String, &str, bool)> = sessions
            .iter()
            .map(|session| {
                (
                    session.session_id.clone(),
                    session.title.as_str(),
                    session.live,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (id(1), "Refactor auth", true),
                (id(2), "Fix the build", true),
                (id(7), "Fix the build", false),
            ]
        );
        assert!(live_session_names(Path::new("/nonexistent")).is_empty());
    }

    #[test]
    fn the_proc_root_follows_the_environment_of_the_sessions_dir() {
        assert_eq!(
            proc_root(Path::new(r"\\wsl.localhost\Ubuntu\home\x\.claude\sessions")),
            Some(PathBuf::from(r"\\wsl.localhost\Ubuntu\proc"))
        );
        assert_eq!(
            proc_root(Path::new(r"\\wsl$\Debian\home\x")),
            Some(PathBuf::from(r"\\wsl$\Debian\proc"))
        );
        assert_eq!(
            proc_root(Path::new("/home/x/.claude/sessions")),
            cfg!(unix).then(|| PathBuf::from("/proc"))
        );
    }

    #[test]
    fn a_missing_directory_yields_no_sessions() {
        assert!(scan(Path::new("/nonexistent/projects"), 10).is_empty());
    }
}
