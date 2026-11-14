use std::backtrace::Backtrace;
use std::fs;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const CRASH_LOG_FILE: &str = "crash.log";
const MAX_BYTES: u64 = 256 * 1024;
const RING_SLOTS: u32 = 1;
const MERGE_CAP_BYTES: u64 = 128 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn install(path: PathBuf) {
    let _ = PATH.set(path.clone());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let body = entry(info, &Backtrace::force_capture().to_string());
        let _ = crate::diagnostics::append_rotated(&path, MAX_BYTES, RING_SLOTS, &body);
        previous(info);
    }));
}

pub fn path() -> Option<PathBuf> {
    PATH.get().cloned()
}

pub fn read_merged(max_bytes: u64) -> String {
    let Some(path) = PATH.get() else {
        return String::new();
    };
    crate::diagnostics::read_ring_merged(path, RING_SLOTS, max_bytes.min(MERGE_CAP_BYTES).max(1))
}

pub fn clear_files(root: &Path) {
    for name in [CRASH_LOG_FILE, &format!("{CRASH_LOG_FILE}.1")] {
        let path = root.join(name);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
}

fn entry(info: &PanicHookInfo<'_>, backtrace: &str) -> String {
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
        .unwrap_or_default();
    let thread = std::thread::current();
    format!(
        "=== {} build {} thread {}\npanic at {location}: {}\n{backtrace}\n",
        chrono::Utc::now().to_rfc3339(),
        crate::version::build_version(),
        thread.name().unwrap_or("unnamed"),
        message(info),
    )
}

fn message(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-text panic payload".to_owned())
}
