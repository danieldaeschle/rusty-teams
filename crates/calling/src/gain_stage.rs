use cxx::UniquePtr;
use webrtc_sys::apm::ffi::{AudioProcessingModule, create_gain_apm};

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE};

const LIFT_INITIAL_GAIN_DB: f32 = 10.0;
const LIFT_GAIN_CHANGE_DB_PER_SECOND: f32 = 10.0;
const OUTPUT_FIXED_GAIN_DB: f32 = 3.0;
const OUTPUT_GAIN_CHANGE_DB_PER_SECOND: f32 = 6.0;

/// One AGC2 (adaptive digital gain, then fixed gain, then its -1 dBFS limiter) on 10 ms mono frames.
pub struct GainStage {
    module: UniquePtr<AudioProcessingModule>,
}

impl GainStage {
    /// Before DeepFilterNet: brings a quiet laptop mic up fast, settles near AGC2's own -22 dBFS.
    pub fn lift() -> GainStage {
        GainStage { module: create_gain_apm(LIFT_INITIAL_GAIN_DB, 0.0, LIFT_GAIN_CHANGE_DB_PER_SECOND) }
    }

    /// Last stage: +6 dB on top of AGC2's target puts speech near -16 dBFS like Teams.
    pub fn output() -> GainStage {
        GainStage { module: create_gain_apm(0.0, OUTPUT_FIXED_GAIN_DB, OUTPUT_GAIN_CHANGE_DB_PER_SECOND) }
    }

    pub fn process(&mut self, frame: &mut [i16]) {
        if frame.len() != FRAME_SAMPLES || self.module.is_null() {
            return;
        }
        let length = frame.len();
        let pointer = frame.as_mut_ptr();
        unsafe {
            self.module.pin_mut().process_stream(pointer, length, pointer, length, SAMPLE_RATE as i32, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_dbfs(samples: &[i16]) -> f32 {
        let mean_square = samples.iter().map(|sample| f32::from(*sample).powi(2)).sum::<f32>() / samples.len() as f32;
        10.0 * (mean_square / 32768.0_f32.powi(2)).log10()
    }

    #[test]
    fn other_frame_lengths_pass_through() {
        let mut stage = GainStage::lift();
        let mut frame = vec![1000i16; 160];
        stage.process(&mut frame);
        assert_eq!(rms_dbfs(&frame), rms_dbfs(&[1000i16; 160]));
    }
}
