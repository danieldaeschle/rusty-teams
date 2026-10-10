use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use crate::capture::Capture;
use crate::devices::DeviceChoice;
use crate::error::{Error, Result};
use crate::pcm::{MonoResampler, SampleQueue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackKind {
    ProcessExclude,
    WholeEndpoint,
    Monitor,
    Synthetic,
}

impl LoopbackKind {
    pub fn excludes_own_playback(self) -> bool {
        matches!(self, LoopbackKind::ProcessExclude | LoopbackKind::Synthetic)
    }
}

pub struct LoopbackStart {
    pub capture: Capture,
    pub kind: LoopbackKind,
    pub warning: Option<String>,
}

fn audio_error(what: &str, error: impl std::fmt::Display) -> Error {
    Error::Audio(format!("{what}: {error}"))
}

fn named_input(choice: &DeviceChoice) -> Option<Device> {
    let DeviceChoice::Device(wanted) = choice else { return None };
    cpal::default_host()
        .input_devices()
        .ok()?
        .find(|device| device.description().is_ok_and(|description| description.name() == wanted))
}

pub fn start_microphone(choice: &DeviceChoice, queue: SampleQueue) -> Result<Capture> {
    let device = named_input(choice)
        .or_else(|| cpal::default_host().default_input_device())
        .ok_or_else(|| Error::Audio("no microphone".into()))?;
    let config = device.default_input_config().map_err(|error| audio_error("microphone format", error))?;
    start_cpal_input(device, config.sample_format(), config.config(), queue)
}

#[cfg(windows)]
pub fn start_loopback(queue: SampleQueue) -> Result<LoopbackStart> {
    match crate::loopback_windows::start_process_loopback(queue.clone(), std::process::id()) {
        Ok(capture) => Ok(LoopbackStart { capture, kind: LoopbackKind::ProcessExclude, warning: None }),
        Err(error) => {
            let capture = start_endpoint_loopback(queue)?;
            Ok(LoopbackStart {
                capture,
                kind: LoopbackKind::WholeEndpoint,
                warning: Some(format!("process loopback unavailable ({error}); capturing the whole endpoint, call audio may echo")),
            })
        }
    }
}

#[cfg(windows)]
fn start_endpoint_loopback(queue: SampleQueue) -> Result<Capture> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| Error::Audio("no output device".into()))?;
    let config = device.default_output_config().map_err(|error| audio_error("output format", error))?;
    start_cpal_input(device, config.sample_format(), config.config(), queue)
}

#[cfg(not(windows))]
pub fn start_loopback(queue: SampleQueue) -> Result<LoopbackStart> {
    Ok(LoopbackStart { capture: start_monitor(queue)?, kind: LoopbackKind::Monitor, warning: None })
}

#[cfg(not(windows))]
fn start_monitor(queue: SampleQueue) -> Result<Capture> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};

    const READ_BYTES: usize = 960;
    let mut child = Command::new("parec")
        .args(["--device=@DEFAULT_MONITOR@", "--format=s16le", "--rate=48000", "--channels=1", "--latency-msec=10"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| audio_error("parec", error))?;
    let mut output = child.stdout.take().ok_or_else(|| Error::Audio("parec without output".into()))?;
    let child = Arc::new(Mutex::new(child));
    let watched = child.clone();
    Capture::spawn_thread("sound-monitor", move |stop| {
        let watcher = std::thread::spawn({
            let stop = stop.clone();
            move || {
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                let mut child = watched.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let _ = child.kill();
                let _ = child.wait();
            }
        });
        let mut bytes = [0u8; READ_BYTES];
        while output.read_exact(&mut bytes).is_ok() {
            let samples: Vec<i16> = bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect();
            queue.push(&samples);
        }
        let _ = watcher.join();
    })
    .map_err(|error| audio_error("monitor thread", error))
}

fn start_cpal_input(device: Device, format: SampleFormat, config: StreamConfig, queue: SampleQueue) -> Result<Capture> {
    let (ready_sender, ready) = mpsc::channel::<Result<()>>();
    let capture = Capture::spawn_thread("sound-input", move |stop| {
        match build_stream(&device, format, &config, queue) {
            Ok(stream) => {
                let started = stream.play().map_err(|error| audio_error("start", error));
                let _ = ready_sender.send(started);
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            Err(error) => {
                let _ = ready_sender.send(Err(error));
            }
        }
    })
    .map_err(|error| audio_error("input thread", error))?;
    ready.recv().map_err(|_| Error::Audio("input thread ended".into()))??;
    Ok(capture)
}

fn build_stream(device: &Device, format: SampleFormat, config: &StreamConfig, queue: SampleQueue) -> Result<Stream> {
    let channels = usize::from(config.channels);
    let mut resampler = MonoResampler::new(config.sample_rate);
    let mut converted = Vec::new();
    let failed = |_error| {};
    match format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                converted.clear();
                resampler.convert(data, channels, &mut converted);
                queue.push(&converted);
            },
            failed,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| {
                converted.clear();
                resampler.convert(&crate::pcm::i16_to_f32(data), channels, &mut converted);
                queue.push(&converted);
            },
            failed,
            None,
        ),
        other => return Err(Error::Audio(format!("unsupported sample format {other:?}"))),
    }
    .map_err(|error| audio_error("open input", error))
}
