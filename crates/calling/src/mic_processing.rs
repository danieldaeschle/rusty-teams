use libwebrtc::native::apm::AudioProcessingModule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseSuppression {
    #[default]
    Auto,
    Low,
    Off,
}

impl NoiseSuppression {
    pub const ALL: [NoiseSuppression; 3] = [NoiseSuppression::Auto, NoiseSuppression::Low, NoiseSuppression::Off];

    pub fn label(self) -> &'static str {
        match self {
            NoiseSuppression::Auto => "Auto",
            NoiseSuppression::Low => "Low",
            NoiseSuppression::Off => "Off",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            NoiseSuppression::Auto => "auto",
            NoiseSuppression::Low => "low",
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

    pub fn front_module(self) -> Option<AudioProcessingModule> {
        (self.echo_canceller || self.noise_suppression)
            .then(|| AudioProcessingModule::new(self.echo_canceller, false, false, self.noise_suppression))
    }

    pub fn gain_module(self) -> Option<AudioProcessingModule> {
        self.gain_controller.then(|| AudioProcessingModule::new(false, true, false, false))
    }
}

impl MicSettings {
    pub fn apm_config(self) -> ApmConfig {
        if self.music_mode {
            return ApmConfig { echo_canceller: true, gain_controller: false, high_pass_filter: false, noise_suppression: false };
        }
        let on = self.noise_suppression != NoiseSuppression::Off;
        let apm_noise_suppression = self.noise_suppression == NoiseSuppression::Low;
        ApmConfig { echo_canceller: on, gain_controller: on, high_pass_filter: on, noise_suppression: apm_noise_suppression }
    }

    pub fn uses_device_track(self) -> bool {
        self.uses_device_track_on(cfg!(windows))
    }

    pub fn uses_device_track_on(self, windows: bool) -> bool {
        !windows && !self.music_mode && self.noise_suppression == NoiseSuppression::Low
    }

    pub fn ml_noise_suppression(self) -> bool {
        !self.music_mode && self.noise_suppression == NoiseSuppression::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_leaves_noise_to_the_ml_stage_and_keeps_the_rest_of_the_apm() {
        let settings = MicSettings::default();
        assert!(!settings.uses_device_track_on(false));
        assert!(settings.ml_noise_suppression());
        assert_eq!(
            settings.apm_config(),
            ApmConfig { echo_canceller: true, gain_controller: true, high_pass_filter: true, noise_suppression: false }
        );
    }

    #[test]
    fn low_keeps_the_device_track_with_the_apm_noise_suppression() {
        let settings = MicSettings { noise_suppression: NoiseSuppression::Low, music_mode: false };
        assert!(settings.uses_device_track_on(false));
        assert!(!settings.ml_noise_suppression());
        assert_eq!(
            settings.apm_config(),
            ApmConfig { echo_canceller: true, gain_controller: true, high_pass_filter: true, noise_suppression: true }
        );
    }

    #[test]
    fn gain_runs_in_its_own_module_after_the_front_stages() {
        let config = MicSettings::default().apm_config();
        assert!(config.front_module().is_some());
        assert!(config.gain_module().is_some());
        let music = MicSettings { noise_suppression: NoiseSuppression::Auto, music_mode: true }.apm_config();
        assert!(music.front_module().is_some());
        assert!(music.gain_module().is_none());
    }

    #[test]
    fn only_auto_outside_music_mode_runs_the_ml_stage() {
        assert!(MicSettings::default().ml_noise_suppression());
        assert!(!MicSettings { noise_suppression: NoiseSuppression::Off, music_mode: false }.ml_noise_suppression());
        assert!(!MicSettings { noise_suppression: NoiseSuppression::Auto, music_mode: true }.ml_noise_suppression());
    }

    #[test]
    fn windows_never_uses_the_device_track() {
        for noise_suppression in NoiseSuppression::ALL {
            for music_mode in [false, true] {
                let settings = MicSettings { noise_suppression, music_mode };
                assert!(!settings.uses_device_track_on(true));
                assert_eq!(settings.uses_device_track_on(false), !music_mode && noise_suppression == NoiseSuppression::Low);
            }
        }
    }

    #[test]
    fn off_leaves_the_device_track_and_processes_nothing() {
        let settings = MicSettings { noise_suppression: NoiseSuppression::Off, music_mode: false };
        assert!(!settings.uses_device_track());
        assert!(settings.apm_config().is_passthrough());
        assert!(settings.apm_config().front_module().is_none());
        assert!(settings.apm_config().gain_module().is_none());
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
        assert_eq!(NoiseSuppression::Low.key(), "low");
        assert_eq!(NoiseSuppression::Low.label(), "Low");
    }
}
