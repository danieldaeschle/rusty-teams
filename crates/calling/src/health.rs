use std::collections::VecDeque;
use std::time::{Duration, Instant};

use libwebrtc::stats::{QualityLimitationReason, RtcStats};

use crate::renegotiation::MediaPath;

const SUSTAIN: Duration = Duration::from_secs(5);
const RECOVERY: Duration = Duration::from_secs(10);
const LIMIT_WINDOW: Duration = Duration::from_secs(10);
const LIMIT_SHARE: f32 = 0.5;
const POOR_LOSS_PERCENT: f64 = 5.;
const POOR_ROUND_TRIP_MS: f64 = 400.;
const POOR_JITTER_MS: f64 = 30.;
const BAD_LOSS_PERCENT: f64 = 10.;
const BAD_ROUND_TRIP_MS: f64 = 500.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NetworkLevel {
    Good,
    Poor,
    Bad,
}

impl NetworkLevel {
    fn better(self) -> Self {
        match self {
            NetworkLevel::Bad => NetworkLevel::Poor,
            _ => NetworkLevel::Good,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Limitation {
    #[default]
    None,
    Cpu,
    Bandwidth,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HealthSample {
    pub loss_percent: f64,
    pub round_trip_ms: f64,
    pub jitter_ms: f64,
    pub sent_kbps: f64,
    pub received_kbps: f64,
    pub limitation: Limitation,
}

impl HealthSample {
    fn is_bad(&self) -> bool {
        self.loss_percent > BAD_LOSS_PERCENT || self.round_trip_ms > BAD_ROUND_TRIP_MS
    }

    fn is_poor(&self) -> bool {
        self.is_bad()
            || self.loss_percent > POOR_LOSS_PERCENT
            || self.round_trip_ms > POOR_ROUND_TRIP_MS
            || self.jitter_ms > POOR_JITTER_MS
    }

    fn instant_level(&self) -> NetworkLevel {
        if self.is_bad() {
            NetworkLevel::Bad
        } else if self.is_poor() {
            NetworkLevel::Poor
        } else {
            NetworkLevel::Good
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransportDetails {
    pub dtls_state: String,
    pub tls_version: String,
    pub dtls_cipher: String,
    pub srtp_cipher: String,
    pub local_fingerprint: String,
    pub remote_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallHealth {
    pub level: NetworkLevel,
    pub cpu_limited: bool,
    pub sample: HealthSample,
    pub transport: TransportDetails,
    pub path: MediaPath,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    pub level: NetworkLevel,
    pub cpu_limited: bool,
}

pub struct QualityTracker {
    level: NetworkLevel,
    poor_since: Option<Instant>,
    bad_since: Option<Instant>,
    recovering_since: Option<Instant>,
    limits: VecDeque<(Instant, Limitation)>,
}

impl Default for QualityTracker {
    fn default() -> Self {
        QualityTracker {
            level: NetworkLevel::Good,
            poor_since: None,
            bad_since: None,
            recovering_since: None,
            limits: VecDeque::new(),
        }
    }
}

fn run_since(since: &mut Option<Instant>, holds: bool, now: Instant) -> bool {
    if !holds {
        *since = None;
        return false;
    }
    now.duration_since(*since.get_or_insert(now)) >= SUSTAIN
}

impl QualityTracker {
    pub fn observe(&mut self, now: Instant, sample: &HealthSample) -> Quality {
        self.limits.push_back((now, sample.limitation));
        while self.limits.front().is_some_and(|(at, _)| now.duration_since(*at) > LIMIT_WINDOW) {
            self.limits.pop_front();
        }
        let bad = run_since(&mut self.bad_since, sample.is_bad(), now);
        let poor = run_since(&mut self.poor_since, sample.is_poor(), now);
        let target = if bad {
            NetworkLevel::Bad
        } else if poor {
            NetworkLevel::Poor
        } else {
            NetworkLevel::Good
        };
        if target > self.level {
            self.level = target;
            self.recovering_since = None;
        } else if sample.instant_level() < self.level {
            let since = *self.recovering_since.get_or_insert(now);
            if now.duration_since(since) >= RECOVERY {
                self.level = self.level.better();
                self.recovering_since = None;
            }
        } else {
            self.recovering_since = None;
        }
        Quality { level: self.level, cpu_limited: self.share_of(Limitation::Cpu, now) > LIMIT_SHARE }
    }

    fn share_of(&self, limitation: Limitation, now: Instant) -> f32 {
        let spans_window = self.limits.front().is_some_and(|(at, _)| now.duration_since(*at) >= SUSTAIN);
        if !spans_window {
            return 0.;
        }
        let matching = self.limits.iter().filter(|(_, found)| *found == limitation).count();
        matching as f32 / self.limits.len() as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HealthCounters {
    pub packets_received: u64,
    pub packets_lost: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub jitter_ms: f64,
    pub round_trip_ms: f64,
    pub outbound_loss_percent: f64,
    pub limitation: Limitation,
}

#[derive(Default)]
pub struct HealthSampler {
    previous: Option<(Instant, HealthCounters)>,
}

impl HealthSampler {
    pub fn sample(&mut self, now: Instant, counters: HealthCounters) -> Option<HealthSample> {
        let (then, before) = self.previous.replace((now, counters))?;
        let seconds = now.duration_since(then).as_secs_f64();
        if seconds <= 0. {
            return None;
        }
        let received = counters.packets_received.saturating_sub(before.packets_received);
        let lost = counters.packets_lost.saturating_sub(before.packets_lost);
        let inbound_loss_percent = if received + lost == 0 { 0. } else { lost as f64 * 100. / (received + lost) as f64 };
        let kbps = |after: u64, before: u64| after.saturating_sub(before) as f64 * 8. / seconds / 1000.;
        Some(HealthSample {
            loss_percent: inbound_loss_percent.max(counters.outbound_loss_percent),
            round_trip_ms: counters.round_trip_ms,
            jitter_ms: counters.jitter_ms,
            sent_kbps: kbps(counters.bytes_sent, before.bytes_sent),
            received_kbps: kbps(counters.bytes_received, before.bytes_received),
            limitation: counters.limitation,
        })
    }
}

fn limitation_of(reason: QualityLimitationReason) -> Limitation {
    match reason {
        QualityLimitationReason::Cpu => Limitation::Cpu,
        QualityLimitationReason::Bandwidth => Limitation::Bandwidth,
        _ => Limitation::None,
    }
}

pub fn health_from_stats(stats: &[RtcStats]) -> (HealthCounters, TransportDetails) {
    let mut counters = HealthCounters::default();
    let mut transport = TransportDetails::default();
    let mut certificate_ids = (String::new(), String::new());
    let mut selected_pair_id = String::new();
    let mut remote_round_trip_ms = 0f64;
    for entry in stats {
        match entry {
            RtcStats::InboundRtp(inbound) => {
                counters.packets_received += inbound.received.packets_received;
                counters.packets_lost += inbound.received.packets_lost.max(0) as u64;
                counters.bytes_received += inbound.inbound.bytes_received;
                if inbound.stream.kind == "audio" {
                    counters.jitter_ms = counters.jitter_ms.max(inbound.received.jitter * 1000.);
                }
            }
            RtcStats::OutboundRtp(outbound) => {
                counters.bytes_sent += outbound.sent.bytes_sent;
                if outbound.stream.kind == "video" && outbound.outbound.active {
                    let limitation = limitation_of(outbound.outbound.quality_limitation_reason);
                    counters.limitation = counters.limitation.max_with(limitation);
                }
            }
            RtcStats::RemoteInboundRtp(remote) => {
                counters.outbound_loss_percent = counters.outbound_loss_percent.max(remote.remote_inbound.fraction_lost * 100.);
                remote_round_trip_ms = remote_round_trip_ms.max(remote.remote_inbound.round_trip_time * 1000.);
            }
            RtcStats::Transport(found) => {
                transport.dtls_state = found.transport.dtls_state.map(|state| format!("{state:?}")).unwrap_or_default();
                transport.tls_version = found.transport.tls_version.clone();
                transport.dtls_cipher = found.transport.dtls_cipher.clone();
                transport.srtp_cipher = found.transport.srtp_cipher.clone();
                certificate_ids = (found.transport.local_certificate_id.clone(), found.transport.remote_certificate_id.clone());
                selected_pair_id = found.transport.selected_candidate_pair_id.clone();
            }
            _ => {}
        }
    }
    let fingerprint = |certificate_id: &str| {
        stats
            .iter()
            .find_map(|entry| match entry {
                RtcStats::Certificate(certificate) if certificate.rtc.id == certificate_id => {
                    Some(format!("{} {}", certificate.certificate.fingerprint_algorithm, certificate.certificate.fingerprint))
                }
                _ => None,
            })
            .unwrap_or_default()
    };
    transport.local_fingerprint = fingerprint(&certificate_ids.0);
    transport.remote_fingerprint = fingerprint(&certificate_ids.1);
    let pair_round_trip_ms = stats.iter().find_map(|entry| match entry {
        RtcStats::CandidatePair(pair) if pair.rtc.id == selected_pair_id => Some(pair.candidate_pair.current_round_trip_time * 1000.),
        _ => None,
    });
    counters.round_trip_ms = pair_round_trip_ms.filter(|milliseconds| *milliseconds > 0.).unwrap_or(remote_round_trip_ms);
    (counters, transport)
}

impl Limitation {
    fn max_with(self, other: Limitation) -> Limitation {
        match (self, other) {
            (Limitation::Cpu, _) | (_, Limitation::Cpu) => Limitation::Cpu,
            (Limitation::Bandwidth, _) | (_, Limitation::Bandwidth) => Limitation::Bandwidth,
            _ => Limitation::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use libwebrtc::stats::dictionaries::{CandidatePairStats, CertificateStats, InboundRtpStreamStats, OutboundRtpStreamStats, ReceivedRtpStreamStats, RtcStats as RtcStatsBase, RtpStreamStats, SentRtpStreamStats, TransportStats as TransportDictionary};
    use libwebrtc::stats::{CandidatePairStats as PairEntry, CertificateStats as CertificateEntry, InboundRtpStats, OutboundRtpStats, TransportStats};

    use super::*;

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    fn lossy(loss_percent: f64) -> HealthSample {
        HealthSample { loss_percent, ..HealthSample::default() }
    }

    fn feed(tracker: &mut QualityTracker, start: Instant, from: u64, to: u64, sample: &HealthSample) -> Quality {
        let mut quality = Quality { level: NetworkLevel::Good, cpu_limited: false };
        for seconds in (from..=to).step_by(2) {
            quality = tracker.observe(at(start, seconds), sample);
        }
        quality
    }

    #[test]
    fn a_brief_spike_does_not_degrade_the_level() {
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        assert_eq!(feed(&mut tracker, start, 0, 2, &lossy(8.)).level, NetworkLevel::Good);
        assert_eq!(feed(&mut tracker, start, 4, 20, &lossy(0.)).level, NetworkLevel::Good);
    }

    #[test]
    fn poor_needs_five_seconds_of_loss_over_five_percent() {
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        assert_eq!(feed(&mut tracker, start, 0, 4, &lossy(6.)).level, NetworkLevel::Good);
        assert_eq!(feed(&mut tracker, start, 6, 6, &lossy(6.)).level, NetworkLevel::Poor);
    }

    #[test]
    fn round_trip_and_jitter_also_make_it_poor() {
        for sample in [HealthSample { round_trip_ms: 450., ..Default::default() }, HealthSample { jitter_ms: 35., ..Default::default() }] {
            let (mut tracker, start) = (QualityTracker::default(), Instant::now());
            assert_eq!(feed(&mut tracker, start, 0, 6, &sample).level, NetworkLevel::Poor);
        }
    }

    #[test]
    fn bad_needs_five_seconds_over_ten_percent_loss_or_500_ms() {
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        assert_eq!(feed(&mut tracker, start, 0, 6, &lossy(12.)).level, NetworkLevel::Bad);
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        let slow = HealthSample { round_trip_ms: 600., ..Default::default() };
        assert_eq!(feed(&mut tracker, start, 0, 6, &slow).level, NetworkLevel::Bad);
    }

    #[test]
    fn the_level_drops_fast_and_recovers_one_step_per_ten_seconds() {
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        assert_eq!(feed(&mut tracker, start, 0, 6, &lossy(12.)).level, NetworkLevel::Bad);
        assert_eq!(feed(&mut tracker, start, 8, 16, &lossy(0.)).level, NetworkLevel::Bad);
        assert_eq!(feed(&mut tracker, start, 18, 18, &lossy(0.)).level, NetworkLevel::Poor);
        assert_eq!(feed(&mut tracker, start, 20, 28, &lossy(0.)).level, NetworkLevel::Poor);
        assert_eq!(feed(&mut tracker, start, 30, 30, &lossy(0.)).level, NetworkLevel::Good);
    }

    #[test]
    fn a_relapse_restarts_the_recovery_timer() {
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        feed(&mut tracker, start, 0, 6, &lossy(7.));
        feed(&mut tracker, start, 8, 16, &lossy(0.));
        feed(&mut tracker, start, 18, 18, &lossy(7.));
        assert_eq!(feed(&mut tracker, start, 20, 28, &lossy(0.)).level, NetworkLevel::Poor);
        assert_eq!(feed(&mut tracker, start, 30, 30, &lossy(0.)).level, NetworkLevel::Good);
    }

    #[test]
    fn bandwidth_limits_alone_keep_it_good() {
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        let limited = HealthSample { limitation: Limitation::Bandwidth, ..Default::default() };
        assert_eq!(feed(&mut tracker, start, 0, 30, &limited).level, NetworkLevel::Good);
    }

    #[test]
    fn cpu_limits_raise_their_own_flag_without_touching_the_level() {
        let (mut tracker, start) = (QualityTracker::default(), Instant::now());
        let limited = HealthSample { limitation: Limitation::Cpu, ..Default::default() };
        let quality = feed(&mut tracker, start, 0, 6, &limited);
        assert!(quality.cpu_limited);
        assert_eq!(quality.level, NetworkLevel::Good);
        assert!(!feed(&mut tracker, start, 8, 20, &HealthSample::default()).cpu_limited);
    }

    #[test]
    fn the_sampler_turns_counter_deltas_into_loss_and_bitrate() {
        let (mut sampler, start) = (HealthSampler::default(), Instant::now());
        let first = HealthCounters { packets_received: 100, packets_lost: 0, bytes_sent: 1000, bytes_received: 2000, ..Default::default() };
        assert_eq!(sampler.sample(start, first), None);
        let second = HealthCounters { packets_received: 190, packets_lost: 10, bytes_sent: 51_000, bytes_received: 27_000, round_trip_ms: 80., jitter_ms: 4., ..first };
        let sample = sampler.sample(at(start, 2), second).unwrap();
        assert_eq!(sample.loss_percent, 10.);
        assert_eq!(sample.sent_kbps, 200.);
        assert_eq!(sample.received_kbps, 100.);
        assert_eq!((sample.round_trip_ms, sample.jitter_ms), (80., 4.));
    }

    #[test]
    fn the_sampler_reports_outbound_loss_from_the_remote_report() {
        let (mut sampler, start) = (HealthSampler::default(), Instant::now());
        sampler.sample(start, HealthCounters::default());
        let counters = HealthCounters { outbound_loss_percent: 7., ..Default::default() };
        assert_eq!(sampler.sample(at(start, 2), counters).unwrap().loss_percent, 7.);
    }

    #[test]
    fn stats_parse_into_counters_and_transport_details() {
        let stats = vec![
            RtcStats::InboundRtp(InboundRtpStats {
                stream: RtpStreamStats { kind: "audio".into(), ..Default::default() },
                received: ReceivedRtpStreamStats { packets_received: 50, packets_lost: 5, jitter: 0.012 },
                inbound: InboundRtpStreamStats { bytes_received: 4000, ..Default::default() },
                ..Default::default()
            }),
            RtcStats::OutboundRtp(OutboundRtpStats {
                stream: RtpStreamStats { kind: "video".into(), ..Default::default() },
                sent: SentRtpStreamStats { bytes_sent: 9000, ..Default::default() },
                outbound: OutboundRtpStreamStats { active: true, quality_limitation_reason: QualityLimitationReason::Cpu, ..Default::default() },
                ..Default::default()
            }),
            RtcStats::Transport(TransportStats {
                transport: TransportDictionary {
                    dtls_cipher: "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256".into(),
                    srtp_cipher: "AEAD_AES_128_GCM".into(),
                    tls_version: "FEFD".into(),
                    local_certificate_id: "CF1".into(),
                    remote_certificate_id: "CF2".into(),
                    selected_candidate_pair_id: "CP1".into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            RtcStats::CandidatePair(PairEntry {
                rtc: RtcStatsBase { id: "CP1".into(), ..Default::default() },
                candidate_pair: CandidatePairStats { current_round_trip_time: 0.045, ..Default::default() },
            }),
            RtcStats::Certificate(CertificateEntry {
                rtc: RtcStatsBase { id: "CF1".into(), ..Default::default() },
                certificate: CertificateStats { fingerprint: "AA:BB".into(), fingerprint_algorithm: "sha-256".into(), ..Default::default() },
            }),
        ];
        let (counters, transport) = health_from_stats(&stats);
        assert_eq!((counters.packets_received, counters.packets_lost), (50, 5));
        assert_eq!((counters.bytes_received, counters.bytes_sent), (4000, 9000));
        assert_eq!(counters.jitter_ms, 12.);
        assert_eq!(counters.round_trip_ms, 45.);
        assert_eq!(counters.limitation, Limitation::Cpu);
        assert_eq!(transport.srtp_cipher, "AEAD_AES_128_GCM");
        assert_eq!(transport.local_fingerprint, "sha-256 AA:BB");
        assert_eq!(transport.remote_fingerprint, "");
    }

    #[test]
    fn inactive_video_streams_do_not_count_as_limited() {
        let stats = vec![RtcStats::OutboundRtp(OutboundRtpStats {
            stream: RtpStreamStats { kind: "video".into(), ..Default::default() },
            outbound: OutboundRtpStreamStats { active: false, quality_limitation_reason: QualityLimitationReason::Bandwidth, ..Default::default() },
            ..Default::default()
        })];
        assert_eq!(health_from_stats(&stats).0.limitation, Limitation::None);
    }
}
