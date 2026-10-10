use std::f32::consts::PI;

pub const SAMPLE_RATE: u32 = 44_100;
const LOW_HZ: f32 = 784.;
const HIGH_HZ: f32 = 988.;
const BEEP_SECONDS: f32 = 0.18;
const FADE_SECONDS: f32 = 0.015;
const AMPLITUDE: f32 = 0.3;
const BEEPS: [f32; 4] = [LOW_HZ, HIGH_HZ, LOW_HZ, HIGH_HZ];
const CYCLE_SECONDS: f32 = 2.4;

pub fn ring_cycle(sample_rate: u32) -> Vec<f32> {
    let total = (CYCLE_SECONDS * sample_rate as f32) as usize;
    let beep = (BEEP_SECONDS * sample_rate as f32) as usize;
    let fade = (FADE_SECONDS * sample_rate as f32) as usize;
    let mut samples = vec![0.; total];
    for (index, frequency) in BEEPS.iter().enumerate() {
        let start = index * beep;
        for position in 0..beep {
            let envelope = (position.min(beep - 1 - position) as f32 / fade as f32).min(1.);
            let phase = 2. * PI * frequency * position as f32 / sample_rate as f32;
            samples[start + position] = AMPLITUDE * envelope * phase.sin();
        }
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_lasts_two_and_a_half_seconds_at_most_and_stays_quiet() {
        let samples = ring_cycle(SAMPLE_RATE);
        assert_eq!(samples.len(), (CYCLE_SECONDS * SAMPLE_RATE as f32) as usize);
        assert!(samples.iter().all(|sample| sample.abs() <= AMPLITUDE + f32::EPSILON));
        assert!(samples.iter().any(|sample| sample.abs() > AMPLITUDE * 0.9));
    }

    #[test]
    fn beeps_fade_in_and_out_without_clicks() {
        let samples = ring_cycle(SAMPLE_RATE);
        assert_eq!(samples[0], 0.);
        let beep = (BEEP_SECONDS * SAMPLE_RATE as f32) as usize;
        assert!(samples[1].abs() < 0.05);
        assert!(samples[beep - 1].abs() < 0.05);
    }

    #[test]
    fn the_cycle_ends_in_silence_so_it_can_repeat() {
        let samples = ring_cycle(SAMPLE_RATE);
        let beeps_end = (BEEP_SECONDS * BEEPS.len() as f32 * SAMPLE_RATE as f32) as usize;
        assert!(samples[beeps_end..].iter().all(|sample| *sample == 0.));
        assert!(beeps_end < samples.len());
    }

    #[test]
    fn the_cycle_is_the_same_every_time() {
        assert_eq!(ring_cycle(SAMPLE_RATE), ring_cycle(SAMPLE_RATE));
    }
}
