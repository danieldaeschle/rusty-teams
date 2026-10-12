use libwebrtc::native::apm::AudioProcessingModule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseSuppression {
    #[default]
    Auto,
    Off,
}

impl NoiseSuppression {
    pub const ALL: [NoiseSuppression; 2] = [NoiseSuppression::Auto, NoiseSuppression::Off];

    pub fn label(self) -> &'static str {
        match self {
            NoiseSuppression::Auto => "Auto",
            NoiseSuppression::Off => "Off",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            NoiseSuppression::Auto => "auto",
            NoiseSuppression::Off => "off",
        }
    }

    pub fn from_key(key: &str) -> Option<NoiseSuppression> {
        NoiseSuppression::ALL.into_iter().find(|level| level.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MicSettings {
    pub noise_suppression: NoiseSuppression,
    pub music_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApmConfig {
    pub echo_canceller: bool,
    pub gain_controller: bool,
    pub high_pass_filter: bool,
    pub noise_suppression: bool,
}

impl ApmConfig {
    pub fn is_passthrough(self) -> bool {
        self == ApmConfig { echo_canceller: false, gain_controller: false, high_pass_filter: false, noise_suppression: false }
    }

    pub fn module(self) -> Option<AudioProcessingModule> {
        (!self.is_passthrough()).then(|| {
            AudioProcessingModule::new(self.echo_canceller, self.gain_controller, self.high_pass_filter, self.noise_suppression)
        })
    }
}

impl MicSettings {
    pub fn apm_config(self) -> ApmConfig {
        if self.music_mode {
            return ApmConfig { echo_canceller: true, gain_controller: false, high_pass_filter: false, noise_suppression: false };
        }
        let on = self.noise_suppression == NoiseSuppression::Auto;
        ApmConfig { echo_canceller: on, gain_controller: on, high_pass_filter: on, noise_suppression: on }
    }

    pub fn uses_device_track(self) -> bool {
        self == MicSettings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_keeps_the_device_track_with_everything_on() {
        let settings = MicSettings::default();
        assert!(settings.uses_device_track());
        assert_eq!(
            settings.apm_config(),
            ApmConfig { echo_canceller: true, gain_controller: true, high_pass_filter: true, noise_suppression: true }
        );
    }

    #[test]
    fn off_leaves_the_device_track_and_processes_nothing() {
        let settings = MicSettings { noise_suppression: NoiseSuppression::Off, music_mode: false };
        assert!(!settings.uses_device_track());
        assert!(settings.apm_config().is_passthrough());
        assert!(settings.apm_config().module().is_none());
    }

    #[test]
    fn music_mode_keeps_only_echo_cancellation_whatever_the_noise_choice() {
        for noise_suppression in NoiseSuppression::ALL {
            let settings = MicSettings { noise_suppression, music_mode: true };
            assert!(!settings.uses_device_track());
            assert_eq!(
                settings.apm_config(),
                ApmConfig { echo_canceller: true, gain_controller: false, high_pass_filter: false, noise_suppression: false }
            );
        }
    }

    #[test]
    fn noise_levels_round_trip_through_their_keys() {
        for level in NoiseSuppression::ALL {
            assert_eq!(NoiseSuppression::from_key(level.key()), Some(level));
        }
        assert_eq!(NoiseSuppression::from_key("high"), None);
    }
}
