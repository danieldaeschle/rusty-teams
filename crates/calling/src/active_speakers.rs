use libwebrtc::peer_connection::PeerConnection;
use libwebrtc::prelude::MediaStreamTrack;
use libwebrtc::rtp_receiver::RtpSource;

/// How long after its last packet a contributing source still counts as speaking.
const RECENT_MS: i64 = 400;
/// Audio level in -dBov at or below which a source counts as speaking (0 loudest, 127 silence).
const SPEAKING_LEVEL: u8 = 60;

pub fn audio_sources(peer: &PeerConnection) -> Vec<RtpSource> {
    peer.receivers()
        .into_iter()
        .filter(|receiver| matches!(receiver.track(), Some(MediaStreamTrack::Audio(_))))
        .flat_map(|receiver| receiver.sources())
        .collect()
}

/// The source ids that are speaking now, like Teams web's `getContributingSources()` reading of the mixed meeting audio.
/// A mixer lists the people it mixes in as CSRCs; a direct call has only the peer's SSRC with its level.
pub fn speaking_sources(sources: &[RtpSource]) -> Vec<u32> {
    let Some(newest) = sources.iter().map(|source| source.timestamp_ms).max() else {
        return Vec::new();
    };
    let has_contributors = sources.iter().any(|source| source.contributing);
    let mut speaking: Vec<u32> = sources
        .iter()
        .filter(|source| source.contributing == has_contributors)
        .filter(|source| newest - source.timestamp_ms <= RECENT_MS)
        .filter(|source| match source.audio_level {
            Some(level) => level <= SPEAKING_LEVEL,
            None => source.contributing,
        })
        .map(|source| source.source_id)
        .collect();
    speaking.sort_unstable();
    speaking.dedup();
    speaking
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(
        source_id: u32,
        contributing: bool,
        audio_level: Option<u8>,
        timestamp_ms: i64,
    ) -> RtpSource {
        RtpSource {
            source_id,
            contributing,
            audio_level,
            timestamp_ms,
        }
    }

    #[test]
    fn mixer_contributors_seen_just_now_are_speaking() {
        let sources = [
            source(900, false, Some(30), 10_000),
            source(7, true, None, 9_900),
            source(8, true, Some(40), 10_000),
            source(9, true, None, 9_000),
        ];
        assert_eq!(speaking_sources(&sources), vec![7, 8]);
    }

    #[test]
    fn a_quiet_contributor_is_not_speaking() {
        let sources = [
            source(900, false, Some(30), 10_000),
            source(8, true, Some(100), 10_000),
        ];
        assert!(speaking_sources(&sources).is_empty());
    }

    #[test]
    fn a_direct_call_uses_the_peer_level() {
        assert_eq!(
            speaking_sources(&[source(42, false, Some(35), 5_000)]),
            vec![42]
        );
        assert!(speaking_sources(&[source(42, false, Some(127), 5_000)]).is_empty());
        assert!(speaking_sources(&[source(42, false, None, 5_000)]).is_empty());
        assert!(speaking_sources(&[]).is_empty());
    }
}
