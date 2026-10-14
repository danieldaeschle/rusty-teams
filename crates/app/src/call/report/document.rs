use std::collections::BTreeMap;

use calling::{EndKind, EndReason, MediaPath};
use serde::Serialize;

use super::markdown;
use super::scrub::Scrubber;
use super::series::{level_name, limitation_name};
use super::{CallRecorder, FeatureRecord, Outcome};
use crate::call::model::CallKind;

const SCHEMA: u32 = 1;
const FILE_STAMP: &str = "%Y%m%dT%H%M%SZ";
const READABLE_STAMP: &str = "%Y-%m-%d %H:%M:%S";
const MIN_SAMPLES_FOR_STALLS: u32 = 3;
const ESCALATION_LABEL: &str = "escalation";
const TRANSFER_LABEL: &str = "transfer";
const FAILURE_WORDS: [&str; 3] = ["failed", "rejected", "not understood"];
const FEATURE_FAILURE_LABELS: [(&str, &str); 11] = [
    ("raise hand failed", "hand"),
    ("lower hand failed", "hand"),
    ("lower all hands failed", "hand"),
    ("hold failed", "hold"),
    ("transfer failed", "transfer"),
    ("reaction failed", "reaction"),
    ("camera offer failed", "camera"),
    ("recording consent failed", "recording"),
    ("caption language failed", "captions"),
    ("caption start failed", "captions"),
    ("end meeting failed", "people action"),
];

pub struct Document {
    pub stem: String,
    pub json: String,
    pub markdown: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub app: AppInfo,
    pub call: CallInfo,
    pub media: MediaInfo,
    pub participants: ParticipantInfo,
    pub problems: Vec<String>,
    pub stats: StatsInfo,
    pub features: Vec<FeatureInfo>,
    pub notices: Vec<NoticeInfo>,
    pub states: Vec<StateInfo>,
    pub sdp: Vec<SdpInfo>,
    pub timeline: Vec<TimelineInfo>,
}

#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub build: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub demo: bool,
}

#[derive(Debug, Serialize)]
pub struct CallInfo {
    pub kind: &'static str,
    pub direction: &'static str,
    pub started_utc: String,
    pub connect_ms: Option<u128>,
    pub total_ms: Option<u128>,
    pub connected_ms: Option<u128>,
    pub end_reason: String,
    pub engine_ice_connected_ms: Option<u128>,
    pub engine_peer_connected_ms: Option<u128>,
}

#[derive(Debug, Serialize)]
pub struct MediaInfo {
    pub path: &'static str,
    pub path_changes: Vec<PathChange>,
    pub escalations: usize,
    pub escalation_failures: usize,
    pub selected_pair: Option<String>,
    pub tls_version: String,
    pub dtls_cipher: String,
    pub srtp_cipher: String,
}

#[derive(Debug, Serialize)]
pub struct PathChange {
    pub at_ms: u128,
    pub path: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ParticipantInfo {
    pub peak_present: usize,
    pub known: Vec<String>,
    pub captions_received: u32,
}

#[derive(Debug, Serialize)]
pub struct StatsInfo {
    pub samples: u32,
    pub metrics: BTreeMap<&'static str, Range>,
    pub audio_in_stalled_samples: u32,
    pub audio_out_stalled_samples: u32,
    pub health_samples: BTreeMap<&'static str, u32>,
    pub health_level_changes: Vec<PathChange>,
    pub quality_limitation_samples: BTreeMap<&'static str, u32>,
    pub cpu_limited_samples: u32,
}

#[derive(Debug, Serialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub last: f64,
}

#[derive(Debug, Serialize)]
pub struct FeatureInfo {
    pub at_ms: u128,
    pub feature: &'static str,
    pub detail: String,
    pub result: &'static str,
    pub confirmed_at_ms: Option<u128>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NoticeInfo {
    pub at_ms: u128,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct StateInfo {
    pub at_ms: u128,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct SdpInfo {
    pub engine_ms: u128,
    pub role: String,
    pub lines: Vec<SdpLineInfo>,
}

#[derive(Debug, Serialize)]
pub struct SdpLineInfo {
    pub kind: String,
    pub mid: String,
    pub direction: String,
    pub codecs: Vec<String>,
    pub payloads: usize,
    pub ssrcs: usize,
    pub rejected: bool,
}

#[derive(Debug, Serialize)]
pub struct TimelineInfo {
    pub engine_ms: u128,
    pub label: String,
    pub detail: String,
}

fn kind_label(kind: CallKind) -> &'static str {
    match kind {
        CallKind::Test => "echo",
        CallKind::Direct => "1:1",
        CallKind::Group => "group",
        CallKind::Meeting => "meeting",
    }
}

fn kind_slug(kind: CallKind) -> &'static str {
    match kind {
        CallKind::Test => "echo",
        CallKind::Direct => "1to1",
        CallKind::Group => "group",
        CallKind::Meeting => "meeting",
    }
}

fn path_name(path: MediaPath) -> &'static str {
    match path {
        MediaPath::Pending => "pending",
        MediaPath::Direct => "direct",
        MediaPath::Mixer => "mixer",
    }
}

fn end_text(reason: &EndReason) -> String {
    match reason {
        EndReason::LocalHangup => "local hangup".to_owned(),
        EndReason::Cancelled => "cancelled before the call connected".to_owned(),
        EndReason::Dropped => "connection dropped".to_owned(),
        EndReason::Failed(message) => format!("failed: {message}"),
        EndReason::Remote(EndKind::Declined) => "remote declined".to_owned(),
        EndReason::Remote(EndKind::NoAnswer) => "no answer".to_owned(),
        EndReason::Remote(EndKind::Unavailable) => "remote unavailable".to_owned(),
        EndReason::Remote(kind) => format!("remote ended ({kind:?})"),
    }
}

impl CallRecorder {
    pub fn document(&self) -> Document {
        let report = self.report();
        Document {
            stem: format!("{}-{}", self.started_at.format(FILE_STAMP), kind_slug(self.kind)),
            json: serde_json::to_string_pretty(&report).unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
            markdown: markdown::render(&report),
        }
    }

    fn scrubber(&self) -> Scrubber {
        let mut scrubber = Scrubber::new();
        scrubber.register_self(&self.me.mri, &self.me.display_name, &self.me.email);
        for (mri, name) in &self.people {
            scrubber.register_person(mri, name);
        }
        scrubber
    }

    fn report(&self) -> Report {
        let mut scrubber = self.scrubber();
        let diagnostics = self.diagnostics.clone().unwrap_or_default();
        let timeline: Vec<TimelineInfo> = diagnostics
            .timeline
            .iter()
            .map(|entry| TimelineInfo {
                engine_ms: entry.elapsed_ms,
                label: scrubber.scrub(&entry.label),
                detail: scrubber.scrub(&entry.detail),
            })
            .collect();
        let features = self.feature_infos(&mut scrubber, &diagnostics.timeline);
        let notices = self
            .notices
            .iter()
            .map(|(at_ms, text)| NoticeInfo { at_ms: *at_ms, text: scrubber.scrub(text) })
            .collect();
        let total_ms = self.ended.as_ref().map(|(at_ms, _)| *at_ms);
        let end_reason = self.ended.as_ref().map_or_else(|| "still running when saved".to_owned(), |(_, reason)| scrubber.scrub(&end_text(reason)));
        let escalations = timeline.iter().filter(|entry| entry.label == ESCALATION_LABEL && entry.detail.contains("new offer sent")).count();
        let escalation_failures = timeline.iter().filter(|entry| entry.label == "escalation failed").count();
        let path = self.paths.last().map_or("unknown", |(_, path)| path_name(*path));
        let stats = self.stats_info();
        let sdp = diagnostics
            .sdp
            .iter()
            .map(|summary| SdpInfo {
                engine_ms: summary.at_ms,
                role: summary.role.clone(),
                lines: summary
                    .lines
                    .iter()
                    .map(|line| SdpLineInfo {
                        kind: line.kind.clone(),
                        mid: line.mid.clone(),
                        direction: line.direction.clone(),
                        codecs: line.codecs.clone(),
                        payloads: line.payload_count,
                        ssrcs: line.ssrc_count,
                        rejected: line.rejected,
                    })
                    .collect(),
            })
            .collect();
        let mut report = Report {
            schema: SCHEMA,
            app: AppInfo {
                version: env!("CARGO_PKG_VERSION"),
                build: env!("TEAMS_BUILD_VERSION"),
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                demo: self.demo,
            },
            call: CallInfo {
                kind: kind_label(self.kind),
                direction: if self.incoming { "incoming" } else { "outgoing" },
                started_utc: self.started_at.format(READABLE_STAMP).to_string(),
                connect_ms: self.connected_ms,
                total_ms,
                connected_ms: self.connected_ms.zip(total_ms).map(|(connected, total)| total.saturating_sub(connected)),
                end_reason,
                engine_ice_connected_ms: diagnostics.ice_connected_ms,
                engine_peer_connected_ms: diagnostics.peer_connected_ms,
            },
            media: MediaInfo {
                path,
                path_changes: self.paths.iter().map(|(at_ms, path)| PathChange { at_ms: *at_ms, path: path_name(*path) }).collect(),
                escalations,
                escalation_failures,
                selected_pair: diagnostics.selected_pair.as_deref().map(|pair| scrubber.scrub(pair)),
                tls_version: diagnostics.tls_version.clone(),
                dtls_cipher: diagnostics.dtls_cipher.clone(),
                srtp_cipher: diagnostics.srtp_cipher.clone(),
            },
            participants: ParticipantInfo {
                peak_present: self.peak_participants,
                known: placeholders(&mut scrubber, &self.people),
                captions_received: self.captions,
            },
            problems: Vec::new(),
            stats,
            features,
            notices,
            states: self.states.iter().map(|(at_ms, state)| StateInfo { at_ms: *at_ms, state: state.clone() }).collect(),
            sdp,
            timeline,
        };
        let abnormal = matches!(self.ended, Some((_, EndReason::Failed(_) | EndReason::Dropped)));
        report.problems = problems(&report, abnormal);
        report
    }

    fn stats_info(&self) -> StatsInfo {
        let series = &self.series;
        let metrics: BTreeMap<&'static str, Range> = series
            .values()
            .iter()
            .map(|(name, value)| (*name, Range { min: value.min, max: value.max, last: value.last }))
            .collect();
        let samples = series.values().get("audio_in_packets").map_or(0, |value| value.samples);
        let health_names = [calling::NetworkLevel::Good, calling::NetworkLevel::Poor, calling::NetworkLevel::Bad];
        let limitation_names = [calling::Limitation::None, calling::Limitation::Cpu, calling::Limitation::Bandwidth];
        StatsInfo {
            samples,
            metrics,
            audio_in_stalled_samples: series.stalled_audio_in,
            audio_out_stalled_samples: series.stalled_audio_out,
            health_samples: health_names.iter().map(|level| (level_name(*level), series.level_samples[*level as usize])).collect(),
            health_level_changes: series.level_changes.iter().map(|(at_ms, level)| PathChange { at_ms: *at_ms, path: level }).collect(),
            quality_limitation_samples: limitation_names
                .iter()
                .map(|limitation| (limitation_name(*limitation), series.limitation_samples[*limitation as usize]))
                .collect(),
            cpu_limited_samples: series.cpu_limited_samples,
        }
    }

    fn feature_infos(&self, scrubber: &mut Scrubber, timeline: &[calling::timeline::TimelineEntry]) -> Vec<FeatureInfo> {
        let mut records: Vec<FeatureRecord> = self.features.clone();
        for entry in timeline {
            if entry.label == TRANSFER_LABEL
                && let Some(record) = oldest_pending(&mut records, TRANSFER_LABEL)
            {
                record.outcome = Outcome::Confirmed(entry.elapsed_ms);
            }
            let Some((_, feature)) = FEATURE_FAILURE_LABELS.iter().find(|(label, _)| entry.label == *label) else {
                continue;
            };
            if let Some(record) = oldest_pending(&mut records, feature) {
                record.outcome = Outcome::Failed(entry.detail.clone());
            }
        }
        records
            .into_iter()
            .map(|record| {
                let (result, confirmed_at_ms, error) = match record.outcome {
                    Outcome::Requested => ("no confirmation", None, None),
                    Outcome::Confirmed(at_ms) => ("ok", Some(at_ms), None),
                    Outcome::Failed(message) => ("failed", None, Some(scrubber.scrub(&message))),
                };
                FeatureInfo {
                    at_ms: record.at_ms,
                    feature: record.feature,
                    detail: scrubber.scrub(&record.detail),
                    result,
                    confirmed_at_ms,
                    error,
                }
            })
            .collect()
    }
}

fn oldest_pending<'a>(records: &'a mut [FeatureRecord], feature: &str) -> Option<&'a mut FeatureRecord> {
    records.iter_mut().find(|record| record.feature == feature && record.outcome == Outcome::Requested)
}

fn placeholders(scrubber: &mut Scrubber, people: &[(String, String)]) -> Vec<String> {
    let mut known = vec![super::scrub::SELF_PLACEHOLDER.to_owned()];
    for (mri, _) in people {
        let placeholder = scrubber.person(mri);
        if !known.contains(&placeholder) {
            known.push(placeholder);
        }
    }
    known
}

fn problems(report: &Report, abnormal_end: bool) -> Vec<String> {
    let mut found = Vec::new();
    if report.call.connect_ms.is_none() {
        found.push(format!("Never connected ({}).", report.call.end_reason));
    } else if abnormal_end {
        found.push(format!("Ended abnormally: {}.", report.call.end_reason));
    }
    let metrics = &report.stats.metrics;
    let samples = report.stats.samples;
    if samples > 0 && metrics.get("audio_in_packets").is_some_and(|range| range.max == 0.) {
        found.push("No inbound audio packets arrived.".to_owned());
    }
    if samples > 0 && metrics.get("audio_out_packets").is_some_and(|range| range.max == 0.) {
        found.push("No outbound audio packets were sent.".to_owned());
    }
    if samples >= MIN_SAMPLES_FOR_STALLS && report.stats.audio_in_stalled_samples > 0 {
        found.push(format!("Inbound audio stalled in {} of {samples} samples.", report.stats.audio_in_stalled_samples));
    }
    if samples >= MIN_SAMPLES_FOR_STALLS && report.stats.audio_out_stalled_samples > 0 {
        found.push(format!("Outbound audio stalled in {} of {samples} samples.", report.stats.audio_out_stalled_samples));
    }
    let degraded: u32 = ["poor", "bad"].iter().filter_map(|name| report.stats.health_samples.get(*name)).sum();
    if degraded > 0 {
        found.push(format!("Network health was poor or bad in {degraded} samples."));
    }
    if report.media.escalation_failures > 0 {
        found.push(format!("{} escalation(s) failed.", report.media.escalation_failures));
    }
    let failures: Vec<&TimelineInfo> =
        report.timeline.iter().filter(|entry| FAILURE_WORDS.iter().any(|word| entry.label.contains(word))).collect();
    if !failures.is_empty() {
        let labels: Vec<&str> = failures.iter().take(5).map(|entry| entry.label.as_str()).collect();
        found.push(format!("{} failure entries in the timeline: {}.", failures.len(), labels.join(", ")));
    }
    for feature in report.features.iter().filter(|feature| feature.result == "failed") {
        found.push(format!("Feature {} ({}) failed at {} ms.", feature.feature, feature.detail, feature.at_ms));
    }
    found
}
