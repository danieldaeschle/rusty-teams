use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::native::apm::AudioProcessingModule;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::prelude::*;
use tokio::task::JoinHandle;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE, SineGenerator};
use crate::audio_io::AudioMode;
use crate::capture::Capture;
use crate::devices::DeviceChoice;
use crate::error::{Error, Result};
use crate::mixer::mix_frames;
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

struct Feeder {
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
        let mut processing = AudioProcessingModule::new(true, false, true, true);
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
            if has_far_end {
                let _ = processing.process_reverse_stream(&mut far_end, SAMPLE_RATE as i32, 1);
            }
            let muted = self.microphone_muted.load(Ordering::SeqCst);
            let mut microphone = self.microphone.next_frame();
            if !muted {
                let _ = processing.process_stream(&mut microphone, SAMPLE_RATE as i32, 1);
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

/// Mic plus computer sound mixed into one track that replaces the device track while sharing.
pub struct ShareAudio {
    pub track: RtcAudioTrack,
    pub loopback: LoopbackKind,
    pub warning: Option<String>,
    microphone_muted: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    feeder: Option<std::thread::JoinHandle<()>>,
    far_end_task: Option<JoinHandle<()>>,
    _captures: Vec<Capture>,
}

impl ShareAudio {
    pub fn start(
        factory: &PeerConnectionFactory,
        mode: AudioMode,
        tone_hz: f32,
        input: &DeviceChoice,
        microphone_muted: bool,
        remote: Option<RtcAudioTrack>,
    ) -> Result<Self> {
        let mut captures = Vec::new();
        let mut warning = None;
        let (microphone, computer, loopback) = if mode == AudioMode::Platform {
            let computer_queue = SampleQueue::default();
            let started = start_loopback(computer_queue.clone())?;
            captures.push(started.capture);
            warning = started.warning;
            let microphone_queue = SampleQueue::default();
            let microphone = match start_microphone(input, microphone_queue.clone()) {
                Ok(capture) => {
                    captures.push(capture);
                    FrameSource::Captured(microphone_queue)
                }
                Err(error) => {
                    warning.get_or_insert(format!("microphone not captured while sharing sound: {error}"));
                    FrameSource::Silence
                }
            };
            (microphone, FrameSource::Captured(computer_queue), started.kind)
        } else {
            let microphone = if mode == AudioMode::Tone {
                FrameSource::Tone(SineGenerator::new(tone_hz, SYNTHETIC_AMPLITUDE))
            } else {
                FrameSource::Silence
            };
            (microphone, FrameSource::Tone(SineGenerator::new(SYSTEM_TONE_HZ, SYNTHETIC_AMPLITUDE)), LoopbackKind::Synthetic)
        };
        let source = NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, SOURCE_QUEUE_MS);
        let track = factory.create_audio_track("microphone", source.clone());
        let far_end = SampleQueue::default();
        let far_end_task = remote.map(|remote| tokio::spawn(feed_far_end(remote, far_end.clone())));
        let microphone_muted = Arc::new(AtomicBool::new(microphone_muted));
        let running = Arc::new(AtomicBool::new(true));
        let feeder = Feeder {
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
        Ok(ShareAudio {
            track,
            loopback,
            warning,
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

impl Drop for ShareAudio {
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

    #[tokio::test(flavor = "multi_thread")]
    async fn synthetic_sources_start_and_stop_cleanly() {
        let audio = ShareAudio::start(&shared_factory(), AudioMode::Tone, 440.0, &DeviceChoice::SystemDefault, true, None).unwrap();
        assert_eq!(audio.loopback, LoopbackKind::Synthetic);
        assert!(audio.warning.is_none());
        audio.set_microphone_muted(false);
        tokio::time::sleep(Duration::from_millis(60)).await;
        drop(audio);
    }
}
