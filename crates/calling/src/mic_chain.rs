use libwebrtc::native::apm::AudioProcessingModule;

use crate::audio::SAMPLE_RATE;
use crate::gain_stage::GainStage;
use crate::mic_processing::ApmConfig;
use crate::noise_stage::NoiseStage;
use crate::voice_high_pass::VoiceHighPass;
use crate::wav_dump::StageDumps;

/// DeepFilterNet drops quiet laptop-mic speech as noise and takes about 6 dB off reverberant speech,
/// so one AGC2 brings the level up before it and a second one restores the level after it.
pub struct MicChain {
    high_pass: Option<VoiceHighPass>,
    front: Option<AudioProcessingModule>,
    ml_noise: Option<NoiseStage>,
    lift: Option<GainStage>,
    output_gain: Option<GainStage>,
    dumps: Option<StageDumps>,
}

impl MicChain {
    pub fn new(processing: ApmConfig, ml_noise: bool, dumps: Option<StageDumps>) -> MicChain {
        if ml_noise {
            crate::noise_stage::prepare();
        }
        MicChain {
            high_pass: processing.high_pass_filter.then(VoiceHighPass::new),
            front: processing.front_module(),
            ml_noise: ml_noise.then(NoiseStage::new),
            lift: (processing.gain_controller && ml_noise).then(GainStage::lift),
            output_gain: processing.gain_controller.then(GainStage::output),
            dumps,
        }
    }

    pub fn process_far_end(&mut self, frame: &mut [i16]) {
        self.dump(0, frame);
        if let Some(front) = self.front.as_mut() {
            let _ = front.process_reverse_stream(frame, SAMPLE_RATE as i32, 1);
        }
    }

    pub fn process(&mut self, frame: &mut [i16]) {
        self.dump(1, frame);
        if let Some(high_pass) = self.high_pass.as_mut() {
            high_pass.process(frame);
        }
        if let Some(front) = self.front.as_mut() {
            let _ = front.process_stream(frame, SAMPLE_RATE as i32, 1);
        }
        self.dump(2, frame);
        if let Some(lift) = self.lift.as_mut() {
            lift.process(frame);
        }
        self.dump(3, frame);
        if let Some(noise_stage) = self.ml_noise.as_mut() {
            noise_stage.process_pcm(0, frame);
        }
        if let Some(output_gain) = self.output_gain.as_mut() {
            output_gain.process(frame);
        }
        self.dump(4, frame);
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
