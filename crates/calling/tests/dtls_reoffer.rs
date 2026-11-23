//! A same-peer re-offer without DTLS attributes, as Teams sends it in direct 1:1 calls, on real
//! libwebrtc peers: refused as is, accepted once the previous fingerprint is carried over.

use calling::devices::shared_factory;
use calling::sdp::with_previous_dtls;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::prelude::*;
use libwebrtc::rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit};

fn peer_with_audio() -> PeerConnection {
    let factory = shared_factory();
    let peer = factory
        .create_peer_connection(RtcConfiguration::default())
        .unwrap();
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
    peer
}

fn without_dtls(sdp: &str) -> String {
    sdp.lines()
        .filter(|line| !line.starts_with("a=fingerprint:") && !line.starts_with("a=setup:"))
        .map(|line| format!("{line}\r\n"))
        .collect()
}

/// The app (offerer) and the remote peer after one completed offer/answer.
async fn negotiated() -> (PeerConnection, PeerConnection) {
    let app = peer_with_audio();
    let remote = peer_with_audio();
    let offer = app.create_offer(OfferOptions::default()).await.unwrap();
    app.set_local_description(offer.clone()).await.unwrap();
    remote.set_remote_description(offer).await.unwrap();
    let answer = remote
        .create_answer(AnswerOptions::default())
        .await
        .unwrap();
    remote.set_local_description(answer.clone()).await.unwrap();
    app.set_remote_description(answer).await.unwrap();
    (app, remote)
}

#[tokio::test]
async fn a_re_offer_without_a_fingerprint_is_refused_as_it_is() {
    let (app, remote) = negotiated().await;
    let re_offer = remote
        .create_offer(OfferOptions::default())
        .await
        .unwrap()
        .to_string();
    let stripped = SessionDescription::parse(&without_dtls(&re_offer), SdpType::Offer).unwrap();
    let error = app.set_remote_description(stripped).await.unwrap_err();
    assert!(error.to_string().contains("fingerprint"), "{error}");
}

#[tokio::test]
async fn a_re_offer_with_the_previous_fingerprint_is_accepted() {
    let (app, remote) = negotiated().await;
    let re_offer = remote
        .create_offer(OfferOptions::default())
        .await
        .unwrap()
        .to_string();
    let previous = app.current_remote_description().unwrap().to_string();
    let filled = with_previous_dtls(&without_dtls(&re_offer), &previous).unwrap();
    let description = SessionDescription::parse(&filled, SdpType::Offer).unwrap();
    app.set_remote_description(description).await.unwrap();
}
