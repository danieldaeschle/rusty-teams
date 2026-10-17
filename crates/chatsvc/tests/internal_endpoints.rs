use std::fs;
use std::path::{Path, PathBuf};

const NON_GRAPH_HOSTS: &[&str] = &[
    "cloud.microsoft",
    "microsoft.com",
    "office.com",
    "office.net",
    "skype.com",
    "live.com",
    "sharepoint.com",
    "microsoftonline.com",
    "microsoft365.com",
    "giphy.com",
];

const NOT_A_SERVICE: &[&str] = &["graph.microsoft.com", "schema.skype.com"];

const ALLOWED_PATHS: &[(&str, &str)] = &[
    ("chatsvc/", "Teams service clients, one module per service"),
    ("calling/src/signaling.rs", "call signaling and flightproxy"),
    ("calling/src/relay.rs", "TURN relay tokens"),
    ("calling/src/meeting_options.rs", "meeting options MT"),
    ("calling/src/whiteboard.rs", "meeting whiteboard MT"),
    ("calling/src/breakout.rs", "breakout room join link parsing"),
    ("calling/assets/", "in-page caption bot command"),
    (
        "session/",
        "sign-in, token refresh, page hosts, token audiences",
    ),
    ("browser/", "headless browser page targets"),
    ("webview/", "WebView2 page hosts"),
    ("app-base/src/task_dialog/sdk.rs", "task dialog host origin"),
    ("core/src/links.rs", "link classification"),
    ("core/src/meeting_join.rs", "meeting join link parsing"),
    ("core/src/message_link.rs", "message link parsing"),
    ("core/src/call_event.rs", "recording link host check"),
    ("core/src/external_image.rs", "GIF host check"),
    ("app-call-model/src/agenda.rs", "demo join link"),
    ("app-call-model/src/demo.rs", "demo data"),
    ("app-state/src/demo.rs", "demo data"),
    ("app-base/src/demo_", "demo data"),
    ("app/src/layout_guard.rs", "layout test fixture"),
    ("graph/src/tabs.rs", "channel tab web links"),
];

#[test]
fn non_graph_hosts_stay_in_their_service_modules() {
    let crates_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    source_files(&crates_directory, &mut files);
    let mut offenders = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&crates_directory)
            .expect("file below crates")
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWED_PATHS
            .iter()
            .any(|(prefix, _)| relative.starts_with(prefix))
        {
            continue;
        }
        let source = fs::read_to_string(&file).expect("read source file");
        for (index, line) in production_lines(&source) {
            if names_non_graph_host(line) {
                offenders.push(format!("{relative}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "non-Graph hosts outside their service modules (move them or extend ALLOWED_PATHS and docs/research/internal-endpoints.md):\n{}",
        offenders.join("\n")
    );
}

fn source_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read source directory") {
        let path = entry.expect("directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if !matches!(name, "tests" | "examples" | "target" | "benches") {
                source_files(&path, files);
            }
        } else if (name.ends_with(".rs") || name.ends_with(".js")) && name != "tests.rs" {
            files.push(path);
        }
    }
}

fn production_lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    source
        .lines()
        .enumerate()
        .take_while(|(_, line)| line.trim() != "#[cfg(test)]")
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
}

fn names_non_graph_host(line: &str) -> bool {
    let mut cleaned = line.to_owned();
    for host in NOT_A_SERVICE {
        cleaned = cleaned.replace(host, "");
    }
    NON_GRAPH_HOSTS.iter().any(|host| cleaned.contains(host))
}

#[test]
fn host_check_ignores_graph_and_markup() {
    assert!(!names_non_graph_host(
        r#"const GRAPH: &str = "https://graph.microsoft.com";"#
    ));
    assert!(!names_non_graph_host(
        r#"itemtype="http://schema.skype.com/Mention""#
    ));
    assert!(names_non_graph_host(
        r#"format!("https://teams.cloud.microsoft/api/mt/{region}")"#
    ));
}
