use calling::devices::shared_factory;
use calling::sdp::{LineRole, parse, stream_lines, to_teams_offer};
use calling::video_layout::{SlotTable, add_one_to_one_lines, add_video_lines, offer_plan};
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::data_channel::DataChannelInit;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::prelude::*;
use libwebrtc::rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit};

fn counter() -> impl FnMut() -> u32 {
    let mut next = 5_000_000u32;
    move || {
        next += 1;
        next
    }
}

async fn browser_offer() -> String {
    offer_with(false).await
}

async fn offer_with(data_channel: bool) -> String {
    layout_offer(data_channel, false).await
}

async fn layout_offer(data_channel: bool, one_to_one: bool) -> String {
    let factory = shared_factory();
    let peer = factory.create_peer_connection(RtcConfiguration::default()).unwrap();
    let _channel = data_channel.then(|| peer.create_data_channel("main-channel", DataChannelInit::default()).unwrap());
    let source = NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 100);
    let track = factory.create_audio_track("microphone", source);
    peer.add_transceiver(
        MediaStreamTrack::Audio(track),
        RtpTransceiverInit {
            direction: RtpTransceiverDirection::SendRecv,
            stream_ids: vec!["microphone".into()],
            send_encodings: Vec::new(),
        },
    )
    .unwrap();
    if one_to_one {
        add_one_to_one_lines(&peer, &factory).unwrap();
    } else {
        add_video_lines(&peer, &factory).unwrap();
    }
    peer.create_offer(OfferOptions {
        offer_to_receive_audio: true,
        offer_to_receive_video: true,
        ..OfferOptions::default()
    })
    .await
    .unwrap()
    .to_string()
}

#[tokio::test]
async fn the_caption_data_channel_adds_one_x_data_line_after_the_video_lines() {
    let offer = to_teams_offer(&offer_with(true).await, &offer_plan(), &mut counter()).unwrap();
    let parsed = parse(&offer.sdp).unwrap();
    let kinds: Vec<(&str, &str)> = parsed.media.iter().map(|media| (media.kind.as_str(), media.mid().unwrap_or_default())).collect();
    assert_eq!(kinds.len(), 13);
    assert_eq!(kinds[12], ("x-data", "12"));
    assert_eq!(&kinds[..12].iter().map(|(_, mid)| *mid).collect::<Vec<_>>(), &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"]);
    assert_eq!(parsed.media[12].value("x-data-protocol"), Some("sctp"));
    assert_eq!(offer.lines.iter().filter(|line| line.role == LineRole::Data).count(), 1);
    let group = parsed.session.attributes.iter().find(|attribute| attribute.name == "group").and_then(|attribute| attribute.value.clone());
    assert_eq!(group.as_deref(), Some("BUNDLE 0 1 2 3 4 5 6 7 8 9 10 11 12"));
}

#[tokio::test]
async fn the_meeting_layout_is_signaled_like_the_web_client_offers_it() {
    let browser = browser_offer().await;
    let offer = to_teams_offer(&browser, &offer_plan(), &mut counter()).unwrap();
    let parsed = parse(&offer.sdp).unwrap();
    let summary: Vec<(String, String, String)> = parsed
        .media
        .iter()
        .map(|media| {
            (
                media.mid().unwrap_or_default().to_owned(),
                media.value("label").unwrap_or_default().to_owned(),
                media.direction().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let expected: Vec<(&str, &str, &str)> = vec![
        ("0", "main-audio", "sendrecv"),
        ("1", "main-video", "sendrecv"),
        ("2", "main-video", "recvonly"),
        ("3", "main-video", "recvonly"),
        ("4", "main-video", "recvonly"),
        ("5", "main-video", "recvonly"),
        ("6", "main-video", "recvonly"),
        ("7", "main-video", "recvonly"),
        ("8", "main-video", "recvonly"),
        ("9", "main-video", "recvonly"),
        ("10", "main-video", "recvonly"),
        ("11", "applicationsharing-video", "sendrecv"),
    ];
    let actual: Vec<(&str, &str, &str)> = summary.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
    assert_eq!(actual, expected);
    for receive in &parsed.media[2..11] {
        assert_eq!(receive.value("x-ssrc-range"), Some("1-1"));
        assert_eq!(receive.value("x-signaling-fb"), Some("* x-message app send:src recv:src,vc"));
        assert!(receive.values("rtpmap").all(|mapping| mapping.contains("H264") || mapping.contains("rtx")), "{:?}", receive.values("rtpmap").collect::<Vec<_>>());
    }
    assert!(parsed.media[1].value("x-ssrc-range").is_some());
    let group = parsed.session.attributes.iter().find(|attribute| attribute.name == "group").and_then(|attribute| attribute.value.clone());
    assert_eq!(group.as_deref(), Some("BUNDLE 0 1 2 3 4 5 6 7 8 9 10 11"));
    assert_eq!(offer.lines.iter().filter(|line| line.role == LineRole::ScreenShare).count(), 1);
}

#[tokio::test]
async fn the_slots_come_from_the_source_stream_ids_of_an_answer() {
    let browser = browser_offer().await;
    let offer = to_teams_offer(&browser, &offer_plan(), &mut counter()).unwrap();
    let mut answer = String::from("v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n");
    let roles: Vec<&str> = ["main-audio"].into_iter().chain(["main-video"; 10]).chain(["applicationsharing-video"]).collect();
    for (index, role) in roles.iter().enumerate() {
        let kind = if index == 0 { "audio" } else { "video" };
        answer.push_str(&format!("m={kind} 3478 RTP/SAVP 107\r\na=x-source-streamid:{}\r\na=label:{role}\r\na=mid:{index}\r\n", 201 + index));
    }
    let streams = stream_lines(&answer, &offer.lines).unwrap();
    let table = SlotTable::from_streams(&streams);
    let slots: Vec<(String, u32)> = table.people.iter().map(|slot| (slot.mid.clone(), slot.stream_id)).collect();
    let expected: Vec<(String, u32)> = (2..=10).map(|mid| (mid.to_string(), 201 + mid)).collect();
    assert_eq!(slots, expected);
    assert_eq!(table.screen.map(|slot| (slot.mid, slot.stream_id)), Some(("11".to_owned(), 212)));
}

fn camera_section(sdp: &str) -> Vec<&str> {
    let start = sdp.find("a=mid:1").expect("camera line");
    let begin = sdp[..start].rfind("m=video").unwrap();
    let end = sdp[start..].find("\nm=").map_or(sdp.len(), |offset| start + offset);
    sdp[begin..end].lines().map(str::trim_end).collect()
}

#[tokio::test]
async fn the_meeting_camera_offers_three_simulcast_layers() {
    let browser = browser_offer().await;
    let camera = camera_section(&browser);
    for rid in ["1", "2", "3"] {
        assert!(camera.contains(&format!("a=rid:{rid} send").as_str()), "{camera:?}");
    }
    assert!(camera.iter().any(|line| line.starts_with("a=simulcast:send") && line.contains('1') && line.contains('3')), "{camera:?}");
    assert!(!camera.iter().any(|line| line.starts_with("a=ssrc:")), "{camera:?}");
    let offer = to_teams_offer(&browser, &offer_plan(), &mut counter()).unwrap();
    let parsed = parse(&offer.sdp).unwrap();
    let signaled = &parsed.media[1];
    let range = signaled.value("x-ssrc-range").unwrap();
    let (first, last) = range.split_once('-').unwrap();
    assert_eq!(last.parse::<u32>().unwrap() - first.parse::<u32>().unwrap(), 99);
    assert!(signaled.value("ssrc").is_some_and(|value| value.contains("fake_attribute")), "{:?}", signaled.value("ssrc"));
    assert!(signaled.value("rid").is_none());
    assert!(signaled.value("simulcast").is_none());
}

#[tokio::test]
async fn a_one_to_one_camera_and_the_share_line_stay_single_layer() {
    let one_to_one = layout_offer(false, true).await;
    assert!(!one_to_one.contains("a=rid:"), "{one_to_one}");
    assert!(!one_to_one.contains("a=simulcast"));
    let meeting = browser_offer().await;
    assert_eq!(meeting.matches("a=rid:").count(), 3);
}
