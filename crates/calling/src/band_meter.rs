use crate::audio::SAMPLE_RATE;

const SPLIT_HERTZ: f32 = 8000.;
const QUALITY: f32 = std::f32::consts::FRAC_1_SQRT_2;
const SILENCE_ENERGY: f64 = 1e6;

pub struct HighBandMeter {
    coefficients: [f32; 5],
    history: [f32; 4],
    total_energy: f64,
    high_energy: f64,
}

impl Default for HighBandMeter {
    fn default() -> Self {
        let omega = 2. * std::f32::consts::PI * SPLIT_HERTZ / SAMPLE_RATE as f32;
        let alpha = omega.sin() / (2. * QUALITY);
        let cosine = omega.cos();
        let a0 = 1. + alpha;
        let b0 = (1. + cosine) / 2. / a0;
        HighBandMeter {
            coefficients: [b0, -(1. + cosine) / a0, b0, -2. * cosine / a0, (1. - alpha) / a0],
            history: [0.; 4],
            total_energy: 0.,
            high_energy: 0.,
        }
    }
}

impl HighBandMeter {
    pub fn add(&mut self, frame: &[i16]) {
        let [b0, b1, b2, a1, a2] = self.coefficients;
        for &sample in frame {
            let input = f32::from(sample);
            let [x1, x2, y1, y2] = self.history;
            let output = b0 * input + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            self.history = [input, x1, output, y1];
            self.total_energy += f64::from(input * input);
            self.high_energy += f64::from(output * output);
        }
    }

    pub fn take_share(&mut self) -> Option<f64> {
        let share = (self.total_energy > SILENCE_ENERGY).then(|| self.high_energy / self.total_energy);
        self.total_energy = 0.;
        self.high_energy = 0.;
        share
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hertz: f32) -> Vec<i16> {
        (0..SAMPLE_RATE as usize)
            .map(|index| ((2. * std::f32::consts::PI * hertz * index as f32 / SAMPLE_RATE as f32).sin() * 8000.) as i16)
            .collect()
    }

    #[test]
    fn wideband_speech_has_nothing_above_eight_kilohertz() {
        let mut meter = HighBandMeter::default();
        meter.add(&sine(1000.));
        assert!(meter.take_share().unwrap() < 0.01);
    }

    #[test]
    fn fullband_content_shows_up_above_eight_kilohertz() {
        let mut meter = HighBandMeter::default();
        meter.add(&sine(14000.));
        assert!(meter.take_share().unwrap() > 0.9);
    }

    #[test]
    fn silence_reports_nothing() {
        let mut meter = HighBandMeter::default();
        meter.add(&[0; 4800]);
        assert_eq!(meter.take_share(), None);
    }
}
