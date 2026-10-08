use std::backtrace::Backtrace;
use std::fs::OpenOptions;
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::PathBuf;

pub const CRASH_LOG_FILE: &str = "crash.log";

pub fn install(path: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(entry(info, &Backtrace::force_capture().to_string()).as_bytes());
        }
        previous(info);
    }));
}

fn entry(info: &PanicHookInfo<'_>, backtrace: &str) -> String {
    let location = info
        .location()
        .map(|location| format!("{}:{}:{}", location.file(), location.line(), location.column()))
        .unwrap_or_default();
    let thread = std::thread::current();
    format!(
        "=== {} build {} thread {}\npanic at {location}: {}\n{backtrace}\n",
        chrono::Utc::now().to_rfc3339(),
        env!("TEAMS_BUILD_VERSION"),
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
