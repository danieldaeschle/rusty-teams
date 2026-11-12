use std::sync::atomic::{AtomicU32, Ordering};

use webrtc_sys::apm::GainProcessor;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE};

/// Start flat; quiet mics climb through adaptive AGC2 instead of blasting a hot PipeWire input.
const LIFT_DEFAULT_GAIN_DB: f32 = 0.0;
const LIFT_GAIN_CHANGE_DB_PER_SECOND: f32 = 6.0;
const OUTPUT_FIXED_GAIN_DB: f32 = 8.0;
/// Only restore DeepFilterNet's level drop when the post-filter frame is still quiet.
const OUTPUT_APPLY_BELOW_DBFS: f32 = -25.0;
const SPEECH_FLOOR_DBFS: f32 = -55.0;
const MIN_SPEECH_FRAMES: u32 = 100;

static LEARNED_LIFT_GAIN_DB: AtomicU32 = AtomicU32::new(LIFT_DEFAULT_GAIN_DB.to_bits());

fn frame_dbfs(frame: &[i16]) -> f32 {
    let mean_square = frame.iter().map(|sample| f32::from(*sample).powi(2)).sum::<f32>() / frame.len().max(1) as f32;
    10.0 * (mean_square / 32768.0_f32.powi(2) + 1e-12).log10()
}

/// One AGC2 (optional adaptive digital gain, then fixed gain, then its limiter) on 10 ms mono frames.
pub struct GainStage {
    module: GainProcessor,
    measured_gain_db_sum: f32,
    measured_frames: u32,
    learns: bool,
}

impl GainStage {
    /// Before DeepFilterNet: brings a quiet laptop mic up; starts at the gain the last call settled on, so the first words already have the right level.
    pub fn lift() -> GainStage {
        let initial_gain_db = f32::from_bits(LEARNED_LIFT_GAIN_DB.load(Ordering::Relaxed));
        GainStage::new(true, initial_gain_db, 0.0, LIFT_GAIN_CHANGE_DB_PER_SECOND, true)
    }

    /// Last stage, fixed: makes up the level DeepFilterNet takes off and limits peaks; a second adaptive stage pumps against the first.
    pub fn output() -> GainStage {
        GainStage::new(false, 0.0, OUTPUT_FIXED_GAIN_DB, 1.0, false)
    }

    fn new(adaptive: bool, initial_gain_db: f32, fixed_gain_db: f32, max_gain_change_db_per_second: f32, learns: bool) -> GainStage {
        GainStage {
            module: GainProcessor::new(adaptive, initial_gain_db, fixed_gain_db, max_gain_change_db_per_second),
            measured_gain_db_sum: 0.0,
            measured_frames: 0,
            learns,
        }
    }

    pub fn process(&mut self, frame: &mut [i16]) {
        if frame.len() != FRAME_SAMPLES {
            return;
        }
        let input_dbfs = frame_dbfs(frame);
        if !self.learns && input_dbfs > OUTPUT_APPLY_BELOW_DBFS {
            return;
        }
        let processed = self.module.process(frame, SAMPLE_RATE as i32);
        if processed && self.learns && input_dbfs > SPEECH_FLOOR_DBFS {
            self.measured_gain_db_sum += frame_dbfs(frame) - input_dbfs;
            self.measured_frames += 1;
        }
    }

    fn learned_gain_db(&self) -> Option<f32> {
        (self.measured_frames >= MIN_SPEECH_FRAMES).then(|| self.measured_gain_db_sum / self.measured_frames as f32)
    }
}

impl Drop for GainStage {
    fn drop(&mut self) {
        if let Some(gain_db) = self.learned_gain_db() {
            LEARNED_LIFT_GAIN_DB.store(gain_db.clamp(0.0, 30.0).to_bits(), Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_frame_lengths_pass_through() {
        let mut stage = GainStage::output();
        let mut frame = vec![1000i16; 160];
        stage.process(&mut frame);
        assert_eq!(frame, vec![1000i16; 160]);
    }

    #[test]
    fn the_fixed_output_stage_adds_its_gain_only_to_quiet_frames() {
        let mut stage = GainStage::output();
        let mut quiet: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|sample| ((sample as f32 * 0.04).sin() * 200.0) as i16)
            .collect();
        let before = frame_dbfs(&quiet);
        stage.process(&mut quiet);
        assert!(frame_dbfs(&quiet) - before > 6.0, "quiet speech should be lifted");

        let mut loud: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|sample| ((sample as f32 * 0.04).sin() * 8000.0) as i16)
            .collect();
        let loud_before = loud.clone();
        stage.process(&mut loud);
        assert_eq!(loud, loud_before, "already-loud frames stay untouched");
    }

    #[test]
    fn learned_gain_needs_a_second_of_speech() {
        let mut stage = GainStage::lift();
        stage.learns = true;
        stage.measured_frames = 99;
        stage.measured_gain_db_sum = 99.0 * 15.0;
        assert_eq!(stage.learned_gain_db(), None);
        stage.measured_frames = 100;
        stage.measured_gain_db_sum = 100.0 * 15.0;
        assert_eq!(stage.learned_gain_db(), Some(15.0));
        stage.measured_frames = 0;
    }
}
