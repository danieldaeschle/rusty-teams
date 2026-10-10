use std::sync::Arc;
use std::time::Duration;

use calling::video_frame::{VideoHub, VideoKey};
use calling::video_layout::VideoRouter;
use calling::video_pattern::{PatternKind, spawn_pattern};
use calling::video_send::LocalSink;
use calling::video_receive::{VideoCounters, spawn_video_pump};
use calling::{CallUpdate, devices::shared_factory};
use libwebrtc::video_source::native::NativeVideoSource;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::prelude::*;
use libwebrtc::rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit};
use libwebrtc::video_source::VideoResolution;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

fn h264_only(capabilities: Vec<RtpCodecCapability>) -> Vec<RtpCodecCapability> {
    capabilities
        .into_iter()
        .filter(|codec| matches!(codec.mime_type.to_ascii_lowercase().as_str(), "video/h264" | "video/rtx"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_generated_h264_stream_survives_the_pipeline_to_bgra_pictures() {
    let factory = shared_factory();
    let sender = factory.create_peer_connection(RtcConfiguration::default()).unwrap();
    let receiver = factory.create_peer_connection(RtcConfiguration::default()).unwrap();

    let (to_receiver, mut for_receiver) = mpsc::unbounded_channel::<IceCandidate>();
    let (to_sender, mut for_sender) = mpsc::unbounded_channel::<IceCandidate>();
    sender.on_ice_candidate(Some(Box::new(move |candidate| {
        let _ = to_receiver.send(candidate);
    })));
    receiver.on_ice_candidate(Some(Box::new(move |candidate| {
        let _ = to_sender.send(candidate);
    })));
    let (track_sender, mut tracks) = mpsc::unbounded_channel::<RtcVideoTrack>();
    receiver.on_track(Some(Box::new(move |event| {
        if let MediaStreamTrack::Video(track) = event.track {
            let _ = track_sender.send(track);
        }
    })));

    let (width, height) = PatternKind::Camera.size();
    let source = NativeVideoSource::new(VideoResolution { width, height }, false);
    let track = factory.create_video_track("camera", source.clone());
    let transceiver = sender
        .add_transceiver(
            MediaStreamTrack::Video(track),
            RtpTransceiverInit {
                direction: RtpTransceiverDirection::SendOnly,
                stream_ids: vec!["camera".into()],
                send_encodings: Vec::new(),
            },
        )
        .unwrap();
    transceiver
        .set_codec_preferences(h264_only(factory.get_rtp_sender_capabilities(MediaType::Video).codecs))
        .unwrap();
    let pattern = spawn_pattern(LocalSink::new(source, None), PatternKind::Camera, None);

    let offer = sender.create_offer(OfferOptions::default()).await.unwrap();
    sender.set_local_description(offer.clone()).await.unwrap();
    receiver.set_remote_description(offer).await.unwrap();
    let answer = receiver.create_answer(AnswerOptions::default()).await.unwrap();
    receiver.set_local_description(answer.clone()).await.unwrap();
    sender.set_remote_description(answer).await.unwrap();

    let relay = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(candidate) = for_receiver.recv() => { let _ = receiver.add_ice_candidate(candidate).await; }
                Some(candidate) = for_sender.recv() => { let _ = sender.add_ice_candidate(candidate).await; }
                else => break,
            }
        }
        (sender, receiver)
    });

    let remote_track = timeout(Duration::from_secs(15), tracks.recv()).await.expect("video track").expect("track");
    let (updates, mut ready) = mpsc::unbounded_channel();
    let hub = VideoHub::new(updates);
    let router = VideoRouter::default();
    router.set("0", Some(VideoKey::Person("8:orgid:loopback".into())));
    let counters = Arc::new(VideoCounters::default());
    let pump = spawn_video_pump(remote_track, "0".into(), router, hub.clone(), counters.clone());

    let mut pictures = 0;
    let mut last = None;
    let collect = async {
        while pictures < 20 {
            let Some(CallUpdate::VideoReady) = ready.recv().await else { break };
            for (key, picture) in hub.drain() {
                assert_eq!(key, VideoKey::Person("8:orgid:loopback".into()));
                pictures += 1;
                last = Some(picture);
            }
        }
    };
    timeout(Duration::from_secs(20), collect).await.expect("pictures within 20 s");
    sleep(Duration::from_millis(200)).await;
    let picture = last.unwrap();
    assert_eq!((picture.width, picture.height), (width, height));
    assert_eq!(picture.bgra.len(), (width * height * 4) as usize);
    let bright = picture.bgra.as_chunks::<4>().0.iter().filter(|pixel| pixel[0] > 150 && pixel[1] > 150 && pixel[2] > 150).count();
    assert!(bright > 1000, "the moving square is visible ({bright} bright pixels)");
    assert!(counters.received.load(std::sync::atomic::Ordering::Relaxed) >= 20);
    pump.abort();
    pattern.abort();
    relay.abort();
}
