use std::f32::consts::PI;

pub const SAMPLE_RATE: u32 = 44_100;
const FADE_SECONDS: f32 = 0.015;
const AMPLITUDE: f32 = 0.3;
const LOW_HZ: f32 = 784.;
const HIGH_HZ: f32 = 988.;
const BEEP_SECONDS: f32 = 0.18;
const BEEPS: [f32; 4] = [LOW_HZ, HIGH_HZ, LOW_HZ, HIGH_HZ];
const CYCLE_SECONDS: f32 = 2.4;
const CHIME_NOTES: [f32; 4] = [523.25, 659.25, 783.99, 1046.5];
const CHIME_NOTE_SECONDS: f32 = 0.22;
const CHIME_CYCLE_SECONDS: f32 = 2.6;
const PULSE_NOTES: [f32; 3] = [587.33, 587.33, 587.33];
const PULSE_NOTE_SECONDS: f32 = 0.26;
const PULSE_CYCLE_SECONDS: f32 = 2.2;
const MEETING_NOTES: [f32; 3] = [659.25, 783.99, 1046.5];
const MEETING_NOTE_SECONDS: f32 = 0.26;
const RINGBACK_HZ: f32 = 425.;
const RINGBACK_ON_SECONDS: f32 = 1.;
const RINGBACK_CYCLE_SECONDS: f32 = 5.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ringtone {
    #[default]
    Default,
    Chime,
    Pulse,
}

impl Ringtone {
    pub const ALL: [Ringtone; 3] = [Ringtone::Default, Ringtone::Chime, Ringtone::Pulse];

    pub fn label(self) -> &'static str {
        match self {
            Ringtone::Default => "Default",
            Ringtone::Chime => "Chime",
            Ringtone::Pulse => "Pulse",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Ringtone::Default => "default",
            Ringtone::Chime => "chime",
            Ringtone::Pulse => "pulse",
        }
    }

    pub fn from_key(key: &str) -> Option<Ringtone> {
        Ringtone::ALL
            .into_iter()
            .find(|ringtone| ringtone.key() == key)
    }

    pub fn cycle(self, sample_rate: u32) -> Vec<f32> {
        match self {
            Ringtone::Default => ring_cycle(sample_rate),
            Ringtone::Chime => tone_cycle(
                sample_rate,
                &CHIME_NOTES,
                CHIME_NOTE_SECONDS,
                CHIME_CYCLE_SECONDS,
            ),
            Ringtone::Pulse => tone_cycle(
                sample_rate,
                &PULSE_NOTES,
                PULSE_NOTE_SECONDS,
                PULSE_CYCLE_SECONDS,
            ),
        }
    }
}

pub fn ring_cycle(sample_rate: u32) -> Vec<f32> {
    tone_cycle(sample_rate, &BEEPS, BEEP_SECONDS, CYCLE_SECONDS)
}

pub fn meeting_chime(sample_rate: u32) -> Vec<f32> {
    let seconds = MEETING_NOTE_SECONDS * MEETING_NOTES.len() as f32;
    tone_cycle(sample_rate, &MEETING_NOTES, MEETING_NOTE_SECONDS, seconds)
}

pub fn ringback_cycle(sample_rate: u32) -> Vec<f32> {
    tone_cycle(
        sample_rate,
        &[RINGBACK_HZ],
        RINGBACK_ON_SECONDS,
        RINGBACK_CYCLE_SECONDS,
    )
}

fn tone_cycle(
    sample_rate: u32,
    frequencies: &[f32],
    tone_seconds: f32,
    cycle_seconds: f32,
) -> Vec<f32> {
    let total = (cycle_seconds * sample_rate as f32) as usize;
    let tone = (tone_seconds * sample_rate as f32) as usize;
    let fade = (FADE_SECONDS * sample_rate as f32) as usize;
    let mut samples = vec![0.; total];
    for (index, frequency) in frequencies.iter().enumerate() {
        let start = index * tone;
        for position in 0..tone {
            let envelope = (position.min(tone - 1 - position) as f32 / fade as f32).min(1.);
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
        assert!(
            samples
                .iter()
                .all(|sample| sample.abs() <= AMPLITUDE + f32::EPSILON)
        );
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

    #[test]
    fn the_ringback_is_one_second_of_425_hz_then_four_seconds_of_silence() {
        let samples = ringback_cycle(SAMPLE_RATE);
        let rate = SAMPLE_RATE as usize;
        assert_eq!(samples.len(), 5 * rate);
        assert!(
            samples[..rate]
                .iter()
                .any(|sample| sample.abs() > AMPLITUDE * 0.9)
        );
        assert!(samples[rate..].iter().all(|sample| *sample == 0.));
        let crossings = samples[..rate]
            .windows(2)
            .filter(|pair| pair[0] < 0. && pair[1] >= 0.)
            .count();
        assert!((424..=426).contains(&crossings));
    }

    #[test]
    fn every_ringtone_is_quiet_ends_in_silence_and_differs_from_the_others() {
        for ringtone in Ringtone::ALL {
            let samples = ringtone.cycle(SAMPLE_RATE);
            assert!(
                samples
                    .iter()
                    .all(|sample| sample.abs() <= AMPLITUDE + f32::EPSILON)
            );
            assert!(samples.iter().any(|sample| sample.abs() > AMPLITUDE * 0.9));
            assert_eq!(samples.last().copied(), Some(0.));
        }
        assert_ne!(
            Ringtone::Default.cycle(SAMPLE_RATE),
            Ringtone::Chime.cycle(SAMPLE_RATE)
        );
        assert_ne!(
            Ringtone::Chime.cycle(SAMPLE_RATE),
            Ringtone::Pulse.cycle(SAMPLE_RATE)
        );
    }

    #[test]
    fn the_meeting_chime_is_short_quiet_and_audible() {
        let samples = meeting_chime(SAMPLE_RATE);
        assert!(samples.len() <= SAMPLE_RATE as usize);
        assert!(
            samples
                .iter()
                .all(|sample| sample.abs() <= AMPLITUDE + f32::EPSILON)
        );
        assert!(samples.iter().any(|sample| sample.abs() > AMPLITUDE * 0.9));
    }

    #[test]
    fn ringtone_keys_round_trip() {
        for ringtone in Ringtone::ALL {
            assert_eq!(Ringtone::from_key(ringtone.key()), Some(ringtone));
        }
        assert_eq!(Ringtone::from_key("nope"), None);
    }
}
