use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const DIAGNOSTICS_FILE: &str = "diagnostics.log";
const MAX_BYTES: u64 = 1024 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

const WEBRTC_AUDIO_MARKERS: [&str; 3] = ["AdmProxy", "audio_device_core_win", "core_audio"];

pub fn install(path: PathBuf) {
    let _ = PATH.set(path);
    if log::set_logger(&WebrtcAudioLog).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}

struct WebrtcAudioLog;

impl log::Log for WebrtcAudioLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "libwebrtc"
    }

    fn log(&self, entry: &log::Record<'_>) {
        if !self.enabled(entry.metadata()) {
            return;
        }
        let text = entry.args().to_string();
        if WEBRTC_AUDIO_MARKERS.iter().any(|marker| text.contains(marker)) {
            record(format!("webrtc: {text}"));
        }
    }

    fn flush(&self) {}
}

pub fn directory() -> Option<PathBuf> {
    PATH.get().and_then(|path| path.parent()).map(PathBuf::from)
}

pub fn record(message: impl AsRef<str>) {
    let message = message.as_ref();
    eprintln!("{message}");
    let Some(path) = PATH.get() else {
        return;
    };
    let full = std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_BYTES);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(!full)
        .truncate(full)
        .open(path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{} {message}", line_prefix());
    }
}

fn line_prefix() -> String {
    format!(
        "{} {}",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        env!("TEAMS_BUILD_VERSION")
    )
}
