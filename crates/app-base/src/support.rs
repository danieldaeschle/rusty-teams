use std::path::{Path, PathBuf};

use crate::diagnostics;
use crate::version;

pub const SUPPORT_BUNDLES_DIR: &str = "support-bundles";
pub const CALL_REPORTS_DIR: &str = "call-reports";
pub const CALL_DUMPS_DIR: &str = "call-dumps";
pub const CRASH_LOG_FILE: &str = "crash.log";

/// Upstream tracker — fork mirror has Issues disabled.
pub const GITHUB_ISSUES_NEW: &str = "https://github.com/danieldaeschle/rusty-teams/issues/new";

/// Runtime facts for Help, `client.md`, clipboard, and issue drafts. English keys only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    pub version: String,
    pub build: String,
    pub os: String,
    pub arch: String,
    pub session: String,
    pub ui_locale: String,
    pub timezone: String,
    pub signed_in: bool,
    pub call_reports: bool,
    pub debug_logging: bool,
    pub audio_input: String,
    pub audio_output: String,
    pub camera: String,
    pub logs_root: String,
    pub support_bundles_root: String,
}

impl ClientInfo {
    pub fn capture(
        package_version: &str,
        ui_locale: &str,
        signed_in: bool,
        call_reports: bool,
        audio_input: impl Into<String>,
        audio_output: impl Into<String>,
        camera: impl Into<String>,
    ) -> Self {
        let logs_root = local_data_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_owned());
        let support_bundles_root = support_bundles_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_owned());
        ClientInfo {
            version: package_version.to_owned(),
            build: version::build_version().to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            session: desktop_session(),
            ui_locale: ui_locale.to_owned(),
            timezone: timezone_name(),
            signed_in,
            call_reports,
            debug_logging: diagnostics::is_enabled(),
            audio_input: audio_input.into(),
            audio_output: audio_output.into(),
            camera: camera.into(),
            logs_root,
            support_bundles_root,
        }
    }

    pub fn markdown(&self) -> String {
        format!(
            "### Client\n\
             - Version: {}\n\
             - Build: {}\n\
             - OS: {} ({})\n\
             - Session: {}\n\
             - UI language: {}\n\
             - Timezone: {}\n\
             - Signed in: {}\n\
             - Call reports: {}\n\
             - Debug logging: {}\n\
             - Audio input: {}\n\
             - Audio output: {}\n\
             - Camera: {}\n\
             - Logs root: {}\n\
             - Support bundles: {}\n",
            self.version,
            self.build,
            self.os,
            self.arch,
            self.session,
            self.ui_locale,
            self.timezone,
            yes_no(self.signed_in),
            yes_no(self.call_reports),
            yes_no(self.debug_logging),
            self.audio_input,
            self.audio_output,
            self.camera,
            self.logs_root,
            self.support_bundles_root,
        )
    }

    pub fn one_line(&self) -> String {
        format!(
            "client version={} build={} os={}/{} session={} locale={} tz={} signed_in={} call_reports={} debug_logging={} logs={}",
            self.version,
            self.build,
            self.os,
            self.arch,
            self.session,
            self.ui_locale,
            self.timezone,
            yes_no(self.signed_in),
            yes_no(self.call_reports),
            yes_no(self.debug_logging),
            self.logs_root,
        )
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

pub fn desktop_session() -> String {
    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_SESSION_TYPE")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown".to_owned())
    }
    #[cfg(target_os = "macos")]
    {
        "macos".to_owned()
    }
    #[cfg(target_os = "windows")]
    {
        "windows".to_owned()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        "unknown".to_owned()
    }
}

fn timezone_name() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{}", chrono::Local::now().offset()))
}

/// OS local application data root for `ms-teams-linux` (legacy product directory name).
pub fn local_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().to_path_buf())
}

pub fn diagnostics_log_path() -> Option<PathBuf> {
    local_data_dir().map(|root| root.join(diagnostics::DIAGNOSTICS_FILE))
}

pub fn crash_log_path() -> Option<PathBuf> {
    local_data_dir().map(|root| root.join(CRASH_LOG_FILE))
}

pub fn call_dumps_dir() -> Option<PathBuf> {
    local_data_dir().map(|root| root.join(CALL_DUMPS_DIR))
}

pub fn call_reports_dir() -> Option<PathBuf> {
    local_data_dir().map(|root| root.join(CALL_REPORTS_DIR))
}

pub fn support_bundles_dir() -> Option<PathBuf> {
    local_data_dir().map(|root| root.join(SUPPORT_BUNDLES_DIR))
}

pub fn support_bundles_directory(data_dir: &Path) -> PathBuf {
    data_dir.join(SUPPORT_BUNDLES_DIR)
}

pub fn bundle_folder_name(utc: chrono::DateTime<chrono::Utc>) -> String {
    format!("support-{}", utc.format("%Y%m%dT%H%M%SZ"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportBundleEntry {
    pub directory: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleFileEntry {
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
}

/// Newest-first list of package folders under `support-bundles/`.
pub fn list_support_bundles(root: &Path) -> std::io::Result<Vec<SupportBundleEntry>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<SupportBundleEntry> = std::fs::read_dir(root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|directory| {
            let name = directory.file_name()?.to_string_lossy().into_owned();
            Some(SupportBundleEntry { directory, name })
        })
        .collect();
    entries.sort_by(|left, right| right.name.cmp(&left.name));
    Ok(entries)
}

pub fn list_bundle_files(bundle: &Path) -> std::io::Result<Vec<BundleFileEntry>> {
    if !bundle.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<BundleFileEntry> = std::fs::read_dir(bundle)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let size_bytes = std::fs::metadata(&path).ok()?.len();
            Some(BundleFileEntry {
                path,
                name,
                size_bytes,
            })
        })
        .collect();
    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

/// Remove one support package directory. Refuses paths outside `parent` when provided.
pub fn delete_support_bundle(bundle: &Path, parent: Option<&Path>) -> std::io::Result<()> {
    if let Some(parent) = parent {
        let canonical_parent = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
        let canonical_bundle = bundle.canonicalize().unwrap_or_else(|_| bundle.to_path_buf());
        if !canonical_bundle.starts_with(&canonical_parent) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "bundle path is outside the support-bundles directory",
            ));
        }
    }
    if bundle.is_dir() {
        std::fs::remove_dir_all(bundle)?;
    }
    Ok(())
}

pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

pub fn format_bytes(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{size} B")
    }
}

pub fn issue_title() -> &'static str {
    "Bug report"
}

pub fn issue_body(client: &ClientInfo, bundle: &SupportBundlePaths) -> String {
    format!(
        "## What happened\n\n\
         <!-- Describe the bug -->\n\n\
         ## Expected\n\n\
         <!-- What should happen -->\n\n\
         {}\n\
         Support package folder: `{}`\n\
         Support package path: `{}`\n\n\
         Attach the files from that folder (do not paste secrets).\n",
        client.markdown(),
        bundle.folder_name,
        bundle.directory.display(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportBundlePaths {
    pub directory: PathBuf,
    pub folder_name: String,
}

pub fn issue_url(client: &ClientInfo, bundle: &SupportBundlePaths) -> String {
    format!(
        "{GITHUB_ISSUES_NEW}?title={}&body={}",
        encode_query(issue_title()),
        encode_query(&issue_body(client, bundle))
    )
}

pub fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Append a client snapshot when debug logging is on (after store/locale when possible).
pub fn record_startup_client_line(
    package_version: &str,
    ui_locale: &str,
    signed_in: bool,
    call_reports: bool,
) {
    if !diagnostics::is_enabled() {
        return;
    }
    let info = ClientInfo::capture(
        package_version,
        ui_locale,
        signed_in,
        call_reports,
        "n/a",
        "n/a",
        "n/a",
    );
    diagnostics::record(info.one_line());
}

/// Chain a panic hook that also writes a short line into the diagnostics log.
pub fn install_panic_diagnostics_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .map(str::to_owned)
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-text panic payload".to_owned());
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown".to_owned());
        diagnostics::record(format!("panic at {location}: {message}"));
        previous(info);
    }));
}

pub fn readme_text() -> &'static str {
    "Attach the files in this folder to a GitHub issue on danieldaeschle/rusty-teams.\n\
     The app store, cookies, and tokens are not included.\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_client() -> ClientInfo {
        ClientInfo {
            version: "0.3.0".into(),
            build: "abc".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            session: "wayland".into(),
            ui_locale: "de".into(),
            timezone: "Europe/Zurich".into(),
            signed_in: true,
            call_reports: false,
            debug_logging: true,
            audio_input: "Default device".into(),
            audio_output: "Default device".into(),
            camera: "none".into(),
            logs_root: "/tmp/ms-teams-linux".into(),
            support_bundles_root: "/tmp/ms-teams-linux/support-bundles".into(),
        }
    }

    #[test]
    fn markdown_block_lists_stable_keys_and_paths() {
        let text = sample_client().markdown();
        assert!(text.contains("- Version: 0.3.0"));
        assert!(text.contains("- Build: abc"));
        assert!(text.contains("- OS: linux (x86_64)"));
        assert!(text.contains("- Session: wayland"));
        assert!(text.contains("- Signed in: yes"));
        assert!(text.contains("- Call reports: no"));
        assert!(text.contains("- Logs root: /tmp/ms-teams-linux"));
        assert!(text.contains("- Support bundles: /tmp/ms-teams-linux/support-bundles"));
        assert!(!text.contains('@'));
    }

    #[test]
    fn layout_relative_names_are_stable() {
        assert_eq!(SUPPORT_BUNDLES_DIR, "support-bundles");
        assert_eq!(CALL_REPORTS_DIR, "call-reports");
        assert_eq!(CALL_DUMPS_DIR, "call-dumps");
        assert_eq!(CRASH_LOG_FILE, "crash.log");
        assert_eq!(diagnostics::DIAGNOSTICS_FILE, "diagnostics.log");
        let root = PathBuf::from("/data/ms-teams-linux");
        assert_eq!(
            support_bundles_directory(&root),
            root.join("support-bundles")
        );
    }

    #[test]
    fn issue_url_encodes_spaces_and_newlines() {
        let info = sample_client();
        let bundle = SupportBundlePaths {
            directory: PathBuf::from("/tmp/ms-teams-linux/support-bundles/support-20260101T000000Z"),
            folder_name: "support-20260101T000000Z".into(),
        };
        let url = issue_url(&info, &bundle);
        assert!(url.starts_with(GITHUB_ISSUES_NEW));
        assert!(url.contains("title=Bug%20report"));
        assert!(url.contains("body="));
        assert!(url.contains("support-20260101T000000Z"));
        assert!(!url.contains(' '));
    }

    #[test]
    fn encode_query_percent_encodes_specials() {
        assert_eq!(encode_query("a b"), "a%20b");
        assert_eq!(encode_query("a&b"), "a%26b");
    }

    #[test]
    fn list_and_delete_support_bundles_in_temp_dir() {
        let root = tempfile::tempdir().unwrap();
        let bundles = root.path().join(SUPPORT_BUNDLES_DIR);
        let older = bundles.join("support-20260101T000000Z");
        let newer = bundles.join("support-20260102T000000Z");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(older.join("client.md"), "old").unwrap();
        std::fs::write(newer.join("client.md"), "new").unwrap();
        std::fs::write(newer.join("diagnostics.log"), "log").unwrap();

        let listed = list_support_bundles(&bundles).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "support-20260102T000000Z");
        assert_eq!(listed[1].name, "support-20260101T000000Z");

        let files = list_bundle_files(&newer).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "client.md");
        assert_eq!(files[1].name, "diagnostics.log");

        delete_support_bundle(&newer, Some(&bundles)).unwrap();
        let listed = list_support_bundles(&bundles).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "support-20260101T000000Z");
        assert!(!newer.exists());
    }

    #[test]
    fn delete_refuses_paths_outside_parent() {
        let root = tempfile::tempdir().unwrap();
        let bundles = root.path().join(SUPPORT_BUNDLES_DIR);
        let outside = root.path().join("other");
        std::fs::create_dir_all(&bundles).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        assert!(delete_support_bundle(&outside, Some(&bundles)).is_err());
        assert!(outside.exists());
    }
}
