use std::fmt::Write;

use super::document::{FeatureInfo, Range, Report, SdpInfo};

const SUMMARY_METRICS: [(&str, &str); 14] = [
    ("audio_in_packets", "Audio in, packets"),
    ("audio_out_packets", "Audio out, packets"),
    ("audio_in_level", "Audio in, level"),
    ("audio_out_level", "Audio out, level"),
    ("video_in_packets", "Video in, packets"),
    ("video_in_frames_decoded", "Video in, frames decoded"),
    ("video_out_packets", "Video out, packets"),
    ("video_out_frames_encoded", "Video out, frames encoded"),
    ("share_in_packets", "Share in, packets"),
    ("share_out_packets", "Share out, packets"),
    ("loss_percent", "Loss, %"),
    ("round_trip_ms", "Round trip, ms"),
    ("jitter_ms", "Jitter, ms"),
    ("received_kbps", "Received, kbit/s"),
];

fn seconds(milliseconds: Option<u128>) -> String {
    milliseconds.map_or("n/a".to_owned(), |value| format!("{:.1} s", value as f64 / 1000.))
}

fn number(value: f64) -> String {
    if value.fract() == 0. { format!("{value:.0}") } else { format!("{value:.3}") }
}

fn resolution(report: &Report, width: &str, height: &str) -> String {
    let side = |name: &str| report.stats.metrics.get(name).map_or(0., |range| range.last);
    let (width, height) = (side(width), side(height));
    if width == 0. { "none".to_owned() } else { format!("{width:.0}x{height:.0}") }
}

pub fn render(report: &Report) -> String {
    let mut text = String::new();
    let call = &report.call;
    let _ = writeln!(text, "# Call test report\n");
    let _ = writeln!(text, "| | |\n|---|---|");
    let _ = writeln!(text, "| Kind | {} |", call.kind);
    let _ = writeln!(text, "| Direction | {} |", call.direction);
    let _ = writeln!(text, "| Started (UTC) | {} |", call.started_utc);
    let _ = writeln!(text, "| Connect time | {} |", seconds(call.connect_ms));
    let _ = writeln!(text, "| Connected for | {} |", seconds(call.connected_ms));
    let _ = writeln!(text, "| Total | {} |", seconds(call.total_ms));
    let _ = writeln!(text, "| End reason | {} |", call.end_reason);
    let _ = writeln!(
        text,
        "| Media path | {} ({} escalation(s), {} failed) |",
        report.media.path, report.media.escalations, report.media.escalation_failures
    );
    let _ = writeln!(text, "| Participants | {} present at peak, {} known |", report.participants.peak_present, report.participants.known.len());
    let _ = writeln!(text, "| Video | in {}, out {} |", resolution(report, "video_in_width", "video_in_height"), resolution(report, "video_out_width", "video_out_height"));
    let _ = writeln!(
        text,
        "| App | {} ({}), {} {}{} |",
        report.app.version,
        report.app.build,
        report.app.os,
        report.app.arch,
        if report.app.demo { ", demo mode" } else { "" }
    );
    write_problems(&mut text, report);
    write_stats(&mut text, report);
    write_features(&mut text, &report.features);
    write_notices(&mut text, report);
    write_sdp(&mut text, &report.sdp);
    let _ = writeln!(text, "\nThe JSON file next to this one holds the full timeline ({} entries).", report.timeline.len());
    text
}

fn write_problems(text: &mut String, report: &Report) {
    let _ = writeln!(text, "\n## Findings\n");
    if report.problems.is_empty() {
        let _ = writeln!(text, "- Nothing suspicious found.");
    }
    for problem in &report.problems {
        let _ = writeln!(text, "- {problem}");
    }
}

fn write_stats(text: &mut String, report: &Report) {
    let _ = writeln!(text, "\n## Stats ({} samples, every 2 s)\n", report.stats.samples);
    let _ = writeln!(text, "| Metric | Min | Max | Last |\n|---|---|---|---|");
    for (name, label) in SUMMARY_METRICS {
        if let Some(Range { min, max, last }) = report.stats.metrics.get(name) {
            let _ = writeln!(text, "| {label} | {} | {} | {} |", number(*min), number(*max), number(*last));
        }
    }
    let health: Vec<String> = report.stats.health_samples.iter().map(|(level, count)| format!("{level} {count}")).collect();
    let limits: Vec<String> = report.stats.quality_limitation_samples.iter().map(|(limit, count)| format!("{limit} {count}")).collect();
    let _ = writeln!(text, "\nHealth samples: {}. Quality limitation samples: {}.", health.join(", "), limits.join(", "));
}

fn write_features(text: &mut String, features: &[FeatureInfo]) {
    let _ = writeln!(text, "\n## Features used\n");
    if features.is_empty() {
        let _ = writeln!(text, "None.");
        return;
    }
    let _ = writeln!(text, "| At | Feature | Detail | Result |\n|---|---|---|---|");
    for feature in features {
        let result = match &feature.error {
            Some(error) => format!("failed: {error}"),
            None => feature.result.to_owned(),
        };
        let _ = writeln!(text, "| {} | {} | {} | {} |", seconds(Some(feature.at_ms)), feature.feature, feature.detail, result);
    }
}

fn write_notices(text: &mut String, report: &Report) {
    let _ = writeln!(text, "\n## Notices shown\n");
    if report.notices.is_empty() {
        let _ = writeln!(text, "None.");
    }
    for notice in &report.notices {
        let _ = writeln!(text, "- {}: {}", seconds(Some(notice.at_ms)), notice.text);
    }
}

fn write_sdp(text: &mut String, sdp: &[SdpInfo]) {
    let _ = writeln!(text, "\n## SDP (summary only)\n");
    if sdp.is_empty() {
        let _ = writeln!(text, "None recorded.");
    }
    for summary in sdp {
        let lines: Vec<String> = summary
            .lines
            .iter()
            .map(|line| {
                let state = if line.rejected { "rejected".to_owned() } else { line.direction.clone() };
                format!("{} mid {} {} [{}]", line.kind, line.mid, state, line.codecs.join(","))
            })
            .collect();
        let _ = writeln!(text, "- {} at {} ms: {}", summary.role, summary.engine_ms, lines.join("; "));
    }
}
