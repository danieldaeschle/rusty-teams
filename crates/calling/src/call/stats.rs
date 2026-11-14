use super::*;

#[derive(Debug, Default)]
pub(super) struct StatsSnapshot {
    pub(super) inbound_packets: u64,
    pub(super) inbound_bytes: u64,
    pub(super) outbound_packets: u64,
    pub(super) outbound_bytes: u64,
    pub(super) source_levels: Vec<String>,
    pub(super) video_senders: Vec<String>,
    pub(super) audio_level: f64,
    pub(super) local_level: f64,
    pub(super) echo_return_loss_enhancement: f64,
    pub(super) video: VideoReport,
    pub(super) share_inbound_packets: u64,
    pub(super) share_outbound_packets: u64,
    pub(super) share_receive: Option<ShareReceive>,
    pub(super) dtls_state: String,
    pub(super) tls_version: String,
    pub(super) dtls_cipher: String,
    pub(super) srtp_cipher: String,
    pub(super) selected_pair: Option<String>,
    pub(super) health: HealthCounters,
    pub(super) transport: TransportDetails,
}

#[derive(Debug, Default)]
pub(super) struct ShareReceive {
    frames_decoded: u32,
    key_frames_decoded: u32,
    width: u32,
    height: u32,
    frames_per_second: f64,
    pli_count: u32,
    fir_count: u32,
    freeze_count: u32,
    qp_sum: u64,
    bytes_received: u64,
}

impl ShareReceive {
    fn average_qp(&self) -> u64 {
        self.qp_sum
            .checked_div(u64::from(self.frames_decoded))
            .unwrap_or(0)
    }

    pub(super) fn summary(&self) -> String {
        format!(
            " share in decoded={} keyframes={} {}x{} fps={:.0} qp={} bytes={} pli={} fir={} freezes={}",
            self.frames_decoded,
            self.key_frames_decoded,
            self.width,
            self.height,
            self.frames_per_second,
            self.average_qp(),
            self.bytes_received,
            self.pli_count,
            self.fir_count,
            self.freeze_count
        )
    }
}

pub(super) async fn collect_stats(peer: &PeerConnection, share_mids: &[String]) -> StatsSnapshot {
    let Ok(stats) = peer.get_stats().await else {
        return StatsSnapshot::default();
    };
    let mut snapshot = StatsSnapshot::default();
    (snapshot.health, snapshot.transport) = health_from_stats(&stats);
    let mut selected_pair_id = None;
    for entry in &stats {
        match entry {
            RtcStats::InboundRtp(inbound) if inbound.stream.kind == "audio" => {
                snapshot.inbound_packets += inbound.received.packets_received;
                snapshot.inbound_bytes += inbound.inbound.bytes_received;
                snapshot.audio_level = snapshot.audio_level.max(inbound.inbound.audio_level);
            }
            RtcStats::MediaSource(source) if source.source.kind == "audio" => {
                snapshot.local_level = snapshot.local_level.max(source.audio.audio_level);
                snapshot.source_levels.push(format!(
                    "{}={:.3}",
                    source.source.track_identifier, source.audio.audio_level
                ));
                snapshot.echo_return_loss_enhancement = source.audio.echo_return_loss_enhancement;
            }
            RtcStats::OutboundRtp(outbound) if outbound.stream.kind == "audio" => {
                snapshot.outbound_packets += outbound.sent.packets_sent;
                snapshot.outbound_bytes += outbound.sent.bytes_sent;
            }
            RtcStats::InboundRtp(inbound) if inbound.stream.kind == "video" => {
                snapshot.video.inbound_packets += inbound.received.packets_received;
                if share_mids.contains(&inbound.inbound.mid) {
                    snapshot.share_inbound_packets += inbound.received.packets_received;
                    snapshot.share_receive = Some(ShareReceive {
                        frames_decoded: inbound.inbound.frames_decoded,
                        key_frames_decoded: inbound.inbound.key_frames_decoded,
                        width: inbound.inbound.frame_width,
                        height: inbound.inbound.frame_height,
                        frames_per_second: inbound.inbound.frames_per_second,
                        pli_count: inbound.inbound.pli_count,
                        fir_count: inbound.inbound.fir_count,
                        freeze_count: inbound.inbound.freeze_count,
                        qp_sum: inbound.inbound.qp_sum,
                        bytes_received: inbound.inbound.bytes_received,
                    });
                }
                snapshot.video.inbound_frames_decoded += u64::from(inbound.inbound.frames_decoded);
                snapshot.video.inbound_width = snapshot
                    .video
                    .inbound_width
                    .max(inbound.inbound.frame_width);
                snapshot.video.inbound_height = snapshot
                    .video
                    .inbound_height
                    .max(inbound.inbound.frame_height);
            }
            RtcStats::RemoteInboundRtp(remote) if remote.stream.kind == "video" => {
                snapshot.video.remote_reports += remote.remote_inbound.round_trip_time_measurements;
                snapshot.video.remote_round_trip_ms = snapshot
                    .video
                    .remote_round_trip_ms
                    .max(remote.remote_inbound.round_trip_time * 1000.0);
            }
            RtcStats::OutboundRtp(outbound) if outbound.stream.kind == "video" => {
                snapshot.video.outbound_packets += outbound.sent.packets_sent;
                if share_mids.contains(&outbound.outbound.mid) {
                    snapshot.share_outbound_packets += outbound.sent.packets_sent;
                }
                snapshot.video.outbound_frames_encoded +=
                    u64::from(outbound.outbound.frames_encoded);
                snapshot.video_senders.push(format!(
                    "mid {} packets={} key={} pli={} fir={} nack={} target={:.0}kbps limit={:?} active={}",
                    outbound.outbound.mid,
                    outbound.sent.packets_sent,
                    outbound.outbound.key_frames_encoded,
                    outbound.outbound.pli_count,
                    outbound.outbound.fir_count,
                    outbound.outbound.nack_count,
                    outbound.outbound.target_bitrate / 1000.0,
                    outbound.outbound.quality_limitation_reason,
                    outbound.outbound.active
                ));
                snapshot.video.outbound_width = snapshot
                    .video
                    .outbound_width
                    .max(outbound.outbound.frame_width);
                snapshot.video.outbound_height = snapshot
                    .video
                    .outbound_height
                    .max(outbound.outbound.frame_height);
            }
            RtcStats::Transport(transport) => {
                snapshot.dtls_state = format!("{:?}", transport.transport.dtls_state);
                snapshot.tls_version = transport.transport.tls_version.clone();
                snapshot.dtls_cipher = transport.transport.dtls_cipher.clone();
                snapshot.srtp_cipher = transport.transport.srtp_cipher.clone();
                if !transport.transport.selected_candidate_pair_id.is_empty() {
                    selected_pair_id = Some(transport.transport.selected_candidate_pair_id.clone());
                }
            }
            _ => {}
        }
    }
    if let Some(pair_id) = selected_pair_id {
        let pair = stats.iter().find_map(|entry| match entry {
            RtcStats::CandidatePair(pair) if pair.rtc.id == pair_id => Some(pair),
            _ => None,
        });
        if let Some(pair) = pair {
            let describe = |candidate_id: &str| {
                stats
                    .iter()
                    .find_map(|entry| match entry {
                        RtcStats::LocalCandidate(local) if local.rtc.id == candidate_id => {
                            Some(&local.local_candidate)
                        }
                        RtcStats::RemoteCandidate(remote) if remote.rtc.id == candidate_id => {
                            Some(&remote.remote_candidate)
                        }
                        _ => None,
                    })
                    .map(|candidate| {
                        let kind = candidate
                            .candidate_type
                            .map(|kind| format!("{kind:?}"))
                            .unwrap_or_default();
                        let relay = candidate
                            .relay_protocol
                            .map(|relay| format!(" via {relay:?}"))
                            .unwrap_or_default();
                        format!("{kind}/{}{relay}", candidate.protocol)
                    })
                    .unwrap_or_else(|| "?".into())
            };
            snapshot.selected_pair = Some(format!(
                "local {} <-> remote {}",
                describe(&pair.candidate_pair.local_candidate_id),
                describe(&pair.candidate_pair.remote_candidate_id)
            ));
        }
    }
    snapshot
}
