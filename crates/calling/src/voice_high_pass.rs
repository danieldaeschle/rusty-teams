use crate::audio::SAMPLE_RATE;

pub const CUTOFF_HZ: f32 = 120.0;
const BUTTERWORTH_FOURTH_ORDER_Q: [f32; 2] = [0.541_196_1, 1.306_563];

#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn high_pass(cutoff_hz: f32, quality: f32) -> Biquad {
        let omega = std::f32::consts::TAU * cutoff_hz / SAMPLE_RATE as f32;
        let alpha = omega.sin() / (2.0 * quality);
        let cosine = omega.cos();
        let a0 = 1.0 + alpha;
        Biquad {
            b0: (1.0 + cosine) / 2.0 / a0,
            b1: -(1.0 + cosine) / a0,
            b2: (1.0 + cosine) / 2.0 / a0,
            a1: -2.0 * cosine / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

/// Fourth-order Butterworth high-pass for speech; WebRTC's own high-pass only removes rumble below about 80 Hz.
pub struct VoiceHighPass {
    sections: [Biquad; 2],
}

impl VoiceHighPass {
    pub fn new() -> VoiceHighPass {
        VoiceHighPass { sections: BUTTERWORTH_FOURTH_ORDER_Q.map(|quality| Biquad::high_pass(CUTOFF_HZ, quality)) }
    }

    pub fn process(&mut self, frame: &mut [i16]) {
        for sample in frame {
            let filtered = self.sections.iter_mut().fold(f32::from(*sample), |value, section| section.process(value));
            *sample = filtered.round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
    }
}

impl Default for VoiceHighPass {
    fn default() -> Self {
        VoiceHighPass::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::SineGenerator;

    fn gain_db(frequency: f32) -> f32 {
        let mut filter = VoiceHighPass::new();
        let mut generator = SineGenerator::new(frequency, 0.5);
        let (mut input_energy, mut output_energy) = (0.0_f64, 0.0_f64);
        for frame_index in 0..200 {
            let input = generator.next_frame();
            let mut output = input.clone();
            filter.process(&mut output);
            if frame_index >= 100 {
                input_energy += input.iter().map(|sample| f64::from(*sample).powi(2)).sum::<f64>();
                output_energy += output.iter().map(|sample| f64::from(*sample).powi(2)).sum::<f64>();
            }
        }
        (10.0 * (output_energy / input_energy).log10()) as f32
    }

    #[test]
    fn speech_band_passes_and_rumble_is_cut() {
        assert!(gain_db(1000.0).abs() < 0.2, "1 kHz {}", gain_db(1000.0));
        assert!(gain_db(200.0) > -1.0, "200 Hz {}", gain_db(200.0));
        assert!((gain_db(CUTOFF_HZ) + 3.0).abs() < 0.5, "cutoff {}", gain_db(CUTOFF_HZ));
        assert!(gain_db(60.0) < -20.0, "60 Hz {}", gain_db(60.0));
    }
}
