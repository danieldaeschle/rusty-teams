use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::prelude::*;
use tokio::task::JoinHandle;
use tokio::time::interval;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE, SineGenerator};
use crate::devices::{self, DeviceChoice};

const TONE_AMPLITUDE: f32 = 0.3;
const SOURCE_QUEUE_MS: u32 = 100;
const FRAME_PERIOD: Duration = Duration::from_millis(10);
pub const AUDIO_MODE_ENV: &str = "CALLING_AUDIO";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioMode {
    #[default]
    Platform,
    Tone,
    Silence,
}

impl AudioMode {
    pub fn from_env() -> Self {
        Self::parse(std::env::var(AUDIO_MODE_ENV).ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("tone") => AudioMode::Tone,
            Some("silence") => AudioMode::Silence,
            _ => AudioMode::Platform,
        }
    }
}

pub struct AudioSetup {
    factory: PeerConnectionFactory,
    pub track: RtcAudioTrack,
    pub listen_only: bool,
    platform_acquired: bool,
    feeder: Option<(JoinHandle<()>, Arc<AtomicBool>)>,
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
        let has_input = platform_acquired && factory.recording_devices() > 0;
        let has_output = platform_acquired && factory.playout_devices() > 0;
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
            return AudioSetup {
                factory: factory.clone(),
                track: factory.create_device_audio_track("microphone"),
                listen_only: false,
                platform_acquired,
                feeder: None,
            };
        }
        let amplitude = if mode == AudioMode::Tone { TONE_AMPLITUDE } else { 0.0 };
        let source = NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, SOURCE_QUEUE_MS);
        let track = factory.create_audio_track("microphone", source.clone());
        let running = Arc::new(AtomicBool::new(true));
        let task = tokio::spawn(feed_frames(source, SineGenerator::new(tone_hz, amplitude), running.clone()));
        AudioSetup {
            factory: factory.clone(),
            track,
            listen_only: mode == AudioMode::Platform,
            platform_acquired,
            feeder: Some((task, running)),
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

async fn feed_frames(source: NativeAudioSource, mut generator: SineGenerator, running: Arc<AtomicBool>) {
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
        assert_eq!(AudioMode::parse(Some("whatever")), AudioMode::Platform);
    }
}
