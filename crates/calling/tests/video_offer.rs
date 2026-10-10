use calling::devices::shared_factory;
use calling::sdp::{LineRole, parse, stream_lines, to_teams_offer};
use calling::video_layout::{SlotTable, add_video_lines, offer_plan};
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
    add_video_lines(&peer, &factory).unwrap();
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
    assert_eq!(kinds.len(), 8);
    assert_eq!(kinds[7], ("x-data", "7"));
    assert_eq!(&kinds[..7].iter().map(|(_, mid)| *mid).collect::<Vec<_>>(), &["0", "1", "2", "3", "4", "5", "6"]);
    assert_eq!(parsed.media[7].value("x-data-protocol"), Some("sctp"));
    assert_eq!(offer.lines.iter().filter(|line| line.role == LineRole::Data).count(), 1);
    let group = parsed.session.attributes.iter().find(|attribute| attribute.name == "group").and_then(|attribute| attribute.value.clone());
    assert_eq!(group.as_deref(), Some("BUNDLE 0 1 2 3 4 5 6 7"));
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
        ("6", "applicationsharing-video", "sendrecv"),
    ];
    let actual: Vec<(&str, &str, &str)> = summary.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
    assert_eq!(actual, expected);
    for receive in &parsed.media[2..6] {
        assert_eq!(receive.value("x-ssrc-range"), Some("1-1"));
        assert_eq!(receive.value("x-signaling-fb"), Some("* x-message app send:src recv:src,vc"));
        assert!(receive.values("rtpmap").all(|mapping| mapping.contains("H264") || mapping.contains("rtx")), "{:?}", receive.values("rtpmap").collect::<Vec<_>>());
    }
    assert!(parsed.media[1].value("x-ssrc-range").is_some());
    let group = parsed.session.attributes.iter().find(|attribute| attribute.name == "group").and_then(|attribute| attribute.value.clone());
    assert_eq!(group.as_deref(), Some("BUNDLE 0 1 2 3 4 5 6"));
    assert_eq!(offer.lines.iter().filter(|line| line.role == LineRole::ScreenShare).count(), 1);
}

#[tokio::test]
async fn the_slots_come_from_the_source_stream_ids_of_an_answer() {
    let browser = browser_offer().await;
    let offer = to_teams_offer(&browser, &offer_plan(), &mut counter()).unwrap();
    let mut answer = String::from("v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n");
    for (index, role) in ["main-audio", "main-video", "main-video", "main-video", "main-video", "main-video", "applicationsharing-video"].iter().enumerate() {
        let kind = if index == 0 { "audio" } else { "video" };
        answer.push_str(&format!("m={kind} 3478 RTP/SAVP 107\r\na=x-source-streamid:{}\r\na=label:{role}\r\na=mid:{index}\r\n", 201 + index));
    }
    let streams = stream_lines(&answer, &offer.lines).unwrap();
    let table = SlotTable::from_streams(&streams);
    let slots: Vec<(String, u32)> = table.people.iter().map(|slot| (slot.mid.clone(), slot.stream_id)).collect();
    assert_eq!(slots, vec![("2".to_owned(), 203), ("3".to_owned(), 204), ("4".to_owned(), 205), ("5".to_owned(), 206)]);
    assert_eq!(table.screen.map(|slot| (slot.mid, slot.stream_id)), Some(("6".to_owned(), 207)));
}
