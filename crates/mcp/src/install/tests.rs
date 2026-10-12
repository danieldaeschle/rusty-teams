use std::fs;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use serde_json::Value;

use super::*;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/install")
        .join(name);
    fs::read_to_string(path).unwrap()
}

fn endpoint(token: &str) -> McpEndpoint {
    McpEndpoint {
        url: "http://127.0.0.1:8765/mcp".to_string(),
        token: token.to_string(),
    }
}

fn homes_in(directory: &Path) -> Homes {
    Homes {
        entries: vec![(Location::Native, directory.to_path_buf())],
    }
}

fn config(target: Target, directory: &Path) -> PathBuf {
    target.config_path(&Location::Native, directory)
}

fn seed(target: Target, directory: &Path, contents: &str) -> PathBuf {
    let path = config(target, directory);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

fn backup_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(".bak-rusty-teams");
    path.with_file_name(name)
}

#[test]
fn install_into_missing_file_creates_it() {
    for target in [
        Target::ClaudeCode,
        Target::Codex,
        Target::CopilotCli,
        Target::Cursor,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        fs::create_dir_all(config(target, home).parent().unwrap()).unwrap();
        fs::create_dir_all(home.join(".claude")).unwrap();
        let homes = homes_in(home);
        let report = install(target, &endpoint("one"), &homes).unwrap();
        assert_eq!(report, InstallReport::Written(vec![Location::Native]));
        let state = status(target, &endpoint("one"), &homes);
        assert_eq!(state.installed_at, vec![Location::Native]);
        assert!(!state.stale);
    }
}

#[test]
fn install_without_agent_is_not_available() {
    let directory = tempfile::tempdir().unwrap();
    let homes = Homes {
        entries: vec![(
            Location::Wsl {
                distro: "Ubuntu".to_string(),
            },
            directory.path().to_path_buf(),
        )],
    };
    let result = install(Target::Cursor, &endpoint("one"), &homes);
    assert!(matches!(
        result,
        Err(InstallError::NotAvailable(Target::Cursor))
    ));
    assert!(!status(Target::Cursor, &endpoint("one"), &homes).available);
}

#[test]
fn install_json_keeps_unrelated_keys_and_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = seed(
        Target::ClaudeCode,
        directory.path(),
        &fixture("claude_with_keys.json"),
    );
    install(
        Target::ClaudeCode,
        &endpoint("one"),
        &homes_in(directory.path()),
    )
    .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["numStartups"], 12);
    assert_eq!(value["mcpServers"]["other"]["command"], "npx");
    assert_eq!(value["mcpServers"]["teams"]["type"], "http");
    assert_eq!(
        value["mcpServers"]["teams"]["headers"]["Authorization"],
        "Bearer one"
    );
    let num_startups = text.find("numStartups").unwrap();
    let theme = text.find("\"theme\"").unwrap();
    let servers = text.find("mcpServers").unwrap();
    let projects = text.find("projects").unwrap();
    assert!(num_startups < theme && theme < servers && servers < projects);
    assert!(text.find("\"other\"").unwrap() < text.find("\"teams\"").unwrap());
}

#[test]
fn install_toml_keeps_comments() {
    let directory = tempfile::tempdir().unwrap();
    let path = seed(
        Target::Codex,
        directory.path(),
        &fixture("codex_with_comments.toml"),
    );
    install(Target::Codex, &endpoint("one"), &homes_in(directory.path())).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# my codex settings"));
    assert!(text.contains("model = \"gpt-5\"  # default model"));
    assert!(text.contains("[mcp_servers.teams]"));
    assert!(text.contains("tool_timeout_sec = 300"));
    assert!(text.contains("http_headers = { Authorization = \"Bearer one\" }"));
}

#[test]
fn install_twice_changes_nothing() {
    for target in [Target::ClaudeCode, Target::Codex, Target::CopilotCli] {
        let directory = tempfile::tempdir().unwrap();
        let fixture_name = match target {
            Target::Codex => "codex_with_comments.toml",
            _ => "claude_with_keys.json",
        };
        let path = seed(target, directory.path(), &fixture(fixture_name));
        let homes = homes_in(directory.path());
        install(target, &endpoint("one"), &homes).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        install(target, &endpoint("one"), &homes).unwrap();
        assert_eq!(first, fs::read_to_string(&path).unwrap());
    }
}

#[test]
fn token_change_is_stale_until_reinstall() {
    for target in [Target::ClaudeCode, Target::Codex, Target::Cursor] {
        let directory = tempfile::tempdir().unwrap();
        let homes = homes_in(directory.path());
        fs::create_dir_all(config(target, directory.path()).parent().unwrap()).unwrap();
        install(target, &endpoint("one"), &homes).unwrap();
        assert!(status(target, &endpoint("two"), &homes).stale);
        let results = reinstall_all(&endpoint("two"), &homes);
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        let state = status(target, &endpoint("two"), &homes);
        assert!(!state.stale);
        assert_eq!(state.installed_at, vec![Location::Native]);
    }
}

#[test]
fn uninstall_removes_only_teams() {
    let directory = tempfile::tempdir().unwrap();
    let homes = homes_in(directory.path());
    let json_path = seed(
        Target::ClaudeCode,
        directory.path(),
        &fixture("claude_with_keys.json"),
    );
    let toml_path = seed(
        Target::Codex,
        directory.path(),
        &fixture("codex_with_comments.toml"),
    );
    for target in [Target::ClaudeCode, Target::Codex] {
        install(target, &endpoint("one"), &homes).unwrap();
        uninstall(target, &homes).unwrap();
        assert!(
            status(target, &endpoint("one"), &homes)
                .installed_at
                .is_empty()
        );
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["other"]["command"], "npx");
    assert!(value["mcpServers"].get("teams").is_none());
    let toml_text = fs::read_to_string(&toml_path).unwrap();
    assert!(toml_text.contains("[mcp_servers.other]"));
    assert!(toml_text.contains("# my codex settings"));
    assert!(!toml_text.contains("teams"));
}

#[test]
fn malformed_file_is_refused_and_untouched() {
    for (target, name) in [
        (Target::ClaudeCode, "malformed.json"),
        (Target::Codex, "malformed.toml"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let original = fixture(name);
        let path = seed(target, directory.path(), &original);
        let homes = homes_in(directory.path());
        let result = install(target, &endpoint("one"), &homes);
        assert!(matches!(result, Err(InstallError::Malformed { .. })));
        assert!(matches!(
            uninstall(target, &homes),
            Err(InstallError::Malformed { .. })
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(!backup_of(&path).exists());
    }
}

#[test]
fn backup_is_created_once_with_original_content() {
    let directory = tempfile::tempdir().unwrap();
    let original = fixture("claude_with_keys.json");
    let path = seed(Target::ClaudeCode, directory.path(), &original);
    let homes = homes_in(directory.path());
    install(Target::ClaudeCode, &endpoint("one"), &homes).unwrap();
    install(Target::ClaudeCode, &endpoint("two"), &homes).unwrap();
    assert_eq!(fs::read_to_string(backup_of(&path)).unwrap(), original);
}

#[test]
fn vscode_install_returns_deeplink_that_round_trips() {
    let directory = tempfile::tempdir().unwrap();
    let report = install(
        Target::VsCode,
        &endpoint("tok en&1"),
        &homes_in(directory.path()),
    )
    .unwrap();
    let InstallReport::OpenUrl(url) = report else {
        panic!("expected a deeplink");
    };
    let encoded = url.strip_prefix("vscode:mcp/install?").unwrap();
    assert!(!encoded.contains(['{', '"', ' ', '&']));
    let decoded = percent_decode_str(encoded).decode_utf8().unwrap();
    let value: Value = serde_json::from_str(&decoded).unwrap();
    assert_eq!(value["name"], "teams");
    assert_eq!(value["type"], "http");
    assert_eq!(value["url"], "http://127.0.0.1:8765/mcp");
    assert_eq!(value["headers"]["Authorization"], "Bearer tok en&1");
}

#[test]
fn vscode_status_reads_servers_key() {
    let directory = tempfile::tempdir().unwrap();
    seed(
        Target::VsCode,
        directory.path(),
        r#"{"servers":{"teams":{"type":"http","url":"http://127.0.0.1:8765/mcp","headers":{"Authorization":"Bearer one"}}}}"#,
    );
    let state = status(
        Target::VsCode,
        &endpoint("one"),
        &homes_in(directory.path()),
    );
    assert!(state.available);
    assert_eq!(state.installed_at, vec![Location::Native]);
    assert!(!state.stale);
}

#[test]
fn wsl_output_decodes_utf16_with_bom() {
    let text = "Ubuntu\r\ndocker-desktop\r\n";
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(wsl::decode_output(&bytes), text);
    let without_bom: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(wsl::decode_output(&without_bom), text);
    assert_eq!(wsl::decode_output(b"plain\n"), "plain\n");
}

#[test]
fn wsl_default_distro_skips_docker_and_markers() {
    assert_eq!(
        wsl::default_distro("docker-desktop\r\nUbuntu-24.04 (Default)\r\nDebian\r\n"),
        Some("Ubuntu-24.04".to_string())
    );
    assert_eq!(
        wsl::default_distro("Ubuntu\r\nDebian\r\n"),
        Some("Ubuntu".to_string())
    );
    assert_eq!(wsl::default_distro("\r\n"), None);
}

#[test]
fn wsl_unc_home_is_built_from_distro_and_linux_home() {
    assert_eq!(
        wsl::unc_home("Ubuntu", "/home/daniel\n").to_string_lossy(),
        "\\\\wsl.localhost\\Ubuntu\\home\\daniel"
    );
}

#[test]
fn wslconfig_mirrored_mode_is_detected() {
    assert!(wsl::wslconfig_is_mirrored(
        "[wsl2]\nmemory=8GB\nNetworkingMode = Mirrored\n"
    ));
    assert!(!wsl::wslconfig_is_mirrored("[wsl2]\nnetworkingMode=nat\n"));
    assert!(!wsl::wslconfig_is_mirrored(
        "[wsl2]\n# networkingMode=mirrored\n"
    ));
}
