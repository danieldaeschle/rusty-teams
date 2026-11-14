use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

pub const DIAGNOSTICS_FILE: &str = "diagnostics.log";
pub const DIAGNOSTICS_SETTING_KEY: &str = "diagnostics_enabled";
pub const ENV_DEBUG_LOG: &str = "RUSTY_TEAMS_DEBUG_LOG";

const MAX_BYTES: u64 = 1024 * 1024;
const RING_SLOTS: u32 = 2;
const MERGE_CAP_BYTES: u64 = 1024 * 1024;
const SLOW_UI_HANDLER: std::time::Duration = std::time::Duration::from_millis(30);

static PATH: OnceLock<PathBuf> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);

const OWN_CRATE_PREFIXES: [&str; 10] = [
    "app",
    "app_base",
    "state",
    "calling",
    "chatsvc",
    "session",
    "webview",
    "mcp",
    "agents",
    "teams",
];

const WEBRTC_AUDIO_MARKERS: [&str; 5] = [
    "AdmProxy",
    "audio_device",
    "core_audio",
    "WebRtcVoiceEngine",
    "webrtc_voice_engine",
];
const WEBRTC_SKIPPED: [&str; 3] = ["audio_device_buffer", "requesting data", "will write"];
const ENDPOINT_DUMP_LINES: std::ops::Range<u32> = 4040..4170;

pub fn install(path: PathBuf) {
    let _ = PATH.set(path);
    if log::set_logger(&DiagnosticsLog).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    crate::support::install_panic_diagnostics_hook();
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enable_from_override() -> bool {
    let on = std::env::args().any(|arg| arg == "--debug-log")
        || std::env::var(ENV_DEBUG_LOG)
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);
    if on {
        set_enabled(true);
    }
    on
}

pub fn load_enabled_from_store(stored: Option<&str>) -> bool {
    let on = stored == Some("1");
    set_enabled(on || is_enabled());
    is_enabled()
}

pub fn directory() -> Option<PathBuf> {
    PATH.get().and_then(|path| path.parent()).map(PathBuf::from)
}

pub fn path() -> Option<PathBuf> {
    PATH.get().cloned()
}

pub fn record_if_slow(label: &str, elapsed: std::time::Duration) {
    if elapsed >= SLOW_UI_HANDLER {
        record(format!(
            "slow ui handler: {label} took {} ms",
            elapsed.as_millis()
        ));
    }
}

pub fn record(message: impl AsRef<str>) {
    let message = message.as_ref();
    eprintln!("{message}");
    if !is_enabled() {
        return;
    }
    let Some(path) = PATH.get() else {
        return;
    };
    if let Err(error) = append_rotated(path, MAX_BYTES, RING_SLOTS, &format!("{} {message}\n", line_prefix()))
    {
        eprintln!("diagnostics write failed: {error}");
    }
}

/// Chronological merge of `.1` then current (capped), for support packages.
pub fn read_merged(max_bytes: u64) -> String {
    let Some(path) = PATH.get() else {
        return String::new();
    };
    read_ring_merged(path, 1, max_bytes.min(MERGE_CAP_BYTES).max(1))
}

pub fn clear_logs() -> std::io::Result<()> {
    let Some(root) = directory() else {
        return Ok(());
    };
    for name in [
        DIAGNOSTICS_FILE,
        &format!("{DIAGNOSTICS_FILE}.1"),
        &format!("{DIAGNOSTICS_FILE}.2"),
        crate::crash_log::CRASH_LOG_FILE,
        &format!("{}.1", crate::crash_log::CRASH_LOG_FILE),
    ] {
        let path = root.join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }
    let dumps = root.join(crate::support::CALL_DUMPS_DIR);
    if dumps.is_dir() {
        for entry in fs::read_dir(&dumps)?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            } else {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

pub(crate) fn append_rotated(
    path: &Path,
    max_bytes: u64,
    ring_slots: u32,
    line: &str,
) -> std::io::Result<()> {
    if path
        .metadata()
        .map(|meta| meta.len() >= max_bytes)
        .unwrap_or(false)
    {
        rotate_ring(path, ring_slots)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

fn rotate_ring(path: &Path, ring_slots: u32) -> std::io::Result<()> {
    for slot in (1..ring_slots).rev() {
        let from = ring_path(path, slot);
        let to = ring_path(path, slot + 1);
        if from.exists() {
            let _ = fs::remove_file(&to);
            fs::rename(&from, &to)?;
        }
    }
    let first = ring_path(path, 1);
    let _ = fs::remove_file(&first);
    if path.exists() {
        fs::rename(path, &first)?;
    }
    Ok(())
}

fn ring_path(path: &Path, slot: u32) -> PathBuf {
    path.with_file_name(format!(
        "{}.{}",
        path.file_name().and_then(|name| name.to_str()).unwrap_or(DIAGNOSTICS_FILE),
        slot
    ))
}

pub(crate) fn read_ring_merged(path: &Path, slots: u32, max_bytes: u64) -> String {
    let mut parts = Vec::new();
    for slot in (1..=slots).rev() {
        let sibling = ring_path(path, slot);
        if let Ok(text) = fs::read_to_string(&sibling) {
            parts.push(text);
        }
    }
    if let Ok(text) = fs::read_to_string(path) {
        parts.push(text);
    }
    let merged = parts.join("");
    if merged.len() as u64 <= max_bytes {
        return merged;
    }
    let start = merged.len() - max_bytes as usize;
    merged[start..].to_owned()
}

fn line_prefix() -> String {
    format!(
        "{} {}",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        crate::version::build_version()
    )
}

struct DiagnosticsLog;

impl log::Log for DiagnosticsLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        if !is_enabled() {
            return false;
        }
        let target = metadata.target();
        if target == "libwebrtc" {
            return true;
        }
        if is_own_crate(target) {
            return metadata.level() <= log::Level::Info;
        }
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, entry: &log::Record<'_>) {
        if !self.enabled(entry.metadata()) {
            return;
        }
        let target = entry.metadata().target();
        let text = entry.args().to_string();
        if target == "libwebrtc" {
            if keeps_webrtc_line(entry.metadata().level(), &text) {
                record(format!("webrtc: {text}"));
            }
            return;
        }
        let level = entry.metadata().level().as_str().to_ascii_lowercase();
        let short = target.split("::").next().unwrap_or(target);
        record(format!("{level} {short}: {text}"));
    }

    fn flush(&self) {}
}

fn is_own_crate(target: &str) -> bool {
    let root = target.split("::").next().unwrap_or(target);
    OWN_CRATE_PREFIXES
        .iter()
        .any(|prefix| root == *prefix || root.starts_with(&format!("{prefix}_")))
}

fn keeps_webrtc_line(level: log::Level, text: &str) -> bool {
    if level <= log::Level::Warn
        && (text.contains("peer_connection")
            || text.contains("PeerConnection")
            || text.contains("ICE")
            || text.contains("ice_"))
    {
        return true;
    }
    WEBRTC_AUDIO_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
        && !WEBRTC_SKIPPED.iter().any(|skipped| text.contains(skipped))
        && !is_endpoint_dump(text)
}

fn is_endpoint_dump(text: &str) -> bool {
    text.split_once("audio_device_core_win.cc:")
        .and_then(|(_, rest)| rest.split_once(')'))
        .and_then(|(line, _)| line.parse::<u32>().ok())
        .is_some_and(|line| ENDPOINT_DUMP_LINES.contains(&line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn audio_lines_are_kept_and_endpoint_dumps_and_noise_are_skipped() {
        assert!(keeps_webrtc_line(log::Level::Debug, "(audio_device_impl.cc:123): Init"));
        assert!(keeps_webrtc_line(
            log::Level::Debug,
            "(webrtc_voice_engine.cc:10): WebRtcVoiceEngine::Init"
        ));
        assert!(!keeps_webrtc_line(
            log::Level::Debug,
            "(audio_device_core_win.cc:4090): endpoint 3"
        ));
        assert!(keeps_webrtc_line(
            log::Level::Debug,
            "(audio_device_core_win.cc:412): AvRevertMmThreadCharacteristics() is OK"
        ));
        assert!(!keeps_webrtc_line(
            log::Level::Debug,
            "(audio_device_buffer.cc:450): [REC : 10000msec, 48kHz] callbacks: 1000"
        ));
        assert!(!keeps_webrtc_line(
            log::Level::Debug,
            "(audio_device_pulse_linux.cc:2236): requesting data"
        ));
        assert!(!keeps_webrtc_line(
            log::Level::Debug,
            "(audio_device_pulse_linux.cc:2256): will write"
        ));
        assert!(!keeps_webrtc_line(
            log::Level::Debug,
            "(peer_connection.cc:55): something"
        ));
        assert!(keeps_webrtc_line(
            log::Level::Warn,
            "(peer_connection.cc:55): ICE failed"
        ));
    }

    #[test]
    fn enabled_gate_defaults_and_toggles() {
        set_enabled(false);
        assert!(!is_enabled());
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(false);
        assert!(!is_enabled());
    }

    #[test]
    fn ring_rotation_preserves_previous_content() {
        let root = tempdir().unwrap();
        let path = root.path().join(DIAGNOSTICS_FILE);
        fs::write(&path, "a".repeat(MAX_BYTES as usize)).unwrap();
        append_rotated(&path, MAX_BYTES, RING_SLOTS, "fresh-line\n").unwrap();
        assert!(ring_path(&path, 1).exists());
        let current = fs::read_to_string(&path).unwrap();
        assert!(current.contains("fresh-line"));
        assert!(!current.contains(&"a".repeat(32)));
        let previous = fs::read_to_string(ring_path(&path, 1)).unwrap();
        assert!(previous.starts_with('a'));
    }

    #[test]
    fn own_crate_prefixes_match() {
        assert!(is_own_crate("app::linux_browser"));
        assert!(is_own_crate("state::notice"));
        assert!(is_own_crate("app_base::diagnostics"));
        assert!(!is_own_crate("hyper::client"));
    }
}
