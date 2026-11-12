use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Document;

const SCHEMA: u32 = 1;
const SHADOW_KIND: &str = "callShadow";
const FILE_STAMP: &str = "%Y%m%dT%H%M%SZ";
const READABLE_STAMP: &str = "%Y-%m-%d %H:%M:%S";
const MAX_EVENTS: usize = 20_000;
const MAX_TIMELINE_ROWS: usize = 300;
const STATS_EVENT: &str = "stats";
const REQUEST_EVENT: &str = "request";

#[derive(Deserialize)]
struct Incoming {
    kind: String,
    at: i64,
    event: String,
    #[serde(default)]
    data: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowEvent {
    pub at_ms: i64,
    pub event: String,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct WebShadow {
    started_at: DateTime<Utc>,
    first_at: Option<i64>,
    events: Vec<ShadowEvent>,
    dropped: usize,
}

#[derive(Serialize)]
struct ShadowReport<'a> {
    schema: u32,
    engine: &'static str,
    version: &'static str,
    build: &'static str,
    started_utc: String,
    duration_ms: i64,
    dropped_events: usize,
    events: &'a [ShadowEvent],
}

#[derive(Default)]
struct RequestGroup {
    first_ms: i64,
    count: usize,
    failed: usize,
    total_ms: i64,
    statuses: BTreeSet<i64>,
}

#[derive(Default)]
struct PeerSummary {
    connected_ms: Option<i64>,
    final_state: String,
    pair: String,
    last_rows: Vec<Value>,
    max_round_trip_ms: f64,
}

impl WebShadow {
    pub fn new(started_at: DateTime<Utc>) -> Self {
        Self {
            started_at,
            first_at: None,
            events: Vec::new(),
            dropped: 0,
        }
    }

    pub fn observe(&mut self, raw: &str) -> bool {
        let Ok(incoming) = serde_json::from_str::<Incoming>(raw) else {
            return false;
        };
        if incoming.kind != SHADOW_KIND {
            return false;
        }
        if self.events.len() >= MAX_EVENTS {
            self.dropped += 1;
            return true;
        }
        let first_at = *self.first_at.get_or_insert(incoming.at);
        self.events.push(ShadowEvent {
            at_ms: incoming.at - first_at,
            event: incoming.event,
            data: incoming.data,
        });
        true
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn document(&self) -> Document {
        let report = ShadowReport {
            schema: SCHEMA,
            engine: "teams-web",
            version: env!("CARGO_PKG_VERSION"),
            build: app_base::version::build_version(),
            started_utc: self.started_at.format(READABLE_STAMP).to_string(),
            duration_ms: self.duration_ms(),
            dropped_events: self.dropped,
            events: &self.events,
        };
        Document {
            stem: format!("{}-web", self.started_at.format(FILE_STAMP)),
            json: serde_json::to_string_pretty(&report)
                .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
            markdown: self.markdown(),
        }
    }

    fn duration_ms(&self) -> i64 {
        self.events.last().map_or(0, |event| event.at_ms)
    }

    fn peers(&self) -> BTreeMap<i64, PeerSummary> {
        let mut peers: BTreeMap<i64, PeerSummary> = BTreeMap::new();
        for event in &self.events {
            let Some(peer) = event.data.get("peer").and_then(Value::as_i64) else {
                continue;
            };
            let summary = peers.entry(peer).or_default();
            match event.event.as_str() {
                "connection" => {
                    let state = text(&event.data, "state");
                    if state == "connected" && summary.connected_ms.is_none() {
                        summary.connected_ms = Some(event.at_ms);
                    }
                    summary.final_state = state;
                }
                "close" => summary.final_state = "closed".to_owned(),
                STATS_EVENT => {
                    let rows = event
                        .data
                        .get("rows")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for row in &rows {
                        if text(row, "type") == "pair" {
                            summary.pair =
                                format!("{} -> {}", text(row, "local"), text(row, "remote"));
                            let round_trip = row
                                .get("roundTripTime")
                                .and_then(Value::as_f64)
                                .unwrap_or(0.)
                                * 1000.;
                            summary.max_round_trip_ms = summary.max_round_trip_ms.max(round_trip);
                        }
                    }
                    summary.last_rows = rows;
                }
                _ => {}
            }
        }
        peers
    }

    fn markdown(&self) -> String {
        let mut markdown = String::new();
        let peers = self.peers();
        let _ = writeln!(markdown, "# Teams web call report\n");
        let _ = writeln!(markdown, "| | |\n|---|---|");
        let _ = writeln!(
            markdown,
            "| Started (UTC) | {} |",
            self.started_at.format(READABLE_STAMP)
        );
        let _ = writeln!(markdown, "| Recorded | {} |", seconds(self.duration_ms()));
        let _ = writeln!(markdown, "| Peer connections | {} |", peers.len());
        let _ = writeln!(
            markdown,
            "| Codecs | {} |",
            self.codecs().into_iter().collect::<Vec<_>>().join(", ")
        );
        if self.dropped > 0 {
            let _ = writeln!(markdown, "| Dropped events | {} |", self.dropped);
        }
        let _ = writeln!(markdown, "\n## Connections\n");
        let _ = writeln!(
            markdown,
            "| Peer | Connected after | Final state | Pair | Max round trip |\n|---|---|---|---|---|"
        );
        for (peer, summary) in &peers {
            let connected = summary.connected_ms.map_or("never".to_owned(), seconds);
            let _ = writeln!(
                markdown,
                "| {peer} | {connected} | {} | {} | {:.0} ms |",
                summary.final_state, summary.pair, summary.max_round_trip_ms
            );
        }
        let _ = writeln!(markdown, "\n## Media, last sample\n");
        let _ = writeln!(
            markdown,
            "| Peer | Direction | Kind | Codec | Packets | Lost | Jitter | Fps | Size | Limited by |\n|---|---|---|---|---|---|---|---|---|---|"
        );
        for (peer, summary) in &peers {
            let active = |row: &&Value| {
                text(row, "type") != "pair"
                    && row.get("packets").and_then(Value::as_f64).unwrap_or(0.) > 0.
            };
            for row in summary.last_rows.iter().filter(active) {
                let size = match (
                    row.get("frameWidth").and_then(Value::as_i64),
                    row.get("frameHeight").and_then(Value::as_i64),
                ) {
                    (Some(width), Some(height)) => format!("{width}x{height}"),
                    _ => String::new(),
                };
                let _ = writeln!(
                    markdown,
                    "| {peer} | {} | {} | {} | {} | {} | {} | {} | {size} | {} |",
                    text(row, "type").trim_end_matches("-rtp"),
                    text(row, "kind"),
                    text(row, "codec"),
                    text(row, "packets"),
                    text(row, "packetsLost"),
                    text(row, "jitter"),
                    text(row, "framesPerSecond"),
                    text(row, "qualityLimitationReason"),
                );
            }
        }
        self.write_requests(&mut markdown);
        self.write_timeline(&mut markdown);
        markdown
    }

    fn codecs(&self) -> BTreeSet<String> {
        self.events
            .iter()
            .filter(|event| event.event == STATS_EVENT)
            .flat_map(|event| {
                event
                    .data
                    .get("rows")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            })
            .filter_map(|row| row.get("codec").and_then(Value::as_str).map(str::to_owned))
            .collect()
    }

    fn write_requests(&self, markdown: &mut String) {
        let mut groups: BTreeMap<(String, String), RequestGroup> = BTreeMap::new();
        for event in self
            .events
            .iter()
            .filter(|event| event.event == REQUEST_EVENT)
        {
            let key = (text(&event.data, "method"), text(&event.data, "path"));
            let group = groups.entry(key).or_insert_with(|| RequestGroup {
                first_ms: event.at_ms,
                ..RequestGroup::default()
            });
            let status = event
                .data
                .get("status")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            group.count += 1;
            group.total_ms += event.data.get("ms").and_then(Value::as_i64).unwrap_or(0);
            group.statuses.insert(status);
            if !(200..400).contains(&status) {
                group.failed += 1;
            }
        }
        let total: usize = groups.values().map(|group| group.count).sum();
        let failed: usize = groups.values().map(|group| group.failed).sum();
        let mut ordered: Vec<_> = groups.into_iter().collect();
        ordered.sort_by_key(|(_, group)| group.first_ms);
        let _ = writeln!(
            markdown,
            "\n## Requests\n\n{total} requests, {failed} failed\n"
        );
        let _ = writeln!(
            markdown,
            "| First at | Method | Path | Count | Failed | Status | Avg took |\n|---|---|---|---|---|---|---|"
        );
        for ((method, path), group) in ordered.iter().take(MAX_TIMELINE_ROWS) {
            let statuses = group
                .statuses
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                markdown,
                "| {} | {method} | {path} | {} | {} | {statuses} | {} ms |",
                seconds(group.first_ms),
                group.count,
                group.failed,
                group.total_ms / group.count as i64,
            );
        }
    }

    fn write_timeline(&self, markdown: &mut String) {
        let _ = writeln!(markdown, "\n## Timeline\n");
        let _ = writeln!(markdown, "| At | Event | Detail |\n|---|---|---|");
        let timeline = self
            .events
            .iter()
            .filter(|event| event.event != STATS_EVENT && event.event != REQUEST_EVENT);
        for event in timeline.take(MAX_TIMELINE_ROWS) {
            let detail = match event.data.get("lines").and_then(Value::as_array) {
                Some(lines) => format!(
                    "{} {}",
                    text(&event.data, "type"),
                    lines
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                None => event.data.to_string(),
            };
            let _ = writeln!(
                markdown,
                "| {} | {} | {} |",
                seconds(event.at_ms),
                event.event,
                detail.replace('|', "/")
            );
        }
    }
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(Value::Number(number)) => number.as_f64().map_or(number.to_string(), |number| {
            if number.fract() == 0. {
                format!("{number:.0}")
            } else {
                format!("{number:.3}")
            }
        }),
        Some(other) => other.to_string(),
    }
}

fn seconds(milliseconds: i64) -> String {
    format!("{:.1} s", milliseconds as f64 / 1000.)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(at: i64, event: &str, data: Value) -> String {
        serde_json::json!({"kind": SHADOW_KIND, "at": at, "event": event, "data": data}).to_string()
    }

    fn started() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-07T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn only_shadow_messages_are_recorded_relative_to_the_first() {
        let mut shadow = WebShadow::new(started());
        assert!(!shadow.observe(r#"{"kind":"newWindow","url":"https://example.com"}"#));
        assert!(!shadow.observe("not json"));
        assert!(shadow.observe(&message(5_000, "peer", serde_json::json!({"peer": 1}))));
        assert!(shadow.observe(&message(
            6_500,
            "connection",
            serde_json::json!({"peer": 1, "state": "connected"})
        )));
        assert_eq!(shadow.events.len(), 2);
        assert_eq!(shadow.events[1].at_ms, 1_500);
    }

    #[test]
    fn the_report_names_connect_time_codecs_and_failed_requests() {
        let mut shadow = WebShadow::new(started());
        shadow.observe(&message(0, "peer", serde_json::json!({"peer": 1})));
        shadow.observe(&message(
            2_000,
            "connection",
            serde_json::json!({"peer": 1, "state": "connected"}),
        ));
        shadow.observe(&message(
            4_000,
            STATS_EVENT,
            serde_json::json!({"peer": 1, "rows": [
                {"type": "inbound-rtp", "kind": "audio", "codec": "audio/opus", "packets": 200, "packetsLost": 3},
                {"type": "pair", "roundTripTime": 0.042, "local": "host/udp", "remote": "relay/udp"}
            ]}),
        ));
        shadow.observe(&message(4_500, REQUEST_EVENT, serde_json::json!({"method": "POST", "path": "api.flightproxy.skype.com/api/v2/cpconv", "status": 500, "ms": 120})));
        let document = shadow.document();
        assert_eq!(document.stem, "20261107T150000Z-web");
        assert!(
            document
                .markdown
                .contains("| 1 | 2.0 s | connected | host/udp -> relay/udp | 42 ms |")
        );
        assert!(document.markdown.contains("| Codecs | audio/opus |"));
        assert!(document.markdown.contains("1 requests, 1 failed"));
        assert!(document.json.contains("\"engine\": \"teams-web\""));
    }
}
