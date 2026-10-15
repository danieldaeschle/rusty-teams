#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicEffect {
    pub id: u128,
    pub enabled: bool,
    pub can_set: bool,
}

impl MicEffect {
    pub fn name(&self) -> String {
        effect_name(self.id)
    }

    /// The state to switch to, when it differs from the current one and Windows lets us change it.
    pub fn change(&self) -> Option<bool> {
        wanted_state(self.id).filter(|wanted| self.can_set && *wanted != self.enabled)
    }
}

const EFFECT_SUFFIX: u128 = 0x8211_11e2_8c70_2c27_d7f0_01fa;
const EFFECT_NAMES: [(u32, &str); 19] = [
    (0x6f64adbe, "acoustic echo cancellation"),
    (0x6f64adbf, "noise suppression"),
    (0x6f64adc0, "automatic gain control"),
    (0x6f64adc1, "beamforming"),
    (0x6f64adc2, "constant tone removal"),
    (0x6f64adc3, "equalizer"),
    (0x6f64adc4, "loudness equalizer"),
    (0x6f64adc5, "bass boost"),
    (0x6f64adc6, "virtual surround"),
    (0x6f64adc7, "virtual headphones"),
    (0x6f64adc8, "speaker fill"),
    (0x6f64adc9, "room correction"),
    (0x6f64adca, "bass management"),
    (0x6f64adcb, "environmental effects"),
    (0x6f64adcc, "speaker protection"),
    (0x6f64adcd, "speaker compensation"),
    (0x6f64adce, "dynamic range compression"),
    (0x6f64adcf, "far field beamforming"),
    (0x6f64add0, "deep noise suppression"),
];

const ECHO_CANCELLATION: u32 = 0x6f64adbe;
const NOISE_SUPPRESSION: u32 = 0x6f64adbf;
const GAIN_CONTROL: u32 = 0x6f64adc0;
const BEAMFORMING: u32 = 0x6f64adc1;
const RANGE_COMPRESSION: u32 = 0x6f64adce;
const DEEP_NOISE_SUPPRESSION: u32 = 0x6f64add0;

fn known_prefix(guid: u128) -> Option<u32> {
    (guid & ((1u128 << 96) - 1) == EFFECT_SUFFIX).then_some((guid >> 96) as u32)
}

/// DeepFilterNet and AGC2 replace the Windows noise, gain and compression stages, stacking them muffles speech.
/// The Windows echo canceller stays: it has the exact speaker signal; AEC3 after it removes the rest before AGC2 lifts it.
pub fn wanted_state(guid: u128) -> Option<bool> {
    match known_prefix(guid)? {
        NOISE_SUPPRESSION | DEEP_NOISE_SUPPRESSION | GAIN_CONTROL | RANGE_COMPRESSION => Some(false),
        BEAMFORMING => Some(true),
        ECHO_CANCELLATION => None,
        _ => None,
    }
}

pub fn effect_name(guid: u128) -> String {
    known_prefix(guid)
        .and_then(|prefix| EFFECT_NAMES.iter().find(|(known, _)| *known == prefix))
        .map_or_else(|| format!("{guid:032x}"), |(_, name)| (*name).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_chain_replaces_windows_noise_gain_and_compression_but_keeps_its_echo_canceller() {
        let guid = |prefix: u32| (u128::from(prefix) << 96) | EFFECT_SUFFIX;
        assert_eq!(wanted_state(guid(NOISE_SUPPRESSION)), Some(false));
        assert_eq!(wanted_state(guid(DEEP_NOISE_SUPPRESSION)), Some(false));
        assert_eq!(wanted_state(guid(GAIN_CONTROL)), Some(false));
        assert_eq!(wanted_state(guid(RANGE_COMPRESSION)), Some(false));
        assert_eq!(wanted_state(guid(BEAMFORMING)), Some(true));
        assert_eq!(wanted_state(guid(ECHO_CANCELLATION)), None);
        assert_eq!(wanted_state(0x1234), None);
        let echo = MicEffect { id: guid(ECHO_CANCELLATION), enabled: true, can_set: true };
        assert!(echo.change().is_none());
        let deep_noise = MicEffect { id: guid(DEEP_NOISE_SUPPRESSION), enabled: true, can_set: true };
        assert_eq!(deep_noise.change(), Some(false));
        assert_eq!(MicEffect { can_set: false, ..deep_noise }.change(), None);
        assert_eq!(MicEffect { enabled: false, ..deep_noise }.change(), None);
    }

    #[test]
    fn known_effect_guids_get_their_names() {
        assert_eq!(effect_name(0x6f64adbe_8211_11e2_8c70_2c27d7f001fa), "acoustic echo cancellation");
        assert_eq!(effect_name(0x6f64add0_8211_11e2_8c70_2c27d7f001fa), "deep noise suppression");
        assert_eq!(effect_name(0x1234), "00000000000000000000000000001234");
    }
}
