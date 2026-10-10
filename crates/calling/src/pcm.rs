use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE};

const MAX_QUEUED_SAMPLES: usize = SAMPLE_RATE as usize / 5;

#[derive(Clone, Default)]
pub struct SampleQueue {
    samples: Arc<Mutex<VecDeque<i16>>>,
}

impl SampleQueue {
    pub fn push(&self, samples: &[i16]) {
        let mut queued = self.samples.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        queued.extend(samples.iter().copied());
        let excess = queued.len().saturating_sub(MAX_QUEUED_SAMPLES);
        queued.drain(..excess);
    }

    pub fn next_frame(&self) -> (Vec<i16>, bool) {
        let mut queued = self.samples.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let available = queued.len().min(FRAME_SAMPLES);
        let mut frame: Vec<i16> = queued.drain(..available).collect();
        frame.resize(FRAME_SAMPLES, 0);
        (frame, available > 0)
    }
}

/// Downmixes interleaved floats to mono and converts them to 48 kHz with linear interpolation.
pub struct MonoResampler {
    step: f64,
    position: f64,
    previous: f32,
}

impl MonoResampler {
    pub fn new(source_rate: u32) -> Self {
        MonoResampler {
            step: f64::from(source_rate) / f64::from(SAMPLE_RATE),
            position: 0.0,
            previous: 0.0,
        }
    }

    pub fn convert(&mut self, interleaved: &[f32], channels: usize, output: &mut Vec<i16>) {
        let channels = channels.max(1);
        for frame in interleaved.chunks_exact(channels) {
            let current = frame.iter().sum::<f32>() / channels as f32;
            while self.position < 1.0 {
                let value = self.previous + (current - self.previous) * self.position as f32;
                output.push((value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16);
                self.position += self.step;
            }
            self.position -= 1.0;
            self.previous = current;
        }
    }
}

pub fn i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&sample| f32::from(sample) / f32::from(i16::MAX)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_come_out_in_order_and_pad_with_silence() {
        let queue = SampleQueue::default();
        queue.push(&[1, 2, 3]);
        let (frame, had_data) = queue.next_frame();
        assert!(had_data);
        assert_eq!(frame.len(), FRAME_SAMPLES);
        assert_eq!(&frame[..4], &[1, 2, 3, 0]);
        assert!(!queue.next_frame().1);
    }

    #[test]
    fn the_queue_drops_the_oldest_samples_when_the_reader_stalls() {
        let queue = SampleQueue::default();
        queue.push(&vec![1; MAX_QUEUED_SAMPLES]);
        queue.push(&vec![2; FRAME_SAMPLES]);
        let (frame, _) = queue.next_frame();
        assert_eq!(frame[0], 1);
        let mut last = frame;
        for _ in 1..MAX_QUEUED_SAMPLES / FRAME_SAMPLES {
            last = queue.next_frame().0;
        }
        assert_eq!(last[FRAME_SAMPLES - 1], 2);
    }

    #[test]
    fn stereo_at_24_khz_becomes_mono_at_48_khz() {
        let mut resampler = MonoResampler::new(24_000);
        let mut output = Vec::new();
        resampler.convert(&[0.5, 0.5].repeat(240), 2, &mut output);
        assert_eq!(output.len(), 480);
        assert!(output[10..].iter().all(|&sample| (sample - (0.5 * f32::from(i16::MAX)) as i16).abs() < 2));
    }

    #[test]
    fn the_native_rate_passes_through() {
        let mut resampler = MonoResampler::new(48_000);
        let mut output = Vec::new();
        resampler.convert(&[0.25; 480], 1, &mut output);
        assert_eq!(output.len(), 480);
    }
}
