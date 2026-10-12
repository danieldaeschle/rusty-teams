use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const DIAGNOSTICS_FILE: &str = "diagnostics.log";
const MAX_BYTES: u64 = 1024 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

const WEBRTC_AUDIO_MARKERS: [&str; 5] = [
    "AdmProxy",
    "audio_device",
    "core_audio",
    "WebRtcVoiceEngine",
    "webrtc_voice_engine",
];
const WEBRTC_SKIPPED: [&str; 1] = ["audio_device_buffer"];
const ENDPOINT_DUMP_LINES: std::ops::Range<u32> = 4040..4170;

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
        if keeps_webrtc_line(&text) {
            record(format!("webrtc: {text}"));
        }
    }

    fn flush(&self) {}
}

fn keeps_webrtc_line(text: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::keeps_webrtc_line;

    #[test]
    fn audio_lines_are_kept_and_endpoint_dumps_and_noise_are_skipped() {
        assert!(keeps_webrtc_line("(audio_device_impl.cc:123): Init"));
        assert!(keeps_webrtc_line(
            "(webrtc_voice_engine.cc:10): WebRtcVoiceEngine::Init"
        ));
        assert!(!keeps_webrtc_line(
            "(audio_device_core_win.cc:4090): endpoint 3"
        ));
        assert!(keeps_webrtc_line(
            "(audio_device_core_win.cc:412): AvRevertMmThreadCharacteristics() is OK"
        ));
        assert!(!keeps_webrtc_line(
            "(audio_device_buffer.cc:450): [REC : 10000msec, 48kHz] callbacks: 1000"
        ));
        assert!(!keeps_webrtc_line("(peer_connection.cc:55): something"));
    }
}
