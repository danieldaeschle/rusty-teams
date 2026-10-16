use crate::call::{CallReport, VideoReport};
use crate::sdp;
use crate::timeline::TimelineEntry;

const RTX_CODEC: &str = "rtx";

#[derive(Debug, Clone, PartialEq)]
pub struct SdpLine {
    pub kind: String,
    pub mid: String,
    pub direction: String,
    pub codecs: Vec<String>,
    pub payload_count: usize,
    pub ssrc_count: usize,
    pub rejected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SdpSummary {
    pub at_ms: u128,
    pub role: String,
    pub lines: Vec<SdpLine>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatsSample {
    pub audio_in_packets: u64,
    pub audio_out_packets: u64,
    pub audio_in_level: f64,
    pub audio_out_level: f64,
    pub video: VideoReport,
    pub share_in_packets: u64,
    pub share_out_packets: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallDiagnostics {
    pub timeline: Vec<TimelineEntry>,
    pub sdp: Vec<SdpSummary>,
    pub ice_connected_ms: Option<u128>,
    pub peer_connected_ms: Option<u128>,
    pub selected_pair: Option<String>,
    pub tls_version: String,
    pub dtls_cipher: String,
    pub srtp_cipher: String,
}

impl CallDiagnostics {
    pub fn new(report: &CallReport, sdp: Vec<SdpSummary>) -> Self {
        CallDiagnostics {
            timeline: report.timeline.clone(),
            sdp,
            ice_connected_ms: report.ice_connected_ms,
            peer_connected_ms: report.peer_connected_ms,
            selected_pair: report.selected_pair.clone(),
            tls_version: report.tls_version.clone(),
            dtls_cipher: report.dtls_cipher.clone(),
            srtp_cipher: report.srtp_cipher.clone(),
        }
    }
}

pub fn summarize_sdp(at_ms: u128, role: &str, text: &str) -> Option<SdpSummary> {
    let description = sdp::parse(text).ok()?;
    let lines = description
        .media
        .iter()
        .map(|media| {
            let named: Vec<String> = media
                .values("rtpmap")
                .filter_map(|value| value.split_once(' '))
                .filter_map(|(_, encoding)| encoding.split('/').next())
                .filter(|name| !name.eq_ignore_ascii_case(RTX_CODEC))
                .map(str::to_owned)
                .collect();
            let mut codecs: Vec<String> = Vec::new();
            for codec in named {
                if !codecs.contains(&codec) {
                    codecs.push(codec);
                }
            }
            SdpLine {
                kind: media.kind.clone(),
                mid: media.mid().unwrap_or_default().to_owned(),
                direction: media.direction().unwrap_or("sendrecv").to_owned(),
                codecs,
                payload_count: media.formats.len(),
                ssrc_count: media.values("ssrc").filter(|value| value.contains(" cname:")).count(),
                rejected: media.port == "0",
            }
        })
        .collect();
    Some(SdpSummary { at_ms, role: role.to_owned(), lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFER: &str = "v=0\r\no=- 1 2 IN IP4 203.0.113.9\r\ns=-\r\nt=0 0\r\n\
m=audio 9 UDP/TLS/RTP/SAVPF 111 9\r\nc=IN IP4 203.0.113.9\r\na=mid:0\r\na=sendrecv\r\na=ice-ufrag:abcd\r\na=ice-pwd:secretsecretsecret\r\n\
a=rtpmap:111 opus/48000/2\r\na=rtpmap:9 G722/8000\r\na=ssrc:111 cname:xyz\r\n\
m=video 0 UDP/TLS/RTP/SAVPF 96 97\r\na=mid:1\r\na=inactive\r\na=rtpmap:96 H264/90000\r\na=rtpmap:97 rtx/90000\r\n";

    #[test]
    fn a_summary_keeps_kinds_mids_directions_codecs_and_counts_only() {
        let summary = summarize_sdp(40, "local offer", OFFER).expect("parses");
        assert_eq!(summary.lines.len(), 2);
        assert_eq!(summary.lines[0].kind, "audio");
        assert_eq!(summary.lines[0].mid, "0");
        assert_eq!(summary.lines[0].direction, "sendrecv");
        assert_eq!(summary.lines[0].codecs, ["opus", "G722"]);
        assert_eq!(summary.lines[0].payload_count, 2);
        assert_eq!(summary.lines[0].ssrc_count, 1);
        assert_eq!(summary.lines[1].direction, "inactive");
        assert_eq!(summary.lines[1].codecs, ["H264"]);
        assert!(summary.lines[1].rejected);
        let rendered = format!("{summary:?}");
        assert!(!rendered.contains("203.0.113.9") && !rendered.contains("secretsecret") && !rendered.contains("abcd"));
    }

    #[test]
    fn text_that_is_no_sdp_gives_no_summary() {
        assert!(summarize_sdp(0, "remote offer", "not sdp").is_none());
    }
}
