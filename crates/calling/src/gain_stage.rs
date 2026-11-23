use std::sync::atomic::{AtomicU32, Ordering};

use webrtc_sys::apm::GainProcessor;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE};

/// Start flat; quiet mics climb through adaptive AGC2 instead of blasting a hot input.
const LIFT_DEFAULT_GAIN_DB: f32 = 0.0;
const LIFT_GAIN_CHANGE_DB_PER_SECOND: f32 = 6.0;
const OUTPUT_MAX_GAIN_DB: f32 = 6.0;
/// Smooth knee for post-filter level recovery: full boost below -32 dBFS, 0 dB above -18 dBFS.
const OUTPUT_FULL_BOOST_DBFS: f32 = -32.0;
const OUTPUT_UNITY_DBFS: f32 = -18.0;
const OUTPUT_SMOOTH_ALPHA: f32 = 0.15;
const SPEECH_FLOOR_DBFS: f32 = -55.0;
const MIN_SPEECH_FRAMES: u32 = 100;

static LEARNED_LIFT_GAIN_DB: AtomicU32 = AtomicU32::new(LIFT_DEFAULT_GAIN_DB.to_bits());

fn frame_dbfs(frame: &[i16]) -> f32 {
    let mean_square = frame
        .iter()
        .map(|sample| f32::from(*sample).powi(2))
        .sum::<f32>()
        / frame.len().max(1) as f32;
    10.0 * (mean_square / 32768.0_f32.powi(2) + 1e-12).log10()
}

fn target_output_gain_db(input_dbfs: f32) -> f32 {
    if input_dbfs >= OUTPUT_UNITY_DBFS {
        0.0
    } else if input_dbfs <= OUTPUT_FULL_BOOST_DBFS {
        OUTPUT_MAX_GAIN_DB
    } else {
        let ratio = (OUTPUT_UNITY_DBFS - input_dbfs) / (OUTPUT_UNITY_DBFS - OUTPUT_FULL_BOOST_DBFS);
        OUTPUT_MAX_GAIN_DB * ratio
    }
}

/// One AGC2 (optional adaptive digital gain, or smooth output makeup with limiter) on 10 ms mono frames.
pub struct GainStage {
    module: GainProcessor,
    measured_gain_db_sum: f32,
    measured_frames: u32,
    learns: bool,
    smoothed_gain_db: f32,
}

impl GainStage {
    /// Before DeepFilterNet: brings a quiet laptop mic up; starts at the gain the last call settled on, so the first words already have the right level.
    pub fn lift() -> GainStage {
        let initial_gain_db = f32::from_bits(LEARNED_LIFT_GAIN_DB.load(Ordering::Relaxed));
        GainStage::new(
            true,
            initial_gain_db,
            0.0,
            LIFT_GAIN_CHANGE_DB_PER_SECOND,
            true,
        )
    }

    /// Last stage: smoothly recovers level lost to DeepFilterNet and limits peaks without hard-threshold step jumps.
    pub fn output() -> GainStage {
        GainStage::new(false, 0.0, 0.0, 1.0, false)
    }

    fn new(
        adaptive: bool,
        initial_gain_db: f32,
        fixed_gain_db: f32,
        max_gain_change_db_per_second: f32,
        learns: bool,
    ) -> GainStage {
        GainStage {
            module: GainProcessor::new(
                adaptive,
                initial_gain_db,
                fixed_gain_db,
                max_gain_change_db_per_second,
            ),
            measured_gain_db_sum: 0.0,
            measured_frames: 0,
            learns,
            smoothed_gain_db: 0.0,
        }
    }

    pub fn process(&mut self, frame: &mut [i16]) {
        if frame.len() != FRAME_SAMPLES {
            return;
        }
        let input_dbfs = frame_dbfs(frame);
        if !self.learns {
            let target_db = target_output_gain_db(input_dbfs);
            let prev_gain = 10.0_f32.powf(self.smoothed_gain_db / 20.0);
            self.smoothed_gain_db += (target_db - self.smoothed_gain_db) * OUTPUT_SMOOTH_ALPHA;
            let next_gain = 10.0_f32.powf(self.smoothed_gain_db / 20.0);
            if self.smoothed_gain_db > 0.05 || (next_gain - prev_gain).abs() > 1e-4 {
                let len = frame.len() as f32;
                for (index, sample) in frame.iter_mut().enumerate() {
                    let t = (index as f32 + 1.0) / len;
                    let gain = prev_gain + (next_gain - prev_gain) * t;
                    *sample = (f32::from(*sample) * gain)
                        .round()
                        .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
                        as i16;
                }
                let _ = self.module.process(frame, SAMPLE_RATE as i32);
            }
            return;
        }
        let processed = self.module.process(frame, SAMPLE_RATE as i32);
        if processed && input_dbfs > SPEECH_FLOOR_DBFS {
            self.measured_gain_db_sum += frame_dbfs(frame) - input_dbfs;
            self.measured_frames += 1;
        }
    }

    fn learned_gain_db(&self) -> Option<f32> {
        (self.measured_frames >= MIN_SPEECH_FRAMES)
            .then(|| self.measured_gain_db_sum / self.measured_frames as f32)
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
        let quiet_template: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|sample| ((sample as f32 * 0.04).sin() * 200.0) as i16)
            .collect();
        let before = frame_dbfs(&quiet_template);
        let mut quiet = quiet_template.clone();
        for _ in 0..30 {
            quiet = quiet_template.clone();
            stage.process(&mut quiet);
        }
        assert!(
            frame_dbfs(&quiet) - before > 4.5,
            "quiet speech should be lifted smoothly"
        );

        let mut fresh_stage = GainStage::output();
        let mut loud: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|sample| ((sample as f32 * 0.04).sin() * 8000.0) as i16)
            .collect();
        let loud_before = loud.clone();
        fresh_stage.process(&mut loud);
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
