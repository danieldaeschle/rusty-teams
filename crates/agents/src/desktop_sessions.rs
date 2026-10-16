use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopSession {
    pub session_id: String,
    pub working_dir: String,
    pub title: String,
    pub last_activity_millis: i64,
}

#[derive(Deserialize)]
struct DesktopConfig {
    #[serde(rename = "lastKnownAccountUuid")]
    account_uuid: Option<String>,
}

#[derive(Deserialize)]
struct DesktopSessionFile {
    #[serde(rename = "cliSessionId")]
    cli_session_id: Option<String>,
    cwd: Option<String>,
    title: Option<String>,
    #[serde(rename = "isArchived", default)]
    archived: bool,
    #[serde(rename = "lastActivityAt", default)]
    last_activity_millis: i64,
}

fn is_plain_account_uuid(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

pub fn desktop_sessions(desktop_dir: &Path) -> Vec<DesktopSession> {
    let Some(account_uuid) = std::fs::read_to_string(desktop_dir.join("config.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<DesktopConfig>(&text).ok())
        .and_then(|config| config.account_uuid)
        .filter(|uuid| is_plain_account_uuid(uuid))
    else {
        return Vec::new();
    };
    let Ok(organizations) =
        std::fs::read_dir(desktop_dir.join("claude-code-sessions").join(account_uuid))
    else {
        return Vec::new();
    };
    organizations
        .flatten()
        .filter_map(|organization| std::fs::read_dir(organization.path()).ok())
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("local_") && name.ends_with(".json")
        })
        .filter_map(|entry| {
            let file: DesktopSessionFile =
                serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()?;
            if file.archived {
                return None;
            }
            Some(DesktopSession {
                session_id: file.cli_session_id.filter(|id| !id.is_empty())?,
                working_dir: file.cwd.unwrap_or_default(),
                title: file.title.unwrap_or_default(),
                last_activity_millis: file.last_activity_millis,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn only_active_sessions_of_the_signed_in_account_with_a_cli_id_are_listed() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"lastKnownAccountUuid":"acc-1","oauth:tokenCache":"secret"}"#,
        );
        let sessions = root.path().join("claude-code-sessions");
        write(
            &sessions.join("acc-1/org-1/local_a.json"),
            r#"{"cliSessionId":"cli-a","cwd":"/srv/app","title":"Fix build","isArchived":false,"lastActivityAt":1700000000000,"wslConfig":{"distro":"Ubuntu"}}"#,
        );
        write(
            &sessions.join("acc-1/org-2/local_b.json"),
            r#"{"cliSessionId":"cli-b","cwd":"/srv/b","title":"Archived","isArchived":true,"lastActivityAt":1}"#,
        );
        write(
            &sessions.join("acc-1/org-1/local_c.json"),
            r#"{"cwd":"/srv/c","title":"No cli id","isArchived":false}"#,
        );
        write(
            &sessions.join("acc-2/org-1/local_d.json"),
            r#"{"cliSessionId":"cli-d","cwd":"/srv/d","title":"Other account","isArchived":false}"#,
        );
        write(&sessions.join("acc-1/org-1/notes.json"), "{}");
        assert_eq!(
            desktop_sessions(root.path()),
            [DesktopSession {
                session_id: "cli-a".to_owned(),
                working_dir: "/srv/app".to_owned(),
                title: "Fix build".to_owned(),
                last_activity_millis: 1_700_000_000_000,
            }]
        );
    }

    #[test]
    fn a_missing_config_or_an_unsafe_account_id_lists_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert!(desktop_sessions(root.path()).is_empty());
        write(
            &root.path().join("config.json"),
            r#"{"lastKnownAccountUuid":"../x"}"#,
        );
        assert!(desktop_sessions(root.path()).is_empty());
    }
}
