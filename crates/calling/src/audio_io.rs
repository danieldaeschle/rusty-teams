use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::prelude::*;
use tokio::task::JoinHandle;
use tokio::time::interval;

use crate::audio::{ClipGenerator, FRAME_SAMPLES, SAMPLE_RATE, SineGenerator};
use crate::devices::{self, DeviceChoice};
use crate::mic_processing::{MicSettings, NoiseSuppression};

const TONE_AMPLITUDE: f32 = 0.3;
const SOURCE_QUEUE_MS: u32 = 100;
const FRAME_PERIOD: Duration = Duration::from_millis(10);
pub const AUDIO_MODE_ENV: &str = "CALLING_AUDIO";
pub const AUDIO_FILE_ENV: &str = "CALLING_AUDIO_FILE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioMode {
    #[default]
    Platform,
    Tone,
    Silence,
    File,
}

impl AudioMode {
    pub fn from_env() -> Self {
        Self::parse(std::env::var(AUDIO_MODE_ENV).ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("tone") => AudioMode::Tone,
            Some("silence") => AudioMode::Silence,
            Some("file") => AudioMode::File,
            _ => AudioMode::Platform,
        }
    }
}

pub struct AudioSetup {
    factory: PeerConnectionFactory,
    pub track: RtcAudioTrack,
    pub track_with_ns: Option<RtcAudioTrack>,
    pub listen_only: bool,
    platform_acquired: bool,
    pub adm_state: String,
    feeder: Option<(JoinHandle<()>, Arc<AtomicBool>)>,
}

fn device_count_text(count: Option<i16>) -> String {
    count.map_or_else(|| "n/a".to_owned(), |count| count.to_string())
}

impl AudioSetup {
    pub fn start(
        factory: &PeerConnectionFactory,
        mode: AudioMode,
        tone_hz: f32,
        input: &DeviceChoice,
        output: &DeviceChoice,
    ) -> Self {
        let platform_acquired = mode == AudioMode::Platform && factory.acquire_platform_adm();
        let (recording_devices, playout_devices) = if platform_acquired {
            (
                Some(factory.recording_devices()),
                Some(factory.playout_devices()),
            )
        } else {
            (None, None)
        };
        let has_input = recording_devices.is_some_and(|count| count > 0);
        let has_output = playout_devices.is_some_and(|count| count > 0);
        let adm_state = format!(
            "acquired {platform_acquired}, recording devices {}, playout devices {}, adm refs {}",
            device_count_text(recording_devices),
            device_count_text(playout_devices),
            factory.platform_adm_ref_count()
        );
        if platform_acquired {
            factory.set_adm_recording_enabled(has_input);
            factory.set_adm_playout_enabled(has_output);
            if has_input && *input != DeviceChoice::SystemDefault {
                devices::select_input(factory, input);
            }
            if has_output && *output != DeviceChoice::SystemDefault {
                devices::select_output(factory, output);
            }
        }
        if has_input {
            // Windows Auto disables built-in WebRTC NS so DeepFilterNet runs alone on 48 kHz mics
            // (with single-stage WebRTC NS fallback inside the capture hook for 16 kHz / warm-up).
            // Linux and macOS keep WebRTC NS on that track, then DeepFilterNet, as before.
            // Low mode switches to `track_with_ns` with built-in WebRTC NS enabled and hook off.
            let track =
                factory.create_device_audio_track_with_options("microphone", !cfg!(windows));
            let track_with_ns = Some(factory.create_device_audio_track("microphone-ns"));
            return AudioSetup {
                factory: factory.clone(),
                track,
                track_with_ns,
                listen_only: false,
                platform_acquired,
                adm_state,
                feeder: None,
            };
        }
        let generator = match mode {
            AudioMode::File => std::env::var_os(AUDIO_FILE_ENV)
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| ClipGenerator::from_wav(&bytes))
                .map_or(
                    FrameGenerator::Sine(SineGenerator::new(tone_hz, 0.0)),
                    FrameGenerator::Clip,
                ),
            AudioMode::Tone => FrameGenerator::Sine(SineGenerator::new(tone_hz, TONE_AMPLITUDE)),
            AudioMode::Platform | AudioMode::Silence => {
                FrameGenerator::Sine(SineGenerator::new(tone_hz, 0.0))
            }
        };
        let source = NativeAudioSource::new(
            AudioSourceOptions::default(),
            SAMPLE_RATE,
            1,
            SOURCE_QUEUE_MS,
        );
        let track = factory.create_audio_track("microphone", source.clone());
        let running = Arc::new(AtomicBool::new(true));
        let task = tokio::spawn(feed_frames(source, generator, running.clone()));
        AudioSetup {
            factory: factory.clone(),
            track,
            track_with_ns: None,
            listen_only: mode == AudioMode::Platform,
            platform_acquired,
            adm_state,
            feeder: Some((task, running)),
        }
    }

    pub fn device_track(&self, settings: MicSettings) -> &RtcAudioTrack {
        if classical_noise_track(settings)
            && let Some(track) = &self.track_with_ns
        {
            return track;
        }
        &self.track
    }

    pub fn set_device_tracks_enabled(&self, enabled: bool) {
        self.track.set_enabled(enabled);
        if let Some(track) = &self.track_with_ns {
            track.set_enabled(enabled);
        }
    }

    pub fn uses_devices(&self) -> bool {
        self.platform_acquired && self.feeder.is_none()
    }

    pub async fn stop(self) {
        if let Some((task, running)) = self.feeder {
            running.store(false, Ordering::SeqCst);
            let _ = task.await;
        }
        if self.platform_acquired {
            self.factory.set_adm_recording_enabled(false);
            self.factory.set_adm_playout_enabled(false);
            self.factory.release_platform_adm();
        }
    }
}

enum FrameGenerator {
    Sine(SineGenerator),
    Clip(ClipGenerator),
}

pub(crate) fn classical_noise_track(settings: MicSettings) -> bool {
    settings.noise_suppression == NoiseSuppression::Low && !settings.music_mode
}

impl FrameGenerator {
    fn next_frame(&mut self) -> Vec<i16> {
        match self {
            FrameGenerator::Sine(generator) => generator.next_frame(),
            FrameGenerator::Clip(generator) => generator.next_frame(),
        }
    }
}

async fn feed_frames(
    source: NativeAudioSource,
    mut generator: FrameGenerator,
    running: Arc<AtomicBool>,
) {
    let mut tick = interval(FRAME_PERIOD);
    while running.load(Ordering::SeqCst) {
        tick.tick().await;
        let frame = AudioFrame {
            data: generator.next_frame().into(),
            sample_rate: SAMPLE_RATE,
            num_channels: 1,
            samples_per_channel: FRAME_SAMPLES as u32,
        };
        if source.capture_frame(&frame).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_mode_parses_the_environment_value() {
        assert_eq!(AudioMode::parse(None), AudioMode::Platform);
        assert_eq!(AudioMode::parse(Some("tone")), AudioMode::Tone);
        assert_eq!(AudioMode::parse(Some("SILENCE")), AudioMode::Silence);
        assert_eq!(AudioMode::parse(Some("file")), AudioMode::File);
        assert_eq!(AudioMode::parse(Some("whatever")), AudioMode::Platform);
    }

    #[test]
    fn only_low_uses_the_classical_noise_track() {
        use crate::mic_processing::{MicSettings, NoiseSuppression};
        let mut settings = MicSettings::default();
        assert!(!classical_noise_track(settings));
        settings.noise_suppression = NoiseSuppression::Low;
        assert!(classical_noise_track(settings));
        settings.noise_suppression = NoiseSuppression::Off;
        assert!(!classical_noise_track(settings));
        settings.noise_suppression = NoiseSuppression::Low;
        settings.music_mode = true;
        assert!(!classical_noise_track(settings));
    }
}
