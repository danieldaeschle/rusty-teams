use crate::audio::FRAME_SAMPLES;

pub fn mix_frames(microphone: &[i16], computer: &[i16], microphone_muted: bool) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|index| {
            let mic = if microphone_muted { 0 } else { i32::from(microphone.get(index).copied().unwrap_or(0)) };
            let sound = i32::from(computer.get(index).copied().unwrap_or(0));
            (mic + sound).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_sources_are_summed() {
        let mixed = mix_frames(&[100; FRAME_SAMPLES], &[-30; FRAME_SAMPLES], false);
        assert_eq!(mixed.len(), FRAME_SAMPLES);
        assert!(mixed.iter().all(|&sample| sample == 70));
    }

    #[test]
    fn the_sum_clips_instead_of_wrapping() {
        let loud = [i16::MAX - 10; FRAME_SAMPLES];
        assert!(mix_frames(&loud, &loud, false).iter().all(|&sample| sample == i16::MAX));
        let low = [i16::MIN + 10; FRAME_SAMPLES];
        assert!(mix_frames(&low, &low, false).iter().all(|&sample| sample == i16::MIN));
    }

    #[test]
    fn muting_drops_only_the_microphone() {
        let mixed = mix_frames(&[500; FRAME_SAMPLES], &[200; FRAME_SAMPLES], true);
        assert!(mixed.iter().all(|&sample| sample == 200));
    }

    #[test]
    fn a_short_source_counts_as_silence_for_the_rest() {
        let mixed = mix_frames(&[10; 4], &[1; FRAME_SAMPLES], false);
        assert_eq!(&mixed[..5], &[11, 11, 11, 11, 1]);
    }
}
