use calling::{CallHealth, HealthSample, MediaPath, NetworkLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthBanner {
    Network,
    Cpu,
}

#[derive(Debug, Clone)]
pub struct HealthState {
    pub latest: Option<CallHealth>,
    pub incoming_video: bool,
    network_closed: bool,
    cpu_closed: bool,
}

impl Default for HealthState {
    fn default() -> Self {
        HealthState {
            latest: None,
            incoming_video: true,
            network_closed: false,
            cpu_closed: false,
        }
    }
}

impl HealthState {
    pub fn apply(&mut self, health: CallHealth) {
        if health.level == NetworkLevel::Good {
            self.network_closed = false;
        }
        if !health.cpu_limited {
            self.cpu_closed = false;
        }
        self.latest = Some(health);
    }

    pub fn level(&self) -> NetworkLevel {
        self.latest
            .as_ref()
            .map_or(NetworkLevel::Good, |health| health.level)
    }

    pub fn banner(&self, effects_on: bool) -> Option<HealthBanner> {
        let health = self.latest.as_ref()?;
        if health.level != NetworkLevel::Good && self.incoming_video && !self.network_closed {
            Some(HealthBanner::Network)
        } else if health.cpu_limited && effects_on && !self.cpu_closed {
            Some(HealthBanner::Cpu)
        } else {
            None
        }
    }

    pub fn close_banner(&mut self, banner: HealthBanner) {
        match banner {
            HealthBanner::Network => self.network_closed = true,
            HealthBanner::Cpu => self.cpu_closed = true,
        }
    }
}

pub fn level_text(level: NetworkLevel) -> &'static str {
    match level {
        NetworkLevel::Good => "Good",
        NetworkLevel::Poor => "Poor",
        NetworkLevel::Bad => "Bad",
    }
}

fn rate_text(kilobits_per_second: f64) -> String {
    if kilobits_per_second >= 1000. {
        format!("{:.1} Mbps", kilobits_per_second / 1000.)
    } else {
        format!("{kilobits_per_second:.0} kbps")
    }
}

pub fn network_rows(sample: &HealthSample) -> Vec<(&'static str, String)> {
    vec![
        ("Packet loss", format!("{:.1}%", sample.loss_percent)),
        ("Round trip", format!("{:.0} ms", sample.round_trip_ms)),
        ("Jitter", format!("{:.0} ms", sample.jitter_ms)),
        ("Sent", rate_text(sample.sent_kbps)),
        ("Received", rate_text(sample.received_kbps)),
    ]
}

pub fn transport_text(health: &CallHealth) -> String {
    match health.transport.srtp_cipher.as_str() {
        "" => "Transport: DTLS-SRTP".to_owned(),
        cipher => format!("Transport: DTLS-SRTP ({cipher})"),
    }
}

pub fn media_text(health: &CallHealth, peer_name: &str) -> Option<String> {
    match health.path {
        MediaPath::Direct => Some(format!("Media: direct to {peer_name}")),
        MediaPath::Mixer => Some("Media: via Microsoft media server".to_owned()),
        MediaPath::Pending => None,
    }
}

#[cfg(test)]
mod tests {
    use calling::TransportDetails;

    use super::*;

    fn health(level: NetworkLevel, cpu_limited: bool) -> CallHealth {
        CallHealth {
            level,
            cpu_limited,
            sample: HealthSample::default(),
            transport: TransportDetails {
                srtp_cipher: "AEAD_AES_128_GCM".into(),
                ..TransportDetails::default()
            },
            path: MediaPath::Direct,
        }
    }

    #[test]
    fn the_network_banner_shows_while_unstable_and_stays_closed_until_the_level_recovers() {
        let mut state = HealthState::default();
        assert_eq!(state.banner(true), None);
        state.apply(health(NetworkLevel::Poor, false));
        assert_eq!(state.banner(true), Some(HealthBanner::Network));
        state.close_banner(HealthBanner::Network);
        state.apply(health(NetworkLevel::Bad, false));
        assert_eq!(state.banner(true), None);
        state.apply(health(NetworkLevel::Good, false));
        state.apply(health(NetworkLevel::Poor, false));
        assert_eq!(state.banner(true), Some(HealthBanner::Network));
    }

    #[test]
    fn the_network_banner_is_pointless_once_incoming_video_is_off() {
        let mut state = HealthState::default();
        state.apply(health(NetworkLevel::Poor, false));
        state.incoming_video = false;
        assert_eq!(state.banner(true), None);
    }

    #[test]
    fn the_cpu_banner_needs_effects_to_turn_off_and_has_its_own_close() {
        let mut state = HealthState::default();
        state.apply(health(NetworkLevel::Good, true));
        assert_eq!(state.banner(false), None);
        assert_eq!(state.banner(true), Some(HealthBanner::Cpu));
        state.close_banner(HealthBanner::Cpu);
        state.apply(health(NetworkLevel::Good, true));
        assert_eq!(state.banner(true), None);
        state.apply(health(NetworkLevel::Good, false));
        state.apply(health(NetworkLevel::Good, true));
        assert_eq!(state.banner(true), Some(HealthBanner::Cpu));
    }

    #[test]
    fn the_popover_names_the_transport_and_the_media_path_without_claiming_end_to_end() {
        let mut found = health(NetworkLevel::Good, false);
        assert_eq!(transport_text(&found), "Transport: DTLS-SRTP (AEAD_AES_128_GCM)");
        assert_eq!(media_text(&found, "Mara").as_deref(), Some("Media: direct to Mara"));
        found.path = MediaPath::Mixer;
        assert_eq!(
            media_text(&found, "Mara").as_deref(),
            Some("Media: via Microsoft media server")
        );
        found.path = MediaPath::Pending;
        assert_eq!(media_text(&found, "Mara"), None);
    }

    #[test]
    fn rates_switch_to_megabits_from_a_thousand() {
        let sample = HealthSample {
            sent_kbps: 420.4,
            received_kbps: 1180.,
            ..HealthSample::default()
        };
        let rows = network_rows(&sample);
        assert_eq!(rows[3].1, "420 kbps");
        assert_eq!(rows[4].1, "1.2 Mbps");
    }
}
