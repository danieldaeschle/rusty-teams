use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use libwebrtc::peer_connection::PeerConnection;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::rtp_parameters::RtpCodecCapability;
use libwebrtc::rtp_transceiver::{RtpTransceiver, RtpTransceiverDirection, RtpTransceiverInit};
use libwebrtc::MediaType;
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::sdp::{LineRole, OfferPlan, StreamLine};
use crate::video_frame::VideoKey;

pub const RECEIVE_SLOTS: usize = 4;
pub const CAMERA_MID: &str = "1";
pub const SHARE_MID: &str = "6";
pub const ONE_TO_ONE_SHARE_MID: &str = "2";
const CAMERA_LINE_LABEL: &str = "main-video";
const SHARE_LINE_LABEL: &str = "applicationsharing-video";
const CAMERA_STREAM: &str = "camera";
const SCREEN_STREAM: &str = "screen";
const H264: &str = "video/h264";
const RTX: &str = "video/rtx";
const NO_SOURCE: i64 = -1;
const PERSON_REQUEST: ReceiveLimits = ReceiveLimits { max_fs: 3600, max_fps: 3000, max_mbps: 108_000 };
const SCREEN_REQUEST: ReceiveLimits = ReceiveLimits { max_fs: 8160, max_fps: 1500, max_mbps: 135_000 };
const SUBSCRIBE_PROFILE: &str = "64001f";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReceiveLimits {
    max_fs: u32,
    max_fps: u32,
    max_mbps: u32,
}

pub fn offer_plan() -> OfferPlan {
    OfferPlan {
        screen_share_mids: vec![SHARE_MID.to_owned()],
        gallery_mids: Vec::new(),
    }
}

pub fn one_to_one_plan() -> OfferPlan {
    OfferPlan {
        screen_share_mids: vec![ONE_TO_ONE_SHARE_MID.to_owned()],
        gallery_mids: Vec::new(),
    }
}

/// Offered after the audio line: camera, `RECEIVE_SLOTS` receive-only slots, screen share (mids 1, 2-5, 6).
#[derive(Clone)]
pub struct VideoLines {
    pub camera: RtpTransceiver,
    pub share: Option<RtpTransceiver>,
}

fn h264_only(capabilities: Vec<RtpCodecCapability>) -> Vec<RtpCodecCapability> {
    capabilities
        .into_iter()
        .filter(|codec| {
            let mime = codec.mime_type.to_ascii_lowercase();
            mime == H264 || mime == RTX
        })
        .collect()
}

fn add_line(
    peer: &PeerConnection,
    direction: RtpTransceiverDirection,
    stream: Option<&str>,
    codecs: Vec<RtpCodecCapability>,
) -> Result<RtpTransceiver> {
    let init = RtpTransceiverInit {
        direction,
        stream_ids: stream.map(str::to_owned).into_iter().collect(),
        send_encodings: Vec::new(),
    };
    let transceiver = peer
        .add_transceiver_for_media(MediaType::Video, init)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    transceiver
        .set_codec_preferences(codecs)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    Ok(transceiver)
}

pub fn add_video_lines(peer: &PeerConnection, factory: &PeerConnectionFactory) -> Result<VideoLines> {
    let sending = h264_only(factory.get_rtp_sender_capabilities(MediaType::Video).codecs);
    let receiving = h264_only(factory.get_rtp_receiver_capabilities(MediaType::Video).codecs);
    let camera = add_line(peer, RtpTransceiverDirection::SendRecv, Some(CAMERA_STREAM), sending.clone())?;
    for _ in 0..RECEIVE_SLOTS {
        add_line(peer, RtpTransceiverDirection::RecvOnly, None, receiving.clone())?;
    }
    let share = add_line(peer, RtpTransceiverDirection::SendRecv, Some(SCREEN_STREAM), sending)?;
    Ok(VideoLines { camera, share: Some(share) })
}

pub fn add_one_to_one_lines(peer: &PeerConnection, factory: &PeerConnectionFactory) -> Result<VideoLines> {
    let sending = h264_only(factory.get_rtp_sender_capabilities(MediaType::Video).codecs);
    let receiving = h264_only(factory.get_rtp_receiver_capabilities(MediaType::Video).codecs);
    let camera = add_line(peer, RtpTransceiverDirection::SendRecv, Some(CAMERA_STREAM), sending)?;
    add_line(peer, RtpTransceiverDirection::RecvOnly, None, receiving)?;
    Ok(VideoLines { camera, share: None })
}

/// Per-line direction told to the mixer through `updateMediaDescriptions`; no SDP renegotiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDescription {
    pub mid: &'static str,
    pub direction: &'static str,
    pub label: Option<&'static str>,
}

impl MediaDescription {
    pub fn camera(sending: bool) -> Self {
        Self::line(CAMERA_MID, sending, CAMERA_LINE_LABEL)
    }

    pub fn share(sending: bool) -> Self {
        Self::line(SHARE_MID, sending, SHARE_LINE_LABEL)
    }

    fn line(mid: &'static str, sending: bool, label: &'static str) -> Self {
        if sending {
            MediaDescription { mid, direction: "sendrecv", label: Some(label) }
        } else {
            MediaDescription { mid, direction: "recvonly", label: None }
        }
    }

    pub fn wire(&self) -> Value {
        let mut description = json!({"mid": self.mid, "direction": self.direction});
        if let Some(label) = self.label {
            description["label"] = json!(label);
        }
        description
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveSlot {
    pub mid: String,
    pub stream_id: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotTable {
    pub people: Vec<ReceiveSlot>,
    pub screen: Option<ReceiveSlot>,
}

impl SlotTable {
    pub fn from_streams(streams: &[StreamLine]) -> Self {
        let slot = |stream: &StreamLine| {
            Some(ReceiveSlot {
                mid: stream.browser_mid.clone(),
                stream_id: stream.source_stream_id?,
            })
        };
        let people = streams
            .iter()
            .filter(|stream| stream.role == LineRole::MainVideo)
            .skip(1)
            .filter_map(slot)
            .take(RECEIVE_SLOTS)
            .collect();
        let screen = streams.iter().find(|stream| stream.role == LineRole::ScreenShare).and_then(slot);
        SlotTable { people, screen }
    }

    pub fn is_empty(&self) -> bool {
        self.people.is_empty() && self.screen.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub mri: String,
    pub source: u32,
}

pub fn plan_receive(current: &[Option<Assignment>], candidates: &[Assignment]) -> Vec<Option<Assignment>> {
    let wanted: Vec<&Assignment> = candidates.iter().take(current.len()).collect();
    let mut next: Vec<Option<Assignment>> = current
        .iter()
        .map(|slot| slot.clone().filter(|held| wanted.contains(&held)))
        .collect();
    for assignment in wanted {
        if next.iter().flatten().any(|held| held == assignment) {
            continue;
        }
        if let Some(free) = next.iter_mut().find(|slot| slot.is_none()) {
            *free = Some(assignment.clone());
        }
    }
    next
}

pub fn prioritize(candidates: Vec<(String, u32)>, speaker_history: &[String]) -> Vec<Assignment> {
    let rank = |mri: &str| speaker_history.iter().position(|speaker| speaker == mri).unwrap_or(usize::MAX);
    let mut ordered: Vec<Assignment> = candidates
        .into_iter()
        .map(|(mri, source)| Assignment { mri, source })
        .collect();
    ordered.sort_by_key(|assignment| rank(&assignment.mri));
    ordered
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRequest {
    pub mid: String,
    pub stream_id: u32,
    pub source: Option<u32>,
    pub screen: bool,
}

pub fn requests_for(
    current: &[Option<Assignment>],
    next: &[Option<Assignment>],
    slots: &[ReceiveSlot],
) -> Vec<SourceRequest> {
    slots
        .iter()
        .zip(current.iter().zip(next))
        .filter(|(_, (before, after))| before != after)
        .map(|(slot, (_, after))| SourceRequest {
            mid: slot.mid.clone(),
            stream_id: slot.stream_id,
            source: after.as_ref().map(|assignment| assignment.source),
            screen: false,
        })
        .collect()
}

impl SourceRequest {
    /// Body of the `applyChannelParameters` link, the signaling twin of the data-channel `sr` message.
    pub fn body(&self, sequence_number: u64) -> Value {
        let limits = if self.screen { SCREEN_REQUEST } else { PERSON_REQUEST };
        let control = json!({
            "controlVideoStreaming": {
                "sequenceNumber": sequence_number,
                "controlInfo": {
                    "sourceId": self.source.map_or(NO_SOURCE, i64::from),
                    "streamMsid": self.stream_id,
                    "fmtParams": [{
                        "max-fs": limits.max_fs,
                        "max-mbps": limits.max_mbps,
                        "max-fps": limits.max_fps,
                        "profile-level-id": SUBSCRIBE_PROFILE,
                    }],
                },
            },
        });
        json!({
            "applyChannelParameters": {
                "multiChannelParameter": {
                    "mids": [self.mid],
                    "mediaParameter": control.to_string(),
                },
            },
        })
    }
}

#[derive(Clone, Default)]
pub struct VideoRouter {
    keys: Arc<Mutex<HashMap<String, VideoKey>>>,
}

impl VideoRouter {
    pub fn set(&self, mid: &str, key: Option<VideoKey>) {
        let mut keys = self.keys.lock().expect("router lock");
        match key {
            Some(key) => keys.insert(mid.to_owned(), key),
            None => keys.remove(mid),
        };
    }

    pub fn key_for(&self, mid: &str) -> Option<VideoKey> {
        self.keys.lock().expect("router lock").get(mid).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment(name: &str, source: u32) -> Assignment {
        Assignment { mri: format!("8:orgid:{name}"), source }
    }

    fn stream(mid: &str, role: LineRole, id: Option<u32>) -> StreamLine {
        StreamLine { mid: mid.into(), browser_mid: mid.into(), role, source_stream_id: id }
    }

    #[test]
    fn slots_are_the_main_video_lines_after_the_camera_line() {
        let streams = vec![
            stream("0", LineRole::MainAudio, Some(201)),
            stream("1", LineRole::MainVideo, Some(202)),
            stream("2", LineRole::MainVideo, Some(203)),
            stream("3", LineRole::MainVideo, Some(204)),
            stream("4", LineRole::MainVideo, None),
            stream("5", LineRole::MainVideo, Some(206)),
            stream("6", LineRole::ScreenShare, Some(207)),
        ];
        let table = SlotTable::from_streams(&streams);
        let mids: Vec<&str> = table.people.iter().map(|slot| slot.mid.as_str()).collect();
        assert_eq!(mids, vec!["2", "3", "5"]);
        assert_eq!(table.screen, Some(ReceiveSlot { mid: "6".into(), stream_id: 207 }));
    }

    #[test]
    fn nobody_is_dropped_while_they_are_still_wanted() {
        let held = vec![Some(assignment("a", 1)), Some(assignment("b", 2)), None, None];
        let candidates = vec![assignment("c", 3), assignment("a", 1), assignment("b", 2)];
        let next = plan_receive(&held, &candidates);
        assert_eq!(next[0], Some(assignment("a", 1)));
        assert_eq!(next[1], Some(assignment("b", 2)));
        assert_eq!(next[2], Some(assignment("c", 3)));
        assert_eq!(next[3], None);
    }

    #[test]
    fn a_new_speaker_replaces_the_least_recent_person() {
        let held: Vec<Option<Assignment>> = (1..=4).map(|index| Some(assignment(&format!("p{index}"), index))).collect();
        let history = vec!["8:orgid:p5".to_owned(), "8:orgid:p1".to_owned(), "8:orgid:p2".to_owned(), "8:orgid:p3".to_owned()];
        let roster = (1..=5).map(|index| (format!("8:orgid:p{index}"), index)).collect();
        let next = plan_receive(&held, &prioritize(roster, &history));
        let names: Vec<&str> = next.iter().flatten().map(|held| held.mri.as_str()).collect();
        assert_eq!(names, vec!["8:orgid:p1", "8:orgid:p2", "8:orgid:p3", "8:orgid:p5"]);
    }

    #[test]
    fn people_who_left_free_their_slot() {
        let held = vec![Some(assignment("a", 1)), Some(assignment("b", 2))];
        let next = plan_receive(&held, &[assignment("b", 2)]);
        assert_eq!(next, vec![None, Some(assignment("b", 2))]);
        let slots = vec![
            ReceiveSlot { mid: "2".into(), stream_id: 203 },
            ReceiveSlot { mid: "3".into(), stream_id: 204 },
        ];
        let requests = requests_for(&held, &next, &slots);
        assert_eq!(requests, vec![SourceRequest { mid: "2".into(), stream_id: 203, source: None, screen: false }]);
    }

    #[test]
    fn a_source_request_wraps_the_control_message_like_the_web_client() {
        let request = SourceRequest { mid: "3".into(), stream_id: 204, source: Some(302), screen: false };
        let body = request.body(7);
        let channel = &body["applyChannelParameters"]["multiChannelParameter"];
        assert_eq!(channel["mids"], json!(["3"]));
        let parameter: Value = serde_json::from_str(channel["mediaParameter"].as_str().unwrap()).unwrap();
        let info = &parameter["controlVideoStreaming"]["controlInfo"];
        assert_eq!(parameter["controlVideoStreaming"]["sequenceNumber"], 7);
        assert_eq!(info["sourceId"], 302);
        assert_eq!(info["streamMsid"], 204);
        assert_eq!(info["fmtParams"][0]["max-fs"], 3600);
        assert_eq!(info["fmtParams"][0]["max-fps"], 3000);
        assert_eq!(info["fmtParams"][0]["profile-level-id"], "64001f");
        let cleared = SourceRequest { source: None, ..request.clone() }.body(8);
        let parameter: Value =
            serde_json::from_str(cleared["applyChannelParameters"]["multiChannelParameter"]["mediaParameter"].as_str().unwrap()).unwrap();
        assert_eq!(parameter["controlVideoStreaming"]["controlInfo"]["sourceId"], -1);
        let screen = SourceRequest { screen: true, ..request }.body(9);
        let parameter: Value =
            serde_json::from_str(screen["applyChannelParameters"]["multiChannelParameter"]["mediaParameter"].as_str().unwrap()).unwrap();
        assert_eq!(parameter["controlVideoStreaming"]["controlInfo"]["fmtParams"][0]["max-fs"], 8160);
    }

    #[test]
    fn media_descriptions_tell_the_mixer_what_a_line_does() {
        assert_eq!(
            MediaDescription::camera(true).wire(),
            json!({"mid": "1", "direction": "sendrecv", "label": "main-video"})
        );
        assert_eq!(MediaDescription::camera(false).wire(), json!({"mid": "1", "direction": "recvonly"}));
        assert_eq!(
            MediaDescription::share(true).wire(),
            json!({"mid": "6", "direction": "sendrecv", "label": "applicationsharing-video"})
        );
    }

    #[test]
    fn the_router_follows_assignments() {
        let router = VideoRouter::default();
        assert_eq!(router.key_for("2"), None);
        router.set("2", Some(VideoKey::Person("8:orgid:a".into())));
        assert_eq!(router.key_for("2"), Some(VideoKey::Person("8:orgid:a".into())));
        router.set("2", None);
        assert_eq!(router.key_for("2"), None);
    }
}
