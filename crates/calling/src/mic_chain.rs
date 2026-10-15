use libwebrtc::native::apm::AudioProcessingModule;

use crate::audio::SAMPLE_RATE;
use crate::mic_processing::ApmConfig;
use crate::noise_stage::NoiseStage;
use crate::wav_dump::StageDumps;

/// High-pass and echo first, then DeepFilterNet, then AGC2 with its limiter, so the gain follows the cleaned voice.
pub struct MicChain {
    front: Option<AudioProcessingModule>,
    ml_noise: Option<NoiseStage>,
    gain: Option<AudioProcessingModule>,
    dumps: Option<StageDumps>,
}

impl MicChain {
    pub fn new(processing: ApmConfig, ml_noise: bool, dumps: Option<StageDumps>) -> MicChain {
        if ml_noise {
            crate::noise_stage::prepare();
        }
        MicChain {
            front: processing.front_module(),
            ml_noise: ml_noise.then(NoiseStage::new),
            gain: processing.gain_module(),
            dumps,
        }
    }

    pub fn process_far_end(&mut self, frame: &mut [i16]) {
        if let Some(front) = self.front.as_mut() {
            let _ = front.process_reverse_stream(frame, SAMPLE_RATE as i32, 1);
        }
    }

    pub fn process(&mut self, frame: &mut [i16]) {
        self.dump(0, frame);
        if let Some(front) = self.front.as_mut() {
            let _ = front.process_stream(frame, SAMPLE_RATE as i32, 1);
        }
        self.dump(1, frame);
        if let Some(noise_stage) = self.ml_noise.as_mut() {
            noise_stage.process_pcm(0, frame);
        }
        self.dump(2, frame);
        if let Some(gain) = self.gain.as_mut() {
            let _ = gain.process_stream(frame, SAMPLE_RATE as i32, 1);
        }
        self.dump(3, frame);
    }

    fn dump(&mut self, stage_index: usize, frame: &[i16]) {
        if let Some(dumps) = self.dumps.as_mut() {
            dumps.write(stage_index, frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::SineGenerator;
    use crate::mic_processing::MicSettings;

    #[test]
    fn off_leaves_the_frame_untouched() {
        let off = MicSettings { noise_suppression: crate::mic_processing::NoiseSuppression::Off, music_mode: false };
        let mut chain = MicChain::new(off.apm_config(), off.ml_noise_suppression(), None);
        let mut generator = SineGenerator::new(300.0, 0.2);
        let original = generator.next_frame();
        let mut frame = original.clone();
        chain.process(&mut frame);
        assert_eq!(frame, original);
    }
}
