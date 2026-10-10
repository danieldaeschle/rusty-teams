use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chatsvc::{CallbackReplier, InstanceNames, Realtime, RealtimeConfig, RealtimeEvent, TrouterCallback};
use futures_util::StreamExt;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::prelude::*;
use libwebrtc::rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit};
use libwebrtc::session_description::{SdpType, SessionDescription};
use libwebrtc::stats::RtcStats;
use session::Session;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio::time::{Instant, interval, sleep_until, timeout};

use crate::audio::{SAMPLE_RATE, rms, tone_ratio, write_wav};
use crate::audio_io::{AudioMode, AudioSetup};
use crate::control::{CallCommand, CallControl, CallUpdate};
use crate::devices::{self, DeviceChoice};
use crate::error::{Error, Result};
use crate::mute::{MuteCommand, MuteEffect};
use crate::relay::{DEFAULT_RELAY_HOST, fetch_relay_grant};
use crate::sdp::{OfferPlan, SignaledOffer, to_browser_answer, to_teams_offer};
use crate::signaling::{
    Conversation, EchoInvitation, Participant, Signaling, TenantRouting, fetch_self, find_echo_bot_thread,
};
use crate::state::{CallSignal, CallState};
use crate::timeline::{Timeline, TimelineEntry};
use crate::trouter_events::{CallEnd, CallEvent, CallbackLinks, acceptance_acknowledgement, classify, decode_body};

const ENDPOINT_WAIT: Duration = Duration::from_secs(30);
const END_CALLBACK_WAIT: Duration = Duration::from_secs(4);
const STATS_PERIOD: Duration = Duration::from_millis(200);
const TICKS_PER_SECOND: u32 = 5;
const RECONNECT_WINDOW: Duration = Duration::from_secs(30);
const FAR_FUTURE: Duration = Duration::from_secs(60 * 60 * 24 * 365);
const PRESENCE_SUFFIX: &str = "unifiedPresenceService";
pub const TRACE_ENV: &str = "CALLING_TRACE";

#[derive(Debug, Clone)]
pub struct TestCallOptions {
    pub hold_after_connected: Option<Duration>,
    pub hard_limit: Option<Duration>,
    pub reconnect_window: Duration,
    pub audio: AudioMode,
    pub tone_hz: f32,
    pub input: DeviceChoice,
    pub output: DeviceChoice,
    pub relay_host: String,
    pub routing: TenantRouting,
    pub record_remote: bool,
    pub wav_path: Option<PathBuf>,
    pub sdp_dump_dir: Option<PathBuf>,
    pub trace: bool,
}

impl Default for TestCallOptions {
    fn default() -> Self {
        TestCallOptions {
            hold_after_connected: None,
            hard_limit: None,
            reconnect_window: RECONNECT_WINDOW,
            audio: AudioMode::from_env(),
            tone_hz: 440.0,
            input: DeviceChoice::SystemDefault,
            output: DeviceChoice::SystemDefault,
            relay_host: DEFAULT_RELAY_HOST.to_owned(),
            routing: TenantRouting::default(),
            record_remote: false,
            wav_path: None,
            sdp_dump_dir: None,
            trace: std::env::var_os(TRACE_ENV).is_some(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AudioSecond {
    pub second: u32,
    pub packets: u64,
    pub bytes: u64,
    pub audio_level: f64,
    pub decoded_rms: f32,
    pub tone_ratio: f32,
}

#[derive(Debug, Clone, Default)]
pub struct TestCallReport {
    pub timeline: Vec<TimelineEntry>,
    pub ice_connected_ms: Option<u128>,
    pub peer_connected_ms: Option<u128>,
    pub selected_pair: Option<String>,
    pub tls_version: String,
    pub dtls_cipher: String,
    pub srtp_cipher: String,
    pub inbound_packets: u64,
    pub inbound_bytes: u64,
    pub outbound_packets: u64,
    pub seconds_with_inbound_audio: u32,
    pub seconds: Vec<AudioSecond>,
    pub end: Option<CallEnd>,
    pub left_cleanly: bool,
    pub recorded_samples: usize,
}

pub fn calling_realtime_config() -> RealtimeConfig {
    RealtimeConfig {
        instance: InstanceNames {
            global: "__callingTrouter".into(),
            binding: "__callingRealtime".into(),
            endpoint_storage_key: "__callingEpid".into(),
        },
        forward_callbacks: true,
        ..RealtimeConfig::default()
    }
}

fn random_u32() -> u32 {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) & 0x7fff_ffff
}

fn callback_base(trouter_uri: &str) -> String {
    let trimmed = trouter_uri.strip_suffix(PRESENCE_SUFFIX).unwrap_or(trouter_uri).trim_end_matches('/');
    format!("{trimmed}/")
}

#[derive(Default)]
struct Recording {
    samples: Vec<i16>,
    current_second: Vec<i16>,
    per_second: Vec<(f32, f32)>,
}

enum PeerEvent {
    Ice(IceConnectionState),
    Connection(PeerConnectionState),
    Track(RtcAudioTrack),
}

#[derive(Clone)]
struct RemoteCall {
    signaling: Arc<Signaling>,
    conversation: Conversation,
    participant: Participant,
}

#[derive(Default)]
struct Live {
    realtime: Option<Realtime>,
    callbacks: Option<mpsc::UnboundedReceiver<TrouterCallback>>,
    replier: Option<CallbackReplier>,
    peer: Option<PeerConnection>,
    audio: Option<AudioSetup>,
    remote: Option<RemoteCall>,
    broker: Option<JoinHandle<()>>,
    broker_running: Arc<AtomicBool>,
}

fn advance(state: &mut CallState, signal: CallSignal, control: &CallControl) -> bool {
    let next = state.next(signal, std::time::Instant::now());
    if next == *state {
        return false;
    }
    *state = next.clone();
    control.send(CallUpdate::State(next));
    true
}

pub async fn run_test_call(
    session: &Session,
    poll_session: &Session,
    options: TestCallOptions,
    mut control: CallControl,
) -> Result<TestCallReport> {
    let timeline = Timeline::new(options.trace);
    let mut report = TestCallReport::default();
    let mut state = CallState::Idle;
    advance(&mut state, CallSignal::Dial, &control);
    let mut live = Live::default();
    let recording = Arc::new(Mutex::new(Recording::default()));
    let setup = CallSetup {
        session,
        poll_session,
        options: &options,
        timeline: &timeline,
        recording: &recording,
    };
    let outcome = drive(&mut live, &setup, &mut control, &mut state, &mut report).await;
    let failure = match outcome {
        Ok(()) => None,
        Err(Error::Cancelled) => {
            advance(&mut state, CallSignal::LocalLeave, &control);
            None
        }
        Err(error) => {
            advance(&mut state, CallSignal::Error(error.to_string()), &control);
            Some(error)
        }
    };
    tear_down(live, &timeline, &mut report).await;
    let recording = recording.lock().await;
    report.recorded_samples = recording.samples.len();
    if let Some(path) = &options.wav_path {
        write_wav(path, &recording.samples).map_err(|error| Error::Webrtc(format!("wav: {error}")))?;
    }
    report.timeline = timeline.entries();
    match failure {
        Some(error) => Err(error),
        None => Ok(report),
    }
}

struct CallSetup<'a> {
    session: &'a Session,
    poll_session: &'a Session,
    options: &'a TestCallOptions,
    timeline: &'a Timeline,
    recording: &'a Arc<Mutex<Recording>>,
}

async fn drive(
    live: &mut Live,
    setup: &CallSetup<'_>,
    control: &mut CallControl,
    state: &mut CallState,
    report: &mut TestCallReport,
) -> Result<()> {
    let CallSetup {
        session,
        poll_session,
        options,
        timeline,
        recording,
    } = *setup;
    let mut realtime = Realtime::start_with(session, calling_realtime_config()).await?;
    live.callbacks = Some(
        realtime
            .take_callbacks()
            .ok_or_else(|| Error::Callback("callback stream already taken".into()))?,
    );
    live.replier = Some(realtime.callback_replier());
    let endpoint = control
        .until_hangup(timeout(ENDPOINT_WAIT, async {
            while let Some(event) = realtime.recv().await {
                if let RealtimeEvent::Endpoint(endpoint) = event {
                    return Some(endpoint);
                }
            }
            None
        }))
        .await;
    live.realtime = Some(realtime);
    let endpoint = endpoint?
        .ok()
        .flatten()
        .ok_or_else(|| Error::Callback("Trouter socket never announced its endpoint".into()))?;
    timeline.record("Trouter connected", "callback socket ready");

    let identity = control.until_hangup(fetch_self(session)).await??;
    let thread_id = control
        .until_hangup(find_echo_bot_thread(session, &identity.object_id))
        .await??;
    timeline.record("echo bot chat", if thread_id.is_some() { "found" } else { "none" });
    let grant = control.until_hangup(fetch_relay_grant(session)).await??;
    timeline.record("GET trap/tokens", format!("relay grant, expires in {} s", grant.expires_in_seconds));

    let factory = devices::shared_factory();
    let mut input = options.input.clone();
    let mut output = options.output.clone();
    let audio = AudioSetup::start(&factory, options.audio, options.tone_hz, &input, &output);
    control.send(CallUpdate::ListenOnly(audio.listen_only));
    control.send(CallUpdate::Devices(devices::list_devices(&factory)));
    control.send(CallUpdate::Selected {
        input: input.clone(),
        output: output.clone(),
    });
    let track = audio.track.clone();
    live.audio = Some(audio);

    let mut configuration = RtcConfiguration::default();
    configuration.ice_servers = vec![grant.ice_server(&options.relay_host)];
    let peer = factory
        .create_peer_connection(configuration)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    live.peer = Some(peer.clone());
    let (peer_events, mut peer_receiver) = mpsc::unbounded_channel();
    let ice_sender = peer_events.clone();
    peer.on_ice_connection_state_change(Some(Box::new(move |state| {
        let _ = ice_sender.send(PeerEvent::Ice(state));
    })));
    let connection_sender = peer_events.clone();
    peer.on_connection_state_change(Some(Box::new(move |state| {
        let _ = connection_sender.send(PeerEvent::Connection(state));
    })));
    let track_sender = peer_events;
    peer.on_track(Some(Box::new(move |event| {
        if let MediaStreamTrack::Audio(track) = event.track {
            let _ = track_sender.send(PeerEvent::Track(track));
        }
    })));

    let transceiver_init = RtpTransceiverInit {
        direction: RtpTransceiverDirection::SendRecv,
        stream_ids: vec!["microphone".into()],
        send_encodings: Vec::new(),
    };
    peer.add_transceiver(MediaStreamTrack::Audio(track), transceiver_init)
        .map_err(|error| Error::Webrtc(error.to_string()))?;

    let offer = peer
        .create_offer(OfferOptions {
            offer_to_receive_audio: true,
            ..OfferOptions::default()
        })
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    peer.set_local_description(offer.clone())
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let browser_offer = offer.to_string();
    let mut numbers = random_u32;
    let signaled = to_teams_offer(&browser_offer, &OfferPlan::default(), &mut numbers)?;
    timeline.record("setLocalDescription", format!("{} m-line(s) signaled", signaled.lines.len()));
    dump_sdp(options, "1-browser-offer.sdp", &browser_offer);
    dump_sdp(options, "2-teams-offer.sdp", &signaled.sdp);

    let participant = Participant {
        mri: format!("8:orgid:{}", identity.object_id),
        display_name: identity.display_name.clone(),
        endpoint_id: endpoint.endpoint_id.clone(),
        participant_id: uuid::Uuid::new_v4().to_string(),
        language_id: "en-gb".into(),
    };
    let links = CallbackLinks::new(&callback_base(&endpoint.trouter_uri), &uuid::Uuid::new_v4().to_string());
    let media_leg_id = uuid::Uuid::new_v4().simple().to_string().to_uppercase();
    let signaling = Arc::new(Signaling::new(
        session.clone(),
        poll_session.clone(),
        options.routing.clone(),
        timeline.clone(),
    ));
    let call_started = Instant::now();
    let conversation = control
        .until_hangup(signaling.create_conversation(&EchoInvitation {
            from: &participant,
            offer_sdp: &signaled.sdp,
            media_leg_id: &media_leg_id,
            callbacks: &links,
        }))
        .await??;
    let remote = RemoteCall {
        signaling: signaling.clone(),
        conversation: conversation.clone(),
        participant: participant.clone(),
    };
    live.remote = Some(remote.clone());
    live.broker_running.store(true, Ordering::SeqCst);
    live.broker = Some(tokio::spawn(poll_broker(
        signaling.clone(),
        conversation.clone(),
        live.broker_running.clone(),
    )));

    let mut muted = false;
    if let Err(error) = signaling.update_endpoint_state(&conversation, &participant, muted).await {
        timeline.record("updateEndpointState failed", error.to_string());
    }

    let callbacks = live.callbacks.as_mut().expect("callbacks set above");
    let replier = live.replier.clone().expect("replier set above");
    let mut stats_tick = interval(STATS_PERIOD);
    let hard_deadline = options.hard_limit.map(|limit| call_started + limit);
    let mut leave_at = hard_deadline;
    let mut reconnect_deadline: Option<Instant> = None;
    let mut remote_set = false;
    let mut previous_packets = 0u64;
    let mut ticks = 0u32;
    let mut second = 0u32;
    loop {
        tokio::select! {
            Some(callback) = callbacks.recv() => {
                let request_id = callback.request_id;
                match handle_callback(&callback, timeline) {
                    Ok(CallEvent::Acceptance(acceptance)) => {
                        replier.reply(request_id, 200, acceptance_acknowledgement(&links).to_string());
                        dump_sdp(options, "3-teams-answer.sdp", &acceptance.sdp);
                        if let Ok(answer) = to_browser_answer(&acceptance.sdp, &signaled, &browser_offer) {
                            dump_sdp(options, "4-browser-answer.sdp", &answer);
                        }
                        apply_answer(&peer, &acceptance.sdp, &signaled, &browser_offer).await.inspect_err(|error| {
                            timeline.record("answer rejected", error.to_string());
                        })?;
                        remote_set = true;
                        let directions: Vec<String> = peer
                            .transceivers()
                            .iter()
                            .map(|transceiver| format!("{:?}", transceiver.current_direction()))
                            .collect();
                        timeline.record("setRemoteDescription", format!("answer applied, directions {}", directions.join(",")));
                        let signaling = signaling.clone();
                        let conversation = conversation.clone();
                        let participant = participant.clone();
                        let links = links.clone();
                        let thread_id = thread_id.clone();
                        tokio::spawn(async move {
                            let _ = signaling.add_echo_bot(&conversation, &participant, thread_id.as_deref(), &links).await;
                            let _ = signaling.update_endpoint_metadata(&conversation, &participant).await;
                        });
                    }
                    Ok(CallEvent::End(end)) => {
                        replier.reply(request_id, 200, "");
                        report.end = Some(end);
                        advance(state, CallSignal::RemoteEnd, control);
                        break;
                    }
                    Ok(_) => replier.reply(request_id, 200, ""),
                    Err(error) => {
                        replier.reply(request_id, 200, "");
                        timeline.record("callback not understood", error.to_string());
                    }
                }
            }
            Some(event) = peer_receiver.recv() => match event {
                PeerEvent::Ice(ice_state) => {
                    timeline.record("ICE state", format!("{ice_state:?}"));
                    if ice_state == IceConnectionState::Connected && report.ice_connected_ms.is_none() {
                        report.ice_connected_ms = Some(timeline.elapsed_ms());
                    }
                }
                PeerEvent::Connection(connection) => {
                    timeline.record("PeerConnection state", format!("{connection:?}"));
                    match connection {
                        PeerConnectionState::Connected => {
                            reconnect_deadline = None;
                            advance(state, CallSignal::MediaConnected, control);
                            if report.peer_connected_ms.is_none() {
                                report.peer_connected_ms = Some(timeline.elapsed_ms());
                                if let Some(hold) = options.hold_after_connected {
                                    let hold_until = Instant::now() + hold;
                                    leave_at = Some(hard_deadline.map_or(hold_until, |deadline| deadline.min(hold_until)));
                                }
                            }
                        }
                        PeerConnectionState::Disconnected => {
                            if advance(state, CallSignal::MediaDisconnected, control) {
                                reconnect_deadline = Some(Instant::now() + options.reconnect_window);
                            }
                        }
                        PeerConnectionState::Failed => {
                            advance(state, CallSignal::MediaFailed, control);
                            break;
                        }
                        _ => {}
                    }
                }
                PeerEvent::Track(track) => {
                    timeline.record("remote audio track", "receiving");
                    if options.record_remote {
                        tokio::spawn(record_track(track, recording.clone(), options.tone_hz));
                    }
                }
            },
            command = control.recv() => match command {
                Some(CallCommand::Mute(mute)) => {
                    muted = apply_mute(mute, muted, live.audio.as_ref().expect("audio set above"), &remote, control);
                }
                Some(CallCommand::SelectInput(choice)) => {
                    if live.audio.as_ref().is_some_and(AudioSetup::uses_devices) {
                        timeline.record("input device", format!("{choice:?} applied={}", devices::select_input(&factory, &choice)));
                    }
                    input = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                }
                Some(CallCommand::SelectOutput(choice)) => {
                    if live.audio.as_ref().is_some_and(AudioSetup::uses_devices) {
                        timeline.record("output device", format!("{choice:?} applied={}", devices::select_output(&factory, &choice)));
                    }
                    output = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                }
                Some(CallCommand::RefreshDevices) => {
                    control.send(CallUpdate::Devices(devices::list_devices(&factory)));
                }
                Some(CallCommand::Hangup) | None => {
                    advance(state, CallSignal::LocalLeave, control);
                    break;
                }
            },
            _ = stats_tick.tick(), if remote_set => {
                let snapshot = collect_stats(&peer).await;
                ticks += 1;
                let local_level = if muted { 0.0 } else { snapshot.local_level as f32 };
                control.send(CallUpdate::Levels { local: local_level, remote: snapshot.audio_level as f32 });
                if snapshot.selected_pair.is_some() {
                    report.selected_pair = snapshot.selected_pair.clone();
                    report.tls_version = snapshot.tls_version.clone();
                    report.dtls_cipher = snapshot.dtls_cipher.clone();
                    report.srtp_cipher = snapshot.srtp_cipher.clone();
                }
                report.inbound_packets = snapshot.inbound_packets;
                report.inbound_bytes = snapshot.inbound_bytes;
                report.outbound_packets = snapshot.outbound_packets;
                if ticks.is_multiple_of(TICKS_PER_SECOND) {
                    second += 1;
                    let (decoded_rms, ratio) = {
                        let recording = recording.lock().await;
                        recording.per_second.last().copied().unwrap_or_default()
                    };
                    if snapshot.inbound_packets > previous_packets {
                        report.seconds_with_inbound_audio += 1;
                    }
                    previous_packets = snapshot.inbound_packets;
                    report.seconds.push(AudioSecond {
                        second,
                        packets: snapshot.inbound_packets,
                        bytes: snapshot.inbound_bytes,
                        audio_level: snapshot.audio_level,
                        decoded_rms,
                        tone_ratio: ratio,
                    });
                    control.send(CallUpdate::Stats {
                        inbound_packets: snapshot.inbound_packets,
                        outbound_packets: snapshot.outbound_packets,
                    });
                    timeline.record(
                        "stats",
                        format!(
                            "in packets={} bytes={} level={:.3} rms={decoded_rms:.3} tone={ratio:.2} out packets={} mic={:.3} erle={:.1} dtls={}",
                            snapshot.inbound_packets, snapshot.inbound_bytes, snapshot.audio_level, snapshot.outbound_packets, snapshot.local_level, snapshot.echo_return_loss_enhancement, snapshot.dtls_state
                        ),
                    );
                }
            }
            _ = sleep_until(reconnect_deadline.unwrap_or_else(far_future)), if reconnect_deadline.is_some() => {
                advance(state, CallSignal::ReconnectTimedOut, control);
                break;
            }
            _ = sleep_until(leave_at.unwrap_or_else(far_future)), if leave_at.is_some() => {
                advance(state, CallSignal::LocalLeave, control);
                break;
            }
        }
    }
    Ok(())
}

fn far_future() -> Instant {
    Instant::now() + FAR_FUTURE
}

fn apply_mute(command: MuteCommand, currently_muted: bool, audio: &AudioSetup, remote: &RemoteCall, control: &CallControl) -> bool {
    let effect = MuteEffect::for_muted(command.target(currently_muted));
    audio.track.set_enabled(effect.track_enabled);
    control.send(CallUpdate::Muted(effect.muted));
    let remote = remote.clone();
    tokio::spawn(async move {
        let _ = remote
            .signaling
            .update_endpoint_state(&remote.conversation, &remote.participant, effect.endpoint_is_muted)
            .await;
    });
    effect.muted
}

async fn tear_down(mut live: Live, timeline: &Timeline, report: &mut TestCallReport) {
    if let Some(audio) = &live.audio {
        audio.track.set_enabled(false);
    }
    live.broker_running.store(false, Ordering::SeqCst);
    if let Some(remote) = &live.remote {
        match remote.signaling.leave(&remote.conversation, &remote.participant).await {
            Ok(()) => report.left_cleanly = true,
            Err(error) => timeline.record("leave failed", error.to_string()),
        }
    }
    if report.end.is_none()
        && let (Some(callbacks), Some(replier)) = (live.callbacks.as_mut(), live.replier.as_ref())
        && live.remote.is_some()
    {
        let waited = timeout(END_CALLBACK_WAIT, async {
            while let Some(callback) = callbacks.recv().await {
                let request_id = callback.request_id;
                let event = handle_callback(&callback, timeline);
                replier.reply(request_id, 200, "");
                if let Ok(CallEvent::End(end)) = event {
                    return Some(end);
                }
            }
            None
        })
        .await;
        report.end = waited.ok().flatten();
    }
    if let Some(peer) = &live.peer {
        peer.close();
    }
    if let Some(audio) = live.audio.take() {
        audio.stop().await;
    }
    if let Some(broker) = live.broker.take() {
        broker.abort();
    }
    if let Some(realtime) = live.realtime.take() {
        realtime.stop().await;
    }
    timeline.record("closed", "peer closed, Trouter registration removed");
}

async fn apply_answer(peer: &PeerConnection, teams_answer: &str, signaled: &SignaledOffer, browser_offer: &str) -> Result<()> {
    let answer = to_browser_answer(teams_answer, signaled, browser_offer)?;
    let description =
        SessionDescription::parse(&answer, SdpType::Answer).map_err(|error| Error::Sdp(error.description))?;
    peer.set_remote_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))
}

/// Writes an SDP with ICE credentials, fingerprints and addresses masked, for debugging the dialect.
fn dump_sdp(options: &TestCallOptions, name: &str, sdp: &str) {
    let Some(directory) = &options.sdp_dump_dir else {
        return;
    };
    let masked: Vec<String> = sdp
        .lines()
        .map(|line| {
            for prefix in ["a=ice-ufrag:", "a=ice-pwd:", "a=fingerprint:"] {
                if line.starts_with(prefix) {
                    return format!("{prefix}<masked>");
                }
            }
            line.split(' ').map(mask_address).collect::<Vec<_>>().join(" ")
        })
        .collect();
    let _ = std::fs::create_dir_all(directory);
    let _ = std::fs::write(directory.join(name), masked.join("\n"));
}

fn mask_address(token: &str) -> String {
    let dotted_quad = token.split('.').count() == 4 && token.split('.').all(|part| part.parse::<u8>().is_ok());
    let colon_address = token.matches(':').count() >= 2 && !token.starts_with("a=") && !token.starts_with("urn:");
    if (dotted_quad && token != "10.10.10.10" && token != "0.0.0.0" && token != "127.0.0.1") || colon_address {
        "<addr>".to_owned()
    } else {
        token.to_owned()
    }
}

fn handle_callback(callback: &TrouterCallback, timeline: &Timeline) -> Result<CallEvent> {
    let body = decode_body(callback)?;
    let (_, event) = classify(&callback.path, body)?;
    let detail = match &event {
        CallEvent::Acceptance(acceptance) => format!("answer sdp {} bytes, fromMixer={}", acceptance.sdp.len(), acceptance.from_mixer),
        CallEvent::End(end) => format!("code={} subCode={} phrase={}", end.code, end.sub_code, end.phrase),
        _ => String::new(),
    };
    timeline.record(format!("Trouter {}", event.name()), detail);
    Ok(event)
}

async fn record_track(track: RtcAudioTrack, recording: Arc<Mutex<Recording>>, tone_hz: f32) {
    let mut stream = NativeAudioStream::new(track, SAMPLE_RATE as i32, 1);
    while let Some(frame) = stream.next().await {
        let mut recording = recording.lock().await;
        recording.samples.extend_from_slice(&frame.data);
        recording.current_second.extend_from_slice(&frame.data);
        if recording.current_second.len() >= SAMPLE_RATE as usize {
            let second: Vec<i16> = std::mem::take(&mut recording.current_second);
            let summary = (rms(&second), tone_ratio(&second, tone_hz));
            recording.per_second.push(summary);
        }
    }
}

async fn poll_broker(signaling: Arc<Signaling>, conversation: Conversation, running: Arc<AtomicBool>) {
    let Ok(first) = conversation.link("subscribe") else {
        return;
    };
    let mut url = first.to_owned();
    while running.load(Ordering::SeqCst) {
        match signaling.poll_broker(&url).await {
            Ok(Some(next)) => url = next,
            Ok(None) => {}
            Err(_) => return,
        }
    }
}

#[derive(Debug, Default)]
struct StatsSnapshot {
    inbound_packets: u64,
    inbound_bytes: u64,
    outbound_packets: u64,
    audio_level: f64,
    local_level: f64,
    echo_return_loss_enhancement: f64,
    dtls_state: String,
    tls_version: String,
    dtls_cipher: String,
    srtp_cipher: String,
    selected_pair: Option<String>,
}

async fn collect_stats(peer: &PeerConnection) -> StatsSnapshot {
    let Ok(stats) = peer.get_stats().await else {
        return StatsSnapshot::default();
    };
    let mut snapshot = StatsSnapshot::default();
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
                snapshot.echo_return_loss_enhancement = source.audio.echo_return_loss_enhancement;
            }
            RtcStats::OutboundRtp(outbound) if outbound.stream.kind == "audio" => {
                snapshot.outbound_packets += outbound.sent.packets_sent;
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
                        RtcStats::LocalCandidate(local) if local.rtc.id == candidate_id => Some(&local.local_candidate),
                        RtcStats::RemoteCandidate(remote) if remote.rtc.id == candidate_id => Some(&remote.remote_candidate),
                        _ => None,
                    })
                    .map(|candidate| {
                        let kind = candidate.candidate_type.map(|kind| format!("{kind:?}")).unwrap_or_default();
                        let relay = candidate.relay_protocol.map(|relay| format!(" via {relay:?}")).unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_base_comes_from_the_presence_uri() {
        assert_eq!(
            callback_base("https://pub-ent-euwe-02-f.trouter.teams.microsoft.com:3443/v4/f/abc//unifiedPresenceService"),
            "https://pub-ent-euwe-02-f.trouter.teams.microsoft.com:3443/v4/f/abc/"
        );
        assert_eq!(callback_base("https://h/v4/f/abc/unifiedPresenceService"), "https://h/v4/f/abc/");
    }
}
