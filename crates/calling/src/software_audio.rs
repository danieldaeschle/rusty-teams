use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::prelude::*;
use tokio::task::JoinHandle;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE, SineGenerator};
use crate::audio_io::AudioMode;
use crate::capture::Capture;
use crate::devices::DeviceChoice;
use crate::error::{Error, Result};
use crate::mic_processing::{ApmConfig, MicSettings};
use crate::mixer::mix_frames;
use crate::noise_stage::NoiseStage;
use crate::pcm::SampleQueue;
use crate::sound_capture::{LoopbackKind, start_loopback, start_microphone};

const SOURCE_QUEUE_MS: u32 = 100;
const FRAME_PERIOD: Duration = Duration::from_millis(10);
const MAX_LAG: Duration = Duration::from_millis(100);
const SYSTEM_TONE_HZ: f32 = 660.0;
const SYNTHETIC_AMPLITUDE: f32 = 0.3;

enum FrameSource {
    Captured(SampleQueue),
    Tone(SineGenerator),
    Silence,
}

impl FrameSource {
    fn next_frame(&mut self) -> Vec<i16> {
        match self {
            FrameSource::Captured(queue) => queue.next_frame().0,
            FrameSource::Tone(generator) => generator.next_frame(),
            FrameSource::Silence => vec![0; FRAME_SAMPLES],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoftwareAudioPlan {
    pub processing: ApmConfig,
    pub ml_noise: bool,
    pub computer_sound: bool,
}

struct Feeder {
    processing: ApmConfig,
    ml_noise: Option<NoiseStage>,
    source: NativeAudioSource,
    microphone: FrameSource,
    computer: FrameSource,
    far_end: SampleQueue,
    microphone_muted: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    runtime: tokio::runtime::Handle,
}

impl Feeder {
    fn run(mut self) {
        let mut processing = self.processing.module();
        let mut due = Instant::now();
        while self.running.load(Ordering::SeqCst) {
            due += FRAME_PERIOD;
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            } else if now - due > MAX_LAG {
                due = now;
            }
            let (mut far_end, has_far_end) = self.far_end.next_frame();
            if has_far_end && let Some(processing) = processing.as_mut() {
                let _ = processing.process_reverse_stream(&mut far_end, SAMPLE_RATE as i32, 1);
            }
            let muted = self.microphone_muted.load(Ordering::SeqCst);
            let mut microphone = self.microphone.next_frame();
            if !muted && let Some(processing) = processing.as_mut() {
                let _ = processing.process_stream(&mut microphone, SAMPLE_RATE as i32, 1);
            }
            if !muted && let Some(noise_stage) = self.ml_noise.as_mut() {
                noise_stage.process_pcm(0, &mut microphone);
            }
            let computer = self.computer.next_frame();
            let frame = AudioFrame {
                data: mix_frames(&microphone, &computer, muted).into(),
                sample_rate: SAMPLE_RATE,
                num_channels: 1,
                samples_per_channel: FRAME_SAMPLES as u32,
            };
            if self.runtime.block_on(self.source.capture_frame(&frame)).is_err() {
                break;
            }
        }
    }
}

/// Mic through our own processing, plus computer sound when planned, as one track that replaces the device track.
pub struct SoftwareAudio {
    pub track: RtcAudioTrack,
    pub plan: SoftwareAudioPlan,
    pub loopback: Option<LoopbackKind>,
    pub warning: Option<String>,
    pub microphone_source: Option<String>,
    microphone_muted: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    feeder: Option<std::thread::JoinHandle<()>>,
    far_end_task: Option<JoinHandle<()>>,
    _captures: Vec<Capture>,
}

impl SoftwareAudioPlan {
    pub fn wanted(mic: MicSettings, sharing_sound: bool, uses_devices: bool) -> Option<Self> {
        Self::wanted_on(cfg!(windows), mic, sharing_sound, uses_devices)
    }

    pub fn wanted_on(windows: bool, mic: MicSettings, sharing_sound: bool, uses_devices: bool) -> Option<Self> {
        (sharing_sound || (uses_devices && !mic.uses_device_track_on(windows))).then(|| SoftwareAudioPlan {
            processing: mic.apm_config(),
            ml_noise: mic.ml_noise_suppression(),
            computer_sound: sharing_sound,
        })
    }
}

impl SoftwareAudio {
    pub fn start(
        factory: &PeerConnectionFactory,
        mode: AudioMode,
        tone_hz: f32,
        input: &DeviceChoice,
        plan: SoftwareAudioPlan,
        microphone_muted: bool,
        remote: Option<RtcAudioTrack>,
    ) -> Result<Self> {
        let mut captures = Vec::new();
        let mut warning = None;
        let mut microphone_source = None;
        let (microphone, computer, loopback) = if mode == AudioMode::Platform {
            let computer_queue = SampleQueue::default();
            let (computer, loopback) = if plan.computer_sound {
                let started = start_loopback(computer_queue.clone())?;
                captures.push(started.capture);
                warning = started.warning;
                (FrameSource::Captured(computer_queue), Some(started.kind))
            } else {
                (FrameSource::Silence, None)
            };
            let microphone_queue = SampleQueue::default();
            let microphone = match start_microphone(input, microphone_queue.clone()) {
                Ok(started) => {
                    microphone_source = Some(started.source);
                    captures.push(started.capture);
                    FrameSource::Captured(microphone_queue)
                }
                Err(error) => {
                    if !plan.computer_sound {
                        return Err(error);
                    }
                    warning.get_or_insert(format!("microphone not captured while sharing sound: {error}"));
                    FrameSource::Silence
                }
            };
            (microphone, computer, loopback)
        } else {
            let microphone = if mode == AudioMode::Tone {
                FrameSource::Tone(SineGenerator::new(tone_hz, SYNTHETIC_AMPLITUDE))
            } else {
                FrameSource::Silence
            };
            let computer = if plan.computer_sound {
                FrameSource::Tone(SineGenerator::new(SYSTEM_TONE_HZ, SYNTHETIC_AMPLITUDE))
            } else {
                FrameSource::Silence
            };
            (microphone, computer, plan.computer_sound.then_some(LoopbackKind::Synthetic))
        };
        let source = NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, SOURCE_QUEUE_MS);
        let track = factory.create_audio_track("microphone", source.clone());
        let far_end = SampleQueue::default();
        let far_end_task = remote.map(|remote| tokio::spawn(feed_far_end(remote, far_end.clone())));
        let microphone_muted = Arc::new(AtomicBool::new(microphone_muted));
        let running = Arc::new(AtomicBool::new(true));
        let feeder = Feeder {
            processing: plan.processing,
            ml_noise: plan.ml_noise.then(NoiseStage::new),
            source,
            microphone,
            computer,
            far_end,
            microphone_muted: microphone_muted.clone(),
            running: running.clone(),
            runtime: tokio::runtime::Handle::current(),
        };
        let thread = std::thread::Builder::new()
            .name("share-audio".into())
            .spawn(move || feeder.run())
            .map_err(|error| Error::Audio(format!("share audio thread: {error}")))?;
        Ok(SoftwareAudio {
            track,
            plan,
            loopback,
            warning,
            microphone_source,
            microphone_muted,
            running,
            feeder: Some(thread),
            far_end_task,
            _captures: captures,
        })
    }

    pub fn set_microphone_muted(&self, muted: bool) {
        self.microphone_muted.store(muted, Ordering::SeqCst);
    }
}

impl Drop for SoftwareAudio {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(task) = self.far_end_task.take() {
            task.abort();
        }
        if let Some(thread) = self.feeder.take() {
            let _ = thread.join();
        }
    }
}

async fn feed_far_end(remote: RtcAudioTrack, queue: SampleQueue) {
    let mut stream = NativeAudioStream::new(remote, SAMPLE_RATE as i32, 1);
    while let Some(frame) = stream.next().await {
        queue.push(&frame.data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::shared_factory;
    use crate::mic_processing::NoiseSuppression;

    #[tokio::test(flavor = "multi_thread")]
    async fn synthetic_sources_start_and_stop_cleanly() {
        let plan = SoftwareAudioPlan { processing: MicSettings::default().apm_config(), ml_noise: true, computer_sound: true };
        let audio = SoftwareAudio::start(&shared_factory(), AudioMode::Tone, 440.0, &DeviceChoice::SystemDefault, plan, true, None).unwrap();
        assert_eq!(audio.loopback, Some(LoopbackKind::Synthetic));
        assert!(audio.warning.is_none());
        audio.set_microphone_muted(false);
        tokio::time::sleep(Duration::from_millis(60)).await;
        drop(audio);
    }

    #[test]
    fn call_start_picks_the_path_per_platform_with_default_settings() {
        let default = MicSettings::default();
        assert!(SoftwareAudioPlan::wanted_on(true, default, false, true).is_some());
        assert!(SoftwareAudioPlan::wanted_on(false, default, false, true).is_none());
    }

    #[test]
    fn the_plan_follows_sharing_and_the_mic_choice() {
        let auto = MicSettings::default();
        let off = MicSettings { noise_suppression: NoiseSuppression::Off, music_mode: false };
        assert_eq!(SoftwareAudioPlan::wanted_on(false, auto, false, true), None);
        assert_eq!(SoftwareAudioPlan::wanted_on(false, off, false, false), None);
        let mic_only = SoftwareAudioPlan::wanted_on(false, off, false, true).unwrap();
        assert!(!mic_only.computer_sound && mic_only.processing.is_passthrough());
        let sharing = SoftwareAudioPlan::wanted_on(false, auto, true, false).unwrap();
        assert!(sharing.computer_sound && sharing.processing.echo_canceller);
    }

    #[test]
    fn windows_runs_the_software_path_for_every_mic_setting() {
        for noise_suppression in NoiseSuppression::ALL {
            for music_mode in [false, true] {
                let mic = MicSettings { noise_suppression, music_mode };
                assert!(SoftwareAudioPlan::wanted_on(true, mic, false, true).is_some());
                assert!(SoftwareAudioPlan::wanted_on(true, mic, false, false).is_none());
                assert_eq!(SoftwareAudioPlan::wanted_on(false, mic, false, true).is_some(), !mic.uses_device_track_on(false));
            }
        }
    }

    #[test]
    fn rnnoise_runs_in_the_software_path_only_for_auto() {
        let ml_noise = |noise_suppression, music_mode| {
            SoftwareAudioPlan::wanted_on(true, MicSettings { noise_suppression, music_mode }, false, true).unwrap().ml_noise
        };
        assert!(ml_noise(NoiseSuppression::Auto, false));
        assert!(!ml_noise(NoiseSuppression::Low, false));
        assert!(!ml_noise(NoiseSuppression::Off, false));
        assert!(!ml_noise(NoiseSuppression::Auto, true));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_microphone_only_plan_has_no_computer_sound() {
        let plan = SoftwareAudioPlan { processing: MicSettings::default().apm_config(), ml_noise: true, computer_sound: false };
        let audio = SoftwareAudio::start(&shared_factory(), AudioMode::Silence, 440.0, &DeviceChoice::SystemDefault, plan, false, None).unwrap();
        assert_eq!(audio.loopback, None);
        tokio::time::sleep(Duration::from_millis(60)).await;
        drop(audio);
    }
}
