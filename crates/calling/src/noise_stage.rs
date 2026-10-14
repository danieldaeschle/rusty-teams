use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use nnnoiseless::{DenoiseState, FRAME_SIZE};

use crate::mic_processing::MicSettings;

static ENABLED: AtomicBool = AtomicBool::new(false);
static STAGE: Mutex<NoiseStage> = Mutex::new(NoiseStage::new());

pub struct NoiseStage {
    channels: Vec<Box<DenoiseState<'static>>>,
}

impl NoiseStage {
    pub const fn new() -> Self {
        NoiseStage { channels: Vec::new() }
    }

    pub fn process(&mut self, channel_index: usize, samples: &mut [f32]) {
        if samples.len() != FRAME_SIZE {
            return;
        }
        while self.channels.len() <= channel_index {
            self.channels.push(DenoiseState::new());
        }
        let input: [f32; FRAME_SIZE] = std::array::from_fn(|index| samples[index]);
        self.channels[channel_index].process_frame(samples, &input);
    }

    pub fn process_pcm(&mut self, channel_index: usize, samples: &mut [i16]) {
        let mut floats: Vec<f32> = samples.iter().map(|sample| f32::from(*sample)).collect();
        self.process(channel_index, &mut floats);
        for (sample, processed) in samples.iter_mut().zip(floats) {
            *sample = processed.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
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
    webrtc_sys::noise_hook::set_capture_hook(Some(process_capture));
    ENABLED.store(settings.ml_noise_suppression(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white_noise(frame_len: usize) -> Vec<f32> {
        let mut state = 0x2545_f491_u32;
        (0..frame_len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 - 0.5) * 6000.0
            })
            .collect()
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32
    }

    #[test]
    fn white_noise_loses_most_of_its_energy() {
        let mut stage = NoiseStage::new();
        let mut last_energy_ratio = 1.0;
        for _ in 0..50 {
            let mut frame = white_noise(FRAME_SIZE);
            let before = energy(&frame);
            stage.process(0, &mut frame);
            last_energy_ratio = energy(&frame) / before;
        }
        assert!(last_energy_ratio < 0.1, "energy ratio {last_energy_ratio}");
    }

    #[test]
    fn other_frame_lengths_pass_through_unchanged() {
        let mut stage = NoiseStage::new();
        for frame_len in [160, 441, FRAME_SIZE * 2] {
            let original = white_noise(frame_len);
            let mut frame = original.clone();
            stage.process(0, &mut frame);
            assert_eq!(frame, original);
        }
    }

    #[test]
    fn each_channel_keeps_its_own_state() {
        let mut stage = NoiseStage::new();
        let mut frame = white_noise(FRAME_SIZE);
        stage.process(1, &mut frame);
        assert_eq!(stage.channels.len(), 2);
    }

    #[test]
    fn rnnoise_costs_little_per_ten_ms_frame() {
        let mut stage = NoiseStage::new();
        let frames = 500;
        let started = std::time::Instant::now();
        for _ in 0..frames {
            let mut frame = white_noise(FRAME_SIZE);
            stage.process(0, &mut frame);
        }
        let per_frame = started.elapsed() / frames;
        eprintln!("rnnoise per 10 ms frame (incl. noise generation): {per_frame:?}");
        assert!(per_frame < std::time::Duration::from_millis(5));
    }
}
