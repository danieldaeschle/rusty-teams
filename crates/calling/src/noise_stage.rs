use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

use df::tract::{DfParams, DfTract, ReduceMask, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE};
use crate::mic_processing::MicSettings;

const PCM_SCALE: f32 = 32768.0;
const ATTENUATION_LIMIT_DB: f32 = 100.0;
const MIN_LOCAL_SNR_DB: f32 = -15.0;
const MAX_LOCAL_SNR_ERB_DB: f32 = 35.0;
const MAX_LOCAL_SNR_DF_DB: f32 = 35.0;
const WARM_UP_FRAMES: usize = 100;

static ENABLED: AtomicBool = AtomicBool::new(false);
static STAGE: Mutex<NoiseStage> = Mutex::new(NoiseStage::new());
static PROTOTYPE: Mutex<Option<DfTract>> = Mutex::new(None);
static PREPARE: Once = Once::new();

fn load_model() -> Option<DfTract> {
    let mut model = DfTract::new(DfParams::default(), &runtime_params(ATTENUATION_LIMIT_DB)).ok().filter(|model| model.hop_size == FRAME_SAMPLES)?;
    warm_up(&mut model);
    Some(model)
}

/// A cold model that hears only noise for a few seconds drops the next sentence; a second of vowel-like sound first avoids that.
fn warm_up(model: &mut DfTract) {
    let mut enhanced = vec![0.0; FRAME_SAMPLES];
    for frame_index in 0..WARM_UP_FRAMES {
        let noisy = vowel_frame(frame_index);
        let (Ok(noisy), Ok(enhanced)) =
            (ArrayView2::from_shape((1, FRAME_SAMPLES), &noisy), ArrayViewMut2::from_shape((1, FRAME_SAMPLES), &mut enhanced))
        else {
            return;
        };
        let _ = model.process(noisy, enhanced);
    }
}

fn vowel_frame(frame_index: usize) -> Vec<f32> {
    let pitch = 110.0 + 40.0 * ((frame_index as f32) * 0.07).sin();
    (0..FRAME_SAMPLES)
        .map(|sample_index| {
            let time = (frame_index * FRAME_SAMPLES + sample_index) as f32 / SAMPLE_RATE as f32;
            (1..=30)
                .map(|harmonic| {
                    let frequency = pitch * harmonic as f32;
                    let formant = [700.0_f32, 1200.0, 2600.0]
                        .iter()
                        .map(|center| (-((frequency - center) / 150.0).powi(2)).exp())
                        .sum::<f32>()
                        + 0.05;
                    formant * (std::f32::consts::TAU * frequency * time).sin()
                })
                .sum::<f32>()
                * 0.03
        })
        .collect()
}

/// The deep-filter CLI defaults; the library defaults (-10 dB floor) drop quiet sentences after a noisy start.
fn runtime_params(attenuation_limit_db: f32) -> RuntimeParams {
    RuntimeParams::default_with_ch(1)
        .with_atten_lim(attenuation_limit_db)
        .with_thresholds(MIN_LOCAL_SNR_DB, MAX_LOCAL_SNR_ERB_DB, MAX_LOCAL_SNR_DF_DB)
        .with_mask_reduce(ReduceMask::MAX)
}

/// Builds the DeepFilterNet3 model off the audio thread; frames pass through unchanged until it is ready.
pub fn prepare() {
    PREPARE.call_once(|| {
        let _ = std::thread::Builder::new().name("deep-filter-load".into()).spawn(|| {
            let model = load_model();
            *PROTOTYPE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = model;
        });
    });
}

pub fn is_ready() -> bool {
    PROTOTYPE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some()
}

fn ready_model() -> Option<DfTract> {
    PROTOTYPE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().map(DfTract::fork)
}

pub struct NoiseStage {
    channels: Vec<Option<DfTract>>,
    input: Vec<f32>,
    output: Vec<f32>,
    pcm: Vec<f32>,
}

impl NoiseStage {
    pub const fn new() -> Self {
        NoiseStage { channels: Vec::new(), input: Vec::new(), output: Vec::new(), pcm: Vec::new() }
    }

    pub fn process(&mut self, channel_index: usize, samples: &mut [f32]) {
        if samples.len() != FRAME_SAMPLES {
            return;
        }
        while self.channels.len() <= channel_index {
            self.channels.push(None);
        }
        let channel = &mut self.channels[channel_index];
        if channel.is_none() {
            *channel = ready_model();
        }
        let Some(model) = channel.as_mut() else {
            return;
        };
        self.input.clear();
        self.input.extend(samples.iter().map(|sample| sample / PCM_SCALE));
        self.output.resize(FRAME_SAMPLES, 0.0);
        let (Ok(noisy), Ok(enhanced)) = (
            ArrayView2::from_shape((1, FRAME_SAMPLES), &self.input),
            ArrayViewMut2::from_shape((1, FRAME_SAMPLES), &mut self.output),
        ) else {
            return;
        };
        if model.process(noisy, enhanced).is_err() {
            return;
        }
        for (sample, enhanced) in samples.iter_mut().zip(&self.output) {
            *sample = enhanced * PCM_SCALE;
        }
    }

    pub fn process_pcm(&mut self, channel_index: usize, samples: &mut [i16]) {
        let mut floats = std::mem::take(&mut self.pcm);
        floats.clear();
        floats.extend(samples.iter().map(|sample| f32::from(*sample)));
        self.process(channel_index, &mut floats);
        for (sample, processed) in samples.iter_mut().zip(&floats) {
            *sample = processed.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
        self.pcm = floats;
    }
}

impl Default for NoiseStage {
    fn default() -> Self {
        NoiseStage::new()
    }
}

fn process_capture(channel_index: usize, samples: &mut [f32]) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut stage = STAGE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    stage.process(channel_index, samples);
}

pub fn apply(settings: MicSettings) {
    if settings.ml_noise_suppression() {
        prepare();
    }
    webrtc_sys::noise_hook::set_capture_hook(Some(process_capture));
    ENABLED.store(settings.ml_noise_suppression(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn white_noise(frame_len: usize, amplitude: f32, state: &mut u32) -> Vec<f32> {
        (0..frame_len)
            .map(|_| {
                *state ^= *state << 13;
                *state ^= *state >> 17;
                *state ^= *state << 5;
                (*state as f32 / u32::MAX as f32 - 0.5) * 2.0 * amplitude
            })
            .collect()
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32
    }

    fn stage_with_model() -> NoiseStage {
        let model = load_model().expect("DeepFilterNet3 loads with a 10 ms hop at 48 kHz");
        NoiseStage { channels: vec![Some(model)], ..NoiseStage::new() }
    }

    #[test]
    fn white_noise_loses_most_of_its_energy() {
        let mut stage = stage_with_model();
        let mut state = 0x2545_f491_u32;
        let (mut before, mut after) = (0.0, 0.0);
        for frame_index in 0..200 {
            let mut frame = white_noise(FRAME_SAMPLES, 3000.0, &mut state);
            let input_energy = energy(&frame);
            stage.process(0, &mut frame);
            if frame_index >= 100 {
                before += input_energy;
                after += energy(&frame);
            }
        }
        assert!(after / before < 0.01, "energy ratio {}", after / before);
    }

    #[test]
    fn the_attenuation_limit_caps_what_the_model_removes() {
        let model = DfTract::new(DfParams::default(), &runtime_params(6.0)).unwrap();
        let mut stage = NoiseStage { channels: vec![Some(model)], ..NoiseStage::new() };
        let mut state = 0x1234_5678_u32;
        let (mut before, mut after) = (0.0, 0.0);
        for frame_index in 0..200 {
            let mut frame = white_noise(FRAME_SAMPLES, 3000.0, &mut state);
            let input_energy = energy(&frame);
            stage.process(0, &mut frame);
            if frame_index >= 100 {
                before += input_energy;
                after += energy(&frame);
            }
        }
        let ratio_db = 10.0 * (after / before).log10();
        assert!((-7.5..=-4.5).contains(&ratio_db), "noise reduced by {ratio_db:.1} dB");
    }

    #[test]
    fn other_frame_lengths_pass_through_unchanged() {
        let mut stage = stage_with_model();
        let mut state = 7_u32;
        for frame_len in [160, 441, FRAME_SAMPLES * 2] {
            let original = white_noise(frame_len, 3000.0, &mut state);
            let mut frame = original.clone();
            stage.process(0, &mut frame);
            assert_eq!(frame, original);
        }
    }

    #[test]
    fn deep_filter_keeps_up_with_real_time() {
        let mut stage = stage_with_model();
        let mut state = 11_u32;
        let frames = 300;
        let mut busy = Duration::ZERO;
        for _ in 0..frames {
            let mut frame = white_noise(FRAME_SAMPLES, 3000.0, &mut state);
            let started = Instant::now();
            stage.process(0, &mut frame);
            busy += started.elapsed();
        }
        let per_frame = busy / frames;
        eprintln!("deep filter per 10 ms frame: {per_frame:?}");
        assert!(per_frame < Duration::from_millis(if cfg!(debug_assertions) { 60 } else { 5 }));
    }
}
