use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chatsvc::TrouterCallback;
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
use crate::control::{CallCommand, CallControl, CallUpdate, Progress};
use crate::devices::{self, DeviceChoice};
use crate::end::EndKind;
use crate::engine::{CallEngine, EngineConfig, Inner};
use crate::error::{Error, Result};
use crate::mute::{MuteCommand, MuteEffect};
use crate::push::CallNotification;
use crate::share_audio::ShareAudio;
use crate::relay::{DEFAULT_RELAY_HOST, RelayGrant, fetch_relay_grant};
use crate::renegotiation::{MediaAction, MediaNegotiator};
use crate::roster::Roster;
use crate::sdp::{RemoteOffer, SignaledOffer, from_teams_offer, stream_lines, to_browser_answer, to_teams_answer, to_teams_offer};
use crate::signaling::{
    Answer, Attached, Callee, Conversation, Invitation, InviteTarget, LeaveReason, MeetingTarget, Participant, Signaling,
    TenantRouting, chat_thread_id, find_echo_bot_thread, escalation_answer_body, renegotiation_body,
};
use crate::state::{CallSignal, CallState};
use crate::timeline::{Timeline, TimelineEntry};
use crate::video_layout::{MediaDescription, SlotTable, VideoLines, VideoRouter, add_video_lines, offer_plan};
use crate::video_send::{LocalVideo, VideoMode};
use crate::camera::{CameraDevice, list_cameras};
use crate::screen::{ShareKind, ShareSource, list_share_sources};
use crate::video_receive::{ReceiveChange, SourceRequester, VideoCounters, VideoReceive, spawn_video_pump};
use crate::trouter_events::{
    CallEnd, CallEvent, CallbackLinks, ProgressStatus, acceptance_acknowledgement, classify, decode_body,
};

const END_CALLBACK_WAIT: Duration = Duration::from_secs(4);
const STATS_PERIOD: Duration = Duration::from_millis(200);
const TICKS_PER_SECOND: u32 = 5;
const RECONNECT_WINDOW: Duration = Duration::from_secs(30);
const OUTGOING_TIMEOUT: Duration = Duration::from_secs(90);
const FAR_FUTURE: Duration = Duration::from_secs(60 * 60 * 24 * 365);
const DEFAULT_KEEP_ALIVE_SECONDS: u64 = 2700;
const MIN_KEEP_ALIVE_SECONDS: u64 = 60;
const KEEP_ALIVE_FRACTION: f64 = 0.9;
const MIC_STREAM: &str = "microphone";

#[derive(Debug, Clone)]
pub struct CallOptions {
    pub hold_after_connected: Option<Duration>,
    pub hard_limit: Option<Duration>,
    pub reconnect_window: Duration,
    pub audio: AudioMode,
    pub video: VideoMode,
    pub camera: DeviceChoice,
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

impl Default for CallOptions {
    fn default() -> Self {
        CallOptions {
            hold_after_connected: None,
            hard_limit: None,
            reconnect_window: RECONNECT_WINDOW,
            audio: AudioMode::from_env(),
            video: VideoMode::from_env(),
            camera: DeviceChoice::SystemDefault,
            tone_hz: 440.0,
            input: DeviceChoice::SystemDefault,
            output: DeviceChoice::SystemDefault,
            relay_host: DEFAULT_RELAY_HOST.to_owned(),
            routing: TenantRouting::default(),
            record_remote: false,
            wav_path: None,
            sdp_dump_dir: None,
            trace: std::env::var_os(crate::engine::TRACE_ENV).is_some(),
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoReport {
    pub inbound_packets: u64,
    pub inbound_frames_decoded: u64,
    pub inbound_width: u32,
    pub inbound_height: u32,
    pub frames_received: u64,
    pub frames_published: u64,
    pub outbound_packets: u64,
    pub outbound_frames_encoded: u64,
    pub outbound_width: u32,
    pub outbound_height: u32,
    pub remote_reports: u64,
    pub remote_round_trip_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub struct CallReport {
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
    pub video: VideoReport,
    pub own_streams: Vec<String>,
    pub seconds_with_inbound_audio: u32,
    pub seconds: Vec<AudioSecond>,
    pub end: Option<CallEnd>,
    pub left_cleanly: bool,
    pub recorded_samples: usize,
}

#[derive(Debug, Clone)]
pub enum CallSpec {
    Echo,
    People { callees: Vec<Callee>, thread_id: String },
    Meeting(MeetingTarget),
}

impl CallSpec {
    fn target(&self) -> InviteTarget {
        match self {
            CallSpec::Echo => InviteTarget::Echo,
            CallSpec::People { callees, thread_id } => InviteTarget::People {
                callees: callees.clone(),
                thread_id: thread_id.clone(),
            },
            CallSpec::Meeting(meeting) => InviteTarget::Meeting(meeting.clone()),
        }
    }

    fn is_meeting(&self) -> bool {
        matches!(self, CallSpec::Meeting(_))
    }

    fn is_people(&self) -> bool {
        matches!(self, CallSpec::People { .. })
    }
}

pub(crate) struct IncomingCall {
    pub notification: CallNotification,
    pub attached: Attached,
    pub links: CallbackLinks,
    pub participant: Participant,
    pub callbacks: mpsc::UnboundedReceiver<TrouterCallback>,
}

pub(crate) enum Plan {
    Outgoing(CallSpec),
    Incoming(Box<IncomingCall>),
}

pub fn keep_alive_period(seconds: Option<u64>) -> Duration {
    let seconds = seconds.unwrap_or(DEFAULT_KEEP_ALIVE_SECONDS).max(MIN_KEEP_ALIVE_SECONDS);
    Duration::from_secs_f64(seconds as f64 * KEEP_ALIVE_FRACTION)
}

fn random_u32() -> u32 {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) & 0x7fff_ffff
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
    VideoTrack { mid: String, track: RtcVideoTrack },
}

struct PeerSession {
    peer: PeerConnection,
    events: mpsc::UnboundedReceiver<PeerEvent>,
}

struct OfferedMedia {
    browser_offer: String,
    signaled: SignaledOffer,
    video: VideoLines,
}

struct NextPeer {
    session: PeerSession,
    offered: Option<OfferedMedia>,
}

struct AnsweredMedia {
    remote: RemoteOffer,
    answer_sdp: String,
}

#[derive(Clone)]
struct RemoteCall {
    signaling: Arc<Signaling>,
    conversation: Conversation,
    participant: Participant,
}

#[derive(Default)]
struct Live {
    peers: Vec<PeerConnection>,
    audio: Option<AudioSetup>,
    remote: Option<RemoteCall>,
    broker: Option<JoinHandle<()>>,
    broker_running: Arc<AtomicBool>,
    callbacks: Option<mpsc::UnboundedReceiver<TrouterCallback>>,
    route_id: Option<String>,
    skip_leave: bool,
    cancel_leave: bool,
    video_tasks: Vec<JoinHandle<()>>,
    video_counters: Arc<VideoCounters>,
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

/// Places the Echo test call on its own non-ringable Trouter instance; the app uses a shared engine instead.
pub async fn run_test_call(
    session: &Session,
    poll_session: &Session,
    options: CallOptions,
    control: CallControl,
) -> Result<CallReport> {
    let config = EngineConfig {
        ringable: false,
        routing: options.routing.clone(),
        relay_host: options.relay_host.clone(),
        trace: options.trace,
        instance: chatsvc::InstanceNames {
            global: "__testCallTrouter".into(),
            binding: "__testCallRealtime".into(),
            endpoint_storage_key: "__testCallEpid".into(),
        },
    };
    let (engine, _events) = CallEngine::start(session.clone(), poll_session.clone(), config).await?;
    let report = engine.run(Plan::Outgoing(CallSpec::Echo), options, control).await;
    engine.stop().await;
    report
}

impl CallEngine {
    pub(crate) async fn run(&self, plan: Plan, options: CallOptions, mut control: CallControl) -> Result<CallReport> {
        let inner = self.inner.as_ref();
        let _one_call_at_a_time = inner.gate.lock().await;
        let timeline = Timeline::new(options.trace);
        let mut report = CallReport::default();
        let mut state = CallState::Idle;
        advance(&mut state, CallSignal::Dial, &control);
        let mut live = Live::default();
        let recording = Arc::new(Mutex::new(Recording::default()));
        let outcome = drive(
            inner,
            &options,
            &timeline,
            &recording,
            plan,
            &mut live,
            &mut control,
            &mut state,
            &mut report,
        )
        .await;
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
        let cancelled = live.cancel_leave || matches!(&state, CallState::Ended { reason: crate::state::EndReason::Cancelled });
        tear_down(live, inner, &timeline, &mut report, cancelled).await;
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
}

fn open_peer(factory: &PeerConnectionFactory, grant: &RelayGrant, relay_host: &str) -> Result<PeerSession> {
    let mut configuration = RtcConfiguration::default();
    configuration.ice_servers = vec![grant.ice_server(relay_host)];
    let peer = factory
        .create_peer_connection(configuration)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let (sender, events) = mpsc::unbounded_channel();
    let ice_sender = sender.clone();
    peer.on_ice_connection_state_change(Some(Box::new(move |state| {
        let _ = ice_sender.send(PeerEvent::Ice(state));
    })));
    let connection_sender = sender.clone();
    peer.on_connection_state_change(Some(Box::new(move |state| {
        let _ = connection_sender.send(PeerEvent::Connection(state));
    })));
    peer.on_track(Some(Box::new(move |event| match event.track {
        MediaStreamTrack::Audio(track) => {
            let _ = sender.send(PeerEvent::Track(track));
        }
        MediaStreamTrack::Video(track) => {
            let mid = event.transceiver.mid().unwrap_or_default();
            let _ = sender.send(PeerEvent::VideoTrack { mid, track });
        }
    })));
    Ok(PeerSession { peer, events })
}

async fn create_offer(peer: &PeerConnection, factory: &PeerConnectionFactory, track: &RtcAudioTrack) -> Result<OfferedMedia> {
    let transceiver_init = RtpTransceiverInit {
        direction: RtpTransceiverDirection::SendRecv,
        stream_ids: vec![MIC_STREAM.into()],
        send_encodings: Vec::new(),
    };
    peer.add_transceiver(MediaStreamTrack::Audio(track.clone()), transceiver_init)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let video = add_video_lines(peer, factory)?;
    let offer = peer
        .create_offer(OfferOptions {
            offer_to_receive_audio: true,
            offer_to_receive_video: true,
            ..OfferOptions::default()
        })
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    peer.set_local_description(offer.clone())
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let browser_offer = offer.to_string();
    let mut numbers = random_u32;
    let signaled = to_teams_offer(&browser_offer, &offer_plan(), &mut numbers)?;
    Ok(OfferedMedia { browser_offer, signaled, video })
}

/// Video lines of a Teams offer are stopped so the answer rejects them; audio is all this client does.
async fn answer_offer(
    peer: &PeerConnection,
    track: Option<&RtcAudioTrack>,
    teams_offer: &str,
) -> Result<AnsweredMedia> {
    let remote = from_teams_offer(teams_offer)?;
    let description =
        SessionDescription::parse(&remote.browser_sdp, SdpType::Offer).map_err(|error| Error::Sdp(error.description))?;
    peer.set_remote_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    for transceiver in peer.transceivers() {
        if matches!(transceiver.receiver().track(), Some(MediaStreamTrack::Video(_))) {
            let _ = transceiver.stop();
        }
    }
    if let Some(track) = track {
        peer.add_track(MediaStreamTrack::Audio(track.clone()), &[MIC_STREAM])
            .map_err(|error| Error::Webrtc(error.to_string()))?;
    }
    let answer = peer
        .create_answer(AnswerOptions::default())
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    peer.set_local_description(answer.clone())
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let mut numbers = random_u32;
    let signaled = to_teams_answer(&answer.to_string(), &remote, &mut numbers)?;
    Ok(AnsweredMedia {
        remote,
        answer_sdp: signaled.sdp,
    })
}

fn new_media_leg_id() -> String {
    uuid::Uuid::new_v4().simple().to_string().to_uppercase()
}

async fn next_peer_event(next: &mut Option<NextPeer>) -> Option<PeerEvent> {
    match next {
        Some(next) => next.session.events.recv().await,
        None => std::future::pending().await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    inner: &Inner,
    options: &CallOptions,
    timeline: &Timeline,
    recording: &Arc<Mutex<Recording>>,
    plan: Plan,
    live: &mut Live,
    control: &mut CallControl,
    state: &mut CallState,
    report: &mut CallReport,
) -> Result<()> {
    let own_mri = format!("8:orgid:{}", inner.identity.object_id);
    control.send(CallUpdate::OwnIdentity { mri: own_mri.clone() });
    let (spec, incoming) = match plan {
        Plan::Outgoing(spec) => (Some(spec), None),
        Plan::Incoming(incoming) => (None, Some(*incoming)),
    };
    let (links, participant, mut callbacks, incoming_media) = match incoming {
        Some(IncomingCall {
            notification,
            attached,
            links,
            participant,
            callbacks,
        }) => (links, participant, callbacks, Some((notification, attached))),
        None => {
            let links = inner.callback_links();
            let participant = inner.participant(&uuid::Uuid::new_v4().to_string());
            let callbacks = inner.register_route(links.call_id());
            (links, participant, callbacks, None)
        }
    };
    live.route_id = Some(links.call_id().to_owned());
    timeline.record("Trouter connected", "callback socket ready");

    let thread_id = if matches!(spec, Some(CallSpec::Echo)) {
        let thread_id = control
            .until_hangup(find_echo_bot_thread(&inner.session, &inner.identity.object_id))
            .await??;
        timeline.record("echo bot chat", if thread_id.is_some() { "found" } else { "none" });
        thread_id
    } else {
        None
    };
    let grant = control.until_hangup(fetch_relay_grant(&inner.session)).await??;
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
    let mut local_video = LocalVideo::new(&factory, options.video, control.video.clone(), control.updates());
    let mut camera_choice = options.camera.clone();
    let mut media_description_request = 1u32;
    control.send(CallUpdate::Cameras(camera_devices(options.video).await));
    refresh_share_sources(control.updates(), options.video);

    let mut current = open_peer(&factory, &grant, &options.relay_host)?;
    live.peers.push(current.peer.clone());

    let signaling = Arc::new(inner.signaling(timeline.clone()));
    let media_leg_id = new_media_leg_id();
    let call_started = Instant::now();
    let mut offered: Option<OfferedMedia> = None;
    let mut negotiator = MediaNegotiator::default();
    let mut remote_set = false;
    let mut keep_alive_link: Option<String> = None;
    let mut keep_alive_seconds: Option<u64> = None;
    let mut keep_alive_at: Option<Instant> = None;
    let mut outgoing_deadline: Option<Instant> = None;
    let is_meeting = spec.as_ref().is_some_and(CallSpec::is_meeting);
    let is_echo = matches!(spec, Some(CallSpec::Echo));

    let conversation = match (spec, incoming_media) {
        (Some(spec), None) => {
            let media = create_offer(&current.peer, &factory, &track).await?;
            timeline.record("setLocalDescription", format!("{} m-line(s) signaled", media.signaled.lines.len()));
            dump_sdp(options, "1-browser-offer.sdp", &media.browser_offer);
            dump_sdp(options, "2-teams-offer.sdp", &media.signaled.sdp);
            let target = spec.target();
            let invitation = Invitation {
                from: &participant,
                offer_sdp: &media.signaled.sdp,
                media_leg_id: &media_leg_id,
                callbacks: &links,
                target: &target,
            };
            let conversation = match &target {
                InviteTarget::Meeting(meeting) => {
                    let subscribed = control
                        .until_hangup(signaling.subscribe_meeting(&participant, &links, meeting))
                        .await??;
                    signaling.join_meeting(&subscribed, &invitation).await?
                }
                _ => signaling.create_conversation(&invitation).await?,
            };
            if spec.is_people() {
                control.send(CallUpdate::Progress(Progress::Calling));
                outgoing_deadline = Some(Instant::now() + OUTGOING_TIMEOUT);
            }
            if let Err(error) = local_video.set_lines(media.video.clone()) {
                timeline.record("video lines not attached", error.to_string());
            }
            offered = Some(media);
            live.remote = Some(RemoteCall {
                signaling: signaling.clone(),
                conversation: conversation.clone(),
                participant: participant.clone(),
            });
            conversation
        }
        (None, Some((_notification, attached))) => {
            let teams_offer = attached
                .offer_sdp
                .clone()
                .ok_or_else(|| Error::Signaling("attach answer without an offer".into()))?;
            let conversation = attached
                .conversation
                .clone()
                .ok_or_else(|| Error::Signaling("attach answer without a conversation".into()))?;
            live.remote = Some(RemoteCall {
                signaling: signaling.clone(),
                conversation: conversation.clone(),
                participant: participant.clone(),
            });
            let answered = answer_offer(&current.peer, Some(&track), &teams_offer).await?;
            timeline.record("setRemoteDescription", format!("offer applied, {} signaled line(s)", answered.remote.lines.len()));
            dump_sdp(options, "3-teams-offer.sdp", &teams_offer);
            dump_sdp(options, "4-teams-answer.sdp", &answered.answer_sdp);
            let acceptance_url = attached
                .links
                .get("acceptance")
                .ok_or_else(|| Error::Signaling("attach answer without an acceptance link".into()))?;
            let modalities = vec!["Audio".to_owned()];
            signaling
                .accept(
                    acceptance_url,
                    &Answer {
                        from: &participant,
                        answer_sdp: &answered.answer_sdp,
                        media_leg_id: &media_leg_id,
                        callbacks: &links,
                        modalities: &modalities,
                    },
                )
                .await?;
            negotiator.answered_incoming(attached.from_mixer);
            remote_set = true;
            keep_alive_link = attached.links.get("callLeg").cloned();
            keep_alive_at = Some(Instant::now() + keep_alive_period(None));
            conversation
        }
        _ => return Err(Error::Callback("call plan without an offer or an invitation".into())),
    };
    let remote = live.remote.clone().expect("remote call set above");
    live.broker_running.store(true, Ordering::SeqCst);
    live.broker = Some(tokio::spawn(poll_broker(
        signaling.clone(),
        conversation.clone(),
        live.broker_running.clone(),
    )));

    let initial = MuteEffect::initial(is_meeting);
    let mut muted = initial.muted;
    live.audio.as_ref().expect("audio set above").track.set_enabled(initial.track_enabled);
    if muted {
        control.send(CallUpdate::Muted(true));
    }
    if let Err(error) = signaling.update_endpoint_state(&conversation, &participant, initial.endpoint_is_muted).await {
        timeline.record("updateEndpointState failed", error.to_string());
    }

    let replier = inner.replier.clone();
    let mut stats_tick = interval(STATS_PERIOD);
    let hard_deadline = options.hard_limit.map(|limit| call_started + limit);
    let mut leave_at = hard_deadline;
    let mut reconnect_deadline: Option<Instant> = None;
    let mut next: Option<NextPeer> = None;
    let mut roster = Roster::default();
    let mut lobby = false;
    let mut previous_packets = 0u64;
    let mut ticks = 0u32;
    let mut second = 0u32;
    let echo_thread = thread_id;
    let mut video = VideoReceive::new(own_mri.clone(), VideoRouter::default());
    let mut media_links: BTreeMap<String, String> = BTreeMap::new();
    let mut share_audio: Option<ShareAudio> = None;
    let mut share_sound_wanted = false;
    let mut remote_audio: Option<RtcAudioTrack> = None;
    let mut own_hand: Option<String> = None;
    let mut meeting_chat = conversation.chat_thread.clone();
    if let Some(thread) = &meeting_chat {
        control.send(CallUpdate::MeetingChat(thread.clone()));
    }
    loop {
        tokio::select! {
            Some(callback) = callbacks.recv() => {
                let request_id = callback.request_id;
                match handle_callback(&callback, timeline) {
                    Ok(CallEvent::Acceptance(acceptance)) => {
                        replier.reply(request_id, 200, acceptance_acknowledgement(&links).to_string());
                        match negotiator.on_acceptance(acceptance.from_mixer) {
                            MediaAction::ApplyAnswer => {
                                let Some(media) = offered.as_ref() else { continue };
                                dump_sdp(options, "3-teams-answer.sdp", &acceptance.sdp);
                                if let Ok(answer) = to_browser_answer(&acceptance.sdp, &media.signaled, &media.browser_offer) {
                                    dump_sdp(options, "4-browser-answer.sdp", &answer);
                                }
                                apply_answer(&current.peer, &acceptance.sdp, &media.signaled, &media.browser_offer)
                                    .await
                                    .inspect_err(|error| timeline.record("answer rejected", error.to_string()))?;
                                remote_set = true;
                                media_links.extend(acceptance.links.clone());
                                negotiate_video(&mut video, &signaling, control, timeline, &acceptance.sdp, media, acceptance.from_mixer, &media_links, &roster);
                                outgoing_deadline = None;
                                let directions: Vec<String> = current
                                    .peer
                                    .transceivers()
                                    .iter()
                                    .map(|transceiver| format!("{:?}", transceiver.current_direction()))
                                    .collect();
                                timeline.record("setRemoteDescription", format!("answer applied, directions {}", directions.join(",")));
                                if acceptance.in_lobby() {
                                    lobby = true;
                                    control.send(CallUpdate::Lobby(true));
                                }
                                keep_alive_link = acceptance.links.get("callLeg").cloned();
                                keep_alive_seconds = acceptance.keep_alive_seconds;
                                keep_alive_at = Some(Instant::now() + keep_alive_period(keep_alive_seconds));
                                let remote = remote.clone();
                                let links = links.clone();
                                let thread_id = echo_thread.clone();
                                tokio::spawn(async move {
                                    if is_echo {
                                        let _ = remote.signaling.add_echo_bot(&remote.conversation, &remote.participant, thread_id.as_deref(), &links).await;
                                    }
                                    let _ = remote.signaling.update_endpoint_metadata(&remote.conversation, &remote.participant).await;
                                });
                            }
                            MediaAction::Escalate => {
                                match start_escalation(&factory, &grant, options, &track, &remote, &links, &media_leg_id, &acceptance.links).await {
                                    Ok(pending) => {
                                        timeline.record("escalation", "new peer connection, new offer sent");
                                        live.peers.push(pending.session.peer.clone());
                                        next = Some(pending);
                                    }
                                    Err(error) => {
                                        negotiator.escalation_failed();
                                        timeline.record("escalation failed", error.to_string());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(CallEvent::MediaAnswer(answer)) => {
                        replier.reply(request_id, 200, "");
                        if negotiator.on_media_answer() == MediaAction::ApplyOnNewPeer {
                            let applied = match next.as_ref().and_then(|pending| Some((pending, pending.offered.as_ref()?))) {
                                Some((pending, media)) => apply_answer(&pending.session.peer, &answer.sdp, &media.signaled, &media.browser_offer).await,
                                None => Err(Error::Webrtc("no pending peer connection for the answer".into())),
                            };
                            match applied {
                                Ok(()) => {
                                    if let Some(media) = next.as_ref().and_then(|pending| pending.offered.as_ref()) {
                                        media_links.extend(answer.links.clone());
                                        negotiate_video(&mut video, &signaling, control, timeline, &answer.sdp, media, true, &media_links, &roster);
                                    }
                                    if let Some(url) = answer.links.get("mediaAcknowledgement") {
                                        let _ = remote.signaling.post_empty("POST mediaAcknowledgement", url).await;
                                    }
                                }
                                Err(error) => {
                                    negotiator.escalation_failed();
                                    next = None;
                                    timeline.record("escalation answer rejected", error.to_string());
                                }
                            }
                        }
                    }
                    Ok(CallEvent::MediaRenegotiation(renegotiation)) => {
                        replier.reply(request_id, 200, "");
                        let action = negotiator.on_renegotiation(renegotiation.new_offer, renegotiation.escalation);
                        let result = match action {
                            MediaAction::AnswerOnNewPeer => {
                                match answer_on_new_peer(&factory, &grant, options, &track, &renegotiation.sdp).await {
                                    Ok((session, answered)) => {
                                        live.peers.push(session.peer.clone());
                                        let sent = send_media_answer(&remote, &links, &renegotiation.links, &answered, &media_leg_id).await;
                                        next = Some(NextPeer { session, offered: None });
                                        sent
                                    }
                                    Err(error) => Err(error),
                                }
                            }
                            MediaAction::AnswerOnSamePeer => {
                                match answer_offer(&current.peer, None, &renegotiation.sdp).await {
                                    Ok(answered) => send_media_answer(&remote, &links, &renegotiation.links, &answered, &media_leg_id).await,
                                    Err(error) => Err(error),
                                }
                            }
                            _ => Ok(()),
                        };
                        if let Err(error) = result {
                            timeline.record("renegotiation failed", error.to_string());
                        }
                    }
                    Ok(CallEvent::RosterUpdate(body)) => {
                        replier.reply(request_id, 200, "");
                        if roster.apply(&body) {
                            let entries = roster.entries();
                            timeline.record("roster", format!("{} participant(s)", entries.len()));
                            control.send(CallUpdate::Roster(entries));
                            let own_streams = roster.stream_summaries(&own_mri);
                            if own_streams != report.own_streams {
                                timeline.record("own media streams", own_streams.join("; "));
                                report.own_streams = own_streams;
                            }
                            send_receive_change(control, video.reselect(&roster));
                            if let Some(in_lobby) = roster.is_in_lobby(&own_mri)
                                && in_lobby != lobby
                            {
                                lobby = in_lobby;
                                control.send(CallUpdate::Lobby(lobby));
                            }
                        }
                    }
                    Ok(CallEvent::ConversationUpdate(body)) => {
                        replier.reply(request_id, 200, "");
                        if let Some(thread) = chat_thread_id(&body)
                            && meeting_chat.as_ref() != Some(&thread)
                        {
                            control.send(CallUpdate::MeetingChat(thread.clone()));
                            meeting_chat = Some(thread);
                        }
                    }
                    Ok(CallEvent::Reactions(events)) => {
                        replier.reply(request_id, 200, "");
                        for event in events {
                            control.send(CallUpdate::Reaction { mri: event.mri, reaction: event.reaction });
                        }
                    }
                    Ok(CallEvent::Progress(status)) => {
                        replier.reply(request_id, 200, "");
                        if status == ProgressStatus::Ringing {
                            control.send(CallUpdate::Progress(Progress::Ringing));
                        }
                    }
                    Ok(CallEvent::Speakers(sources)) => {
                        replier.reply(request_id, 200, "");
                        let speakers = roster.mris_for_sources(&sources);
                        if video.note_speakers(&speakers) {
                            send_receive_change(control, video.reselect(&roster));
                        }
                        control.send(CallUpdate::Speakers(speakers));
                    }
                    Ok(CallEvent::End(end)) => {
                        replier.reply(request_id, 200, "");
                        let kind = end.kind();
                        report.end = Some(end);
                        advance(state, CallSignal::RemoteEnd(kind), control);
                        break;
                    }
                    Ok(_) => replier.reply(request_id, 200, ""),
                    Err(error) => {
                        replier.reply(request_id, 200, "");
                        timeline.record("callback not understood", error.to_string());
                    }
                }
            }
            Some(event) = current.events.recv() => match event {
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
                PeerEvent::Track(remote_track) => {
                    timeline.record("remote audio track", "receiving");
                    remote_audio = Some(remote_track.clone());
                    if options.record_remote {
                        tokio::spawn(record_track(remote_track, recording.clone(), options.tone_hz));
                    }
                }
                PeerEvent::VideoTrack { mid, track } => {
                    timeline.record("remote video track", format!("mid {mid}"));
                    live.video_tasks.push(spawn_video_pump(track, mid, video.router().clone(), control.video.clone(), live.video_counters.clone()));
                }
            },
            Some(event) = next_peer_event(&mut next) => match event {
                PeerEvent::Connection(PeerConnectionState::Connected) => {
                    if let Some(pending) = next.take() {
                        timeline.record("escalation", "new peer connection connected, old one dropped");
                        let old = std::mem::replace(&mut current, pending.session);
                        old.peer.close();
                        if let Some(audio) = &share_audio
                            && let Err(error) = swap_audio_track(&current.peer, &audio.track)
                        {
                            timeline.record("computer sound not carried over", error.to_string());
                        }
                        offered = pending.offered;
                        if let Some(media) = &offered
                            && let Err(error) = local_video.set_lines(media.video.clone())
                        {
                            timeline.record("video lines not attached", error.to_string());
                        }
                        remote_set = true;
                    }
                }
                PeerEvent::VideoTrack { mid, track } => {
                    timeline.record("remote video track", format!("mid {mid} (new peer)"));
                    live.video_tasks.push(spawn_video_pump(track, mid, video.router().clone(), control.video.clone(), live.video_counters.clone()));
                }
                PeerEvent::Connection(PeerConnectionState::Failed) => {
                    negotiator.escalation_failed();
                    if let Some(pending) = next.take() {
                        pending.session.peer.close();
                    }
                    timeline.record("escalation failed", "new peer connection failed, staying on the old one");
                }
                _ => {}
            },
            command = control.recv() => match command {
                Some(CallCommand::Mute(mute)) => {
                    muted = apply_mute(mute, muted, live.audio.as_ref().expect("audio set above"), share_audio.as_ref(), &remote, control);
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
                Some(CallCommand::SetCamera(on)) => {
                    if !remote_set {
                        control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                    } else if on {
                        match local_video.start_camera(&camera_choice) {
                            Ok(()) => {
                                control.send(CallUpdate::Camera(true));
                                tell_media_descriptions(&remote, &media_links, MediaDescription::camera(true), &mut media_description_request, timeline);
                            }
                            Err(error) => control.send(CallUpdate::Notice(format!("Camera could not start: {error}"))),
                        }
                    } else {
                        local_video.stop_camera();
                        control.send(CallUpdate::Camera(false));
                        tell_media_descriptions(&remote, &media_links, MediaDescription::camera(false), &mut media_description_request, timeline);
                    }
                }
                Some(CallCommand::SelectCamera(choice)) => {
                    camera_choice = choice;
                    if local_video.camera_on()
                        && let Err(error) = local_video.start_camera(&camera_choice)
                    {
                        control.send(CallUpdate::Notice(format!("Camera could not start: {error}")));
                    }
                }
                Some(CallCommand::StartShare(source)) => {
                    if !remote_set {
                        control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                    } else {
                        match local_video.start_share(&source) {
                            Ok(()) => {
                                control.send(CallUpdate::LocalShare(Some(source.label())));
                                tell_media_descriptions(&remote, &media_links, MediaDescription::share(true), &mut media_description_request, timeline);
                                if share_sound_wanted && share_audio.is_none() {
                                    share_audio = engage_share_sound(&factory, options, &input, muted, remote_audio.clone(), &current.peer, control, timeline);
                                }
                            }
                            Err(error) => control.send(CallUpdate::Notice(format!("Sharing could not start: {error}"))),
                        }
                    }
                }
                Some(CallCommand::StopShare) => {
                    if local_video.sharing() {
                        local_video.stop_share();
                        release_share_sound(&current.peer, &track, &mut share_audio, control, timeline);
                        control.send(CallUpdate::LocalShare(None));
                        tell_media_descriptions(&remote, &media_links, MediaDescription::share(false), &mut media_description_request, timeline);
                    }
                }
                Some(CallCommand::SetShareSound(on)) => {
                    share_sound_wanted = on;
                    if local_video.sharing() {
                        if on && share_audio.is_none() {
                            share_audio = engage_share_sound(&factory, options, &input, muted, remote_audio.clone(), &current.peer, control, timeline);
                        } else if !on {
                            release_share_sound(&current.peer, &track, &mut share_audio, control, timeline);
                        }
                    }
                }
                Some(CallCommand::RefreshShareSources) => refresh_share_sources(control.updates(), options.video),
                Some(CallCommand::SetHand(true)) => match remote.signaling.raise_hand(&remote.conversation, &remote.participant).await {
                    Ok(state_id) => own_hand = state_id,
                    Err(error) => {
                        timeline.record("raise hand failed", error.to_string());
                        control.send(CallUpdate::Notice("Could not raise your hand".into()));
                    }
                },
                Some(CallCommand::SetHand(false)) => {
                    let state_ids: Vec<String> = roster.hand_of(&own_mri).map(|hand| hand.state_id.clone()).or(own_hand.take()).into_iter().collect();
                    own_hand = None;
                    if !state_ids.is_empty()
                        && let Err(error) = remote.signaling.lower_hands(&remote.conversation, &remote.participant, &state_ids).await
                    {
                        timeline.record("lower hand failed", error.to_string());
                        control.send(CallUpdate::Notice("Could not lower your hand".into()));
                    }
                }
                Some(CallCommand::LowerHand { mri }) => {
                    let state_ids: Vec<String> = roster.hand_of(&mri).map(|hand| hand.state_id.clone()).into_iter().collect();
                    if !state_ids.is_empty()
                        && let Err(error) = remote.signaling.lower_hands(&remote.conversation, &remote.participant, &state_ids).await
                    {
                        timeline.record("lower hand failed", error.to_string());
                        control.send(CallUpdate::Notice("Could not lower that hand".into()));
                    }
                }
                Some(CallCommand::LowerAllHands) => {
                    if let Err(error) = remote.signaling.lower_all_hands(&remote.conversation, &remote.participant).await {
                        timeline.record("lower all hands failed", error.to_string());
                        control.send(CallUpdate::Notice("Could not lower all hands".into()));
                    }
                }
                Some(CallCommand::SendReaction(reaction)) => {
                    if let Err(error) = remote.signaling.send_reaction(&remote.conversation, &remote.participant, reaction).await {
                        timeline.record("reaction failed", error.to_string());
                        control.send(CallUpdate::Notice("Could not send the reaction".into()));
                    }
                }
                Some(CallCommand::EndMeeting) => {
                    if is_meeting {
                        match remote.signaling.end_for_all(&remote.conversation, &remote.participant).await {
                            Ok(()) => {
                                live.skip_leave = true;
                                report.left_cleanly = true;
                            }
                            Err(error) => timeline.record("end meeting failed", error.to_string()),
                        }
                    }
                    advance(state, CallSignal::LocalLeave, control);
                    break;
                }
                Some(CallCommand::Hangup) | None => {
                    advance(state, CallSignal::LocalLeave, control);
                    break;
                }
            },
            _ = stats_tick.tick(), if remote_set => {
                let snapshot = collect_stats(&current.peer).await;
                ticks += 1;
                let local_level = if muted && share_audio.is_none() { 0.0 } else { snapshot.local_level as f32 };
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
                report.video = VideoReport {
                    frames_received: live.video_counters.received.load(Ordering::Relaxed),
                    frames_published: live.video_counters.published.load(Ordering::Relaxed),
                    ..snapshot.video.clone()
                };
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
                            "in packets={} bytes={} level={:.3} rms={decoded_rms:.3} tone={ratio:.2} out packets={} mic={:.3} erle={:.1} dtls={} video in packets={} decoded={} {}x{} out packets={} encoded={} {}x{} mixer reports {} (rtt {:.0} ms)",
                            snapshot.inbound_packets, snapshot.inbound_bytes, snapshot.audio_level, snapshot.outbound_packets, snapshot.local_level, snapshot.echo_return_loss_enhancement, snapshot.dtls_state,
                            snapshot.video.inbound_packets, snapshot.video.inbound_frames_decoded, snapshot.video.inbound_width, snapshot.video.inbound_height,
                            snapshot.video.outbound_packets, snapshot.video.outbound_frames_encoded, snapshot.video.outbound_width, snapshot.video.outbound_height,
                            snapshot.video.remote_reports, snapshot.video.remote_round_trip_ms
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
            _ = sleep_until(outgoing_deadline.unwrap_or_else(far_future)), if outgoing_deadline.is_some() => {
                timeline.record("no answer", "outgoing call timed out");
                live.cancel_leave = true;
                advance(state, CallSignal::RemoteEnd(EndKind::NoAnswer), control);
                break;
            }
            _ = sleep_until(keep_alive_at.unwrap_or_else(far_future)), if keep_alive_at.is_some() => {
                keep_alive_at = Some(Instant::now() + keep_alive_period(keep_alive_seconds));
                let remote = remote.clone();
                let links = links.clone();
                let call_leg = keep_alive_link.clone();
                tokio::spawn(async move {
                    if let Some(url) = call_leg {
                        let _ = remote.signaling.keep_call_alive(&url).await;
                    }
                    let _ = remote.signaling.keep_conversation_alive(&remote.conversation, &remote.participant, &links).await;
                });
            }
        }
    }
    live.callbacks = Some(callbacks);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn start_escalation(
    factory: &PeerConnectionFactory,
    grant: &RelayGrant,
    options: &CallOptions,
    track: &RtcAudioTrack,
    remote: &RemoteCall,
    links: &CallbackLinks,
    media_leg_id: &str,
    acceptance_links: &BTreeMap<String, String>,
) -> Result<NextPeer> {
    let url = acceptance_links
        .get("mediaRenegotiation")
        .ok_or_else(|| Error::Signaling("acceptance without a mediaRenegotiation link".into()))?;
    let session = open_peer(factory, grant, &options.relay_host)?;
    let media = create_offer(&session.peer, factory, track).await?;
    let body = renegotiation_body(&remote.participant, links, &media.signaled.sdp, media_leg_id);
    remote.signaling.post_json("POST mediaRenegotiation", url, body).await?;
    Ok(NextPeer {
        session,
        offered: Some(media),
    })
}

async fn answer_on_new_peer(
    factory: &PeerConnectionFactory,
    grant: &RelayGrant,
    options: &CallOptions,
    track: &RtcAudioTrack,
    teams_offer: &str,
) -> Result<(PeerSession, AnsweredMedia)> {
    let session = open_peer(factory, grant, &options.relay_host)?;
    let answered = answer_offer(&session.peer, Some(track), teams_offer).await?;
    Ok((session, answered))
}

async fn send_media_answer(
    remote: &RemoteCall,
    links: &CallbackLinks,
    reply_links: &BTreeMap<String, String>,
    answered: &AnsweredMedia,
    media_leg_id: &str,
) -> Result<()> {
    let url = reply_links
        .get("mediaAnswer")
        .ok_or_else(|| Error::Signaling("renegotiation without a mediaAnswer link".into()))?;
    let body = escalation_answer_body(&remote.participant, links, &answered.answer_sdp, media_leg_id);
    remote.signaling.post_json("POST mediaAnswer", url, body).await
}

#[allow(clippy::too_many_arguments)]
fn negotiate_video(
    video: &mut VideoReceive,
    signaling: &Arc<Signaling>,
    control: &CallControl,
    timeline: &Timeline,
    answer_sdp: &str,
    media: &OfferedMedia,
    from_mixer: bool,
    links: &BTreeMap<String, String>,
    roster: &Roster,
) {
    let streams = match stream_lines(answer_sdp, &media.signaled.lines) {
        Ok(streams) => streams,
        Err(error) => {
            timeline.record("video slots unknown", error.to_string());
            return;
        }
    };
    let table = SlotTable::from_streams(&streams);
    let link = links.get("applyChannelParameters").or_else(|| links.get("controlVideoStreaming"));
    timeline.record(
        "video slots",
        format!(
            "{} receive slot(s), screen {}, fromMixer={from_mixer}, request link {}",
            table.people.len(),
            table.screen.is_some(),
            link.is_some()
        ),
    );
    let requester = link
        .filter(|_| from_mixer)
        .map(|url| SourceRequester::start(signaling.clone(), url.clone(), video.router().clone(), control.video.clone(), timeline.clone()));
    video.negotiated(table, requester);
    send_receive_change(control, video.reselect(roster));
}

const NOT_CONNECTED_FOR_VIDEO: &str = "Video starts once the call is connected";

async fn camera_devices(mode: VideoMode) -> Vec<CameraDevice> {
    match mode {
        VideoMode::Pattern => vec![CameraDevice { key: "pattern".into(), name: "Test pattern".into() }],
        VideoMode::Off => Vec::new(),
        VideoMode::Platform => tokio::task::spawn_blocking(list_cameras).await.unwrap_or_default(),
    }
}

fn refresh_share_sources(updates: mpsc::UnboundedSender<CallUpdate>, mode: VideoMode) {
    tokio::spawn(async move {
        let sources = match mode {
            VideoMode::Pattern => vec![
                ShareSource { id: 1, kind: ShareKind::Screen, title: "Test screen".into() },
                ShareSource { id: 2, kind: ShareKind::Window, title: "Test window".into() },
            ],
            VideoMode::Off => Vec::new(),
            VideoMode::Platform => tokio::task::spawn_blocking(list_share_sources).await.unwrap_or_default(),
        };
        let _ = updates.send(CallUpdate::ShareSources(sources));
    });
}

fn tell_media_descriptions(
    remote: &RemoteCall,
    links: &BTreeMap<String, String>,
    description: MediaDescription,
    request_id: &mut u32,
    timeline: &Timeline,
) {
    let Some(url) = links.get("updateMediaDescriptions").cloned() else {
        timeline.record("updateMediaDescriptions skipped", "no link from the mixer");
        return;
    };
    *request_id += 1;
    let request_id = *request_id;
    let signaling = remote.signaling.clone();
    let timeline = timeline.clone();
    tokio::spawn(async move {
        let sent = format!("{} {} request {request_id}", description.mid, description.direction);
        if let Err(error) = signaling.update_media_descriptions(&url, &[description], request_id).await {
            timeline.record("updateMediaDescriptions failed", format!("{sent}: {error}"));
        }
    });
}

fn send_receive_change(control: &CallControl, change: ReceiveChange) {
    if let Some(sharer) = change.screen_sharer {
        control.send(CallUpdate::ScreenShare(sharer));
    }
}

fn far_future() -> Instant {
    Instant::now() + FAR_FUTURE
}

fn apply_mute(
    command: MuteCommand,
    currently_muted: bool,
    audio: &AudioSetup,
    share_audio: Option<&ShareAudio>,
    remote: &RemoteCall,
    control: &CallControl,
) -> bool {
    let effect = MuteEffect::for_muted(command.target(currently_muted));
    audio.track.set_enabled(effect.track_enabled);
    if let Some(share_audio) = share_audio {
        share_audio.set_microphone_muted(effect.muted);
    }
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

fn swap_audio_track(peer: &PeerConnection, track: &RtcAudioTrack) -> Result<()> {
    let sender = peer
        .senders()
        .into_iter()
        .find(|sender| matches!(sender.track(), Some(MediaStreamTrack::Audio(_))))
        .ok_or_else(|| Error::Webrtc("no audio sender".into()))?;
    sender
        .set_track(Some(MediaStreamTrack::Audio(track.clone())))
        .map_err(|error| Error::Webrtc(error.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn engage_share_sound(
    factory: &PeerConnectionFactory,
    options: &CallOptions,
    input: &DeviceChoice,
    muted: bool,
    remote_audio: Option<RtcAudioTrack>,
    peer: &PeerConnection,
    control: &CallControl,
    timeline: &Timeline,
) -> Option<ShareAudio> {
    let started = ShareAudio::start(factory, options.audio, options.tone_hz, input, muted, remote_audio)
        .and_then(|audio| swap_audio_track(peer, &audio.track).map(|()| audio));
    match started {
        Ok(audio) => {
            timeline.record("computer sound", format!("{:?}, own playback excluded: {}", audio.loopback, audio.loopback.excludes_own_playback()));
            if let Some(warning) = &audio.warning {
                timeline.record("computer sound warning", warning.clone());
            }
            control.send(CallUpdate::ShareSound(true));
            Some(audio)
        }
        Err(error) => {
            timeline.record("computer sound failed", error.to_string());
            control.send(CallUpdate::Notice("Computer sound could not start, sharing without it".into()));
            None
        }
    }
}

fn release_share_sound(
    peer: &PeerConnection,
    device_track: &RtcAudioTrack,
    share_audio: &mut Option<ShareAudio>,
    control: &CallControl,
    timeline: &Timeline,
) {
    if share_audio.take().is_none() {
        return;
    }
    if let Err(error) = swap_audio_track(peer, device_track) {
        timeline.record("microphone track not restored", error.to_string());
    }
    control.send(CallUpdate::ShareSound(false));
}

async fn tear_down(mut live: Live, inner: &Inner, timeline: &Timeline, report: &mut CallReport, cancelled: bool) {
    if let Some(audio) = &live.audio {
        audio.track.set_enabled(false);
    }
    live.broker_running.store(false, Ordering::SeqCst);
    if !live.skip_leave
        && let Some(remote) = &live.remote
    {
        let reason = if cancelled { LeaveReason::Cancel } else { LeaveReason::Hangup };
        match remote.signaling.leave(&remote.conversation, &remote.participant, reason).await {
            Ok(()) => report.left_cleanly = true,
            Err(error) => timeline.record("leave failed", error.to_string()),
        }
    }
    if report.end.is_none()
        && let Some(callbacks) = live.callbacks.as_mut()
        && live.remote.is_some()
    {
        let waited = timeout(END_CALLBACK_WAIT, async {
            while let Some(callback) = callbacks.recv().await {
                let request_id = callback.request_id;
                let event = handle_callback(&callback, timeline);
                inner.replier.reply(request_id, 200, "");
                if let Ok(CallEvent::End(end)) = event {
                    return Some(end);
                }
            }
            None
        })
        .await;
        report.end = waited.ok().flatten();
    }
    for peer in &live.peers {
        peer.close();
    }
    if let Some(audio) = live.audio.take() {
        audio.stop().await;
    }
    if let Some(broker) = live.broker.take() {
        broker.abort();
    }
    for task in live.video_tasks.drain(..) {
        task.abort();
    }
    if let Some(route_id) = &live.route_id {
        inner.unregister_route(route_id);
    }
    timeline.record("closed", "peer closed, call route removed");
}

fn handle_callback(callback: &TrouterCallback, timeline: &Timeline) -> Result<CallEvent> {
    let body = decode_body(callback)?;
    let (_, event) = classify(&callback.path, body)?;
    let detail = match &event {
        CallEvent::Acceptance(acceptance) => format!("answer sdp {} bytes, fromMixer={}", acceptance.sdp.len(), acceptance.from_mixer),
        CallEvent::End(end) => format!("code={} subCode={} phrase={}", end.code, end.sub_code, end.phrase),
        CallEvent::Progress(status) => format!("{status:?}"),
        _ => String::new(),
    };
    timeline.record(format!("Trouter {}", event.name()), detail);
    Ok(event)
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
fn dump_sdp(options: &CallOptions, name: &str, sdp: &str) {
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
    video: VideoReport,
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
            RtcStats::InboundRtp(inbound) if inbound.stream.kind == "video" => {
                snapshot.video.inbound_packets += inbound.received.packets_received;
                snapshot.video.inbound_frames_decoded += u64::from(inbound.inbound.frames_decoded);
                snapshot.video.inbound_width = snapshot.video.inbound_width.max(inbound.inbound.frame_width);
                snapshot.video.inbound_height = snapshot.video.inbound_height.max(inbound.inbound.frame_height);
            }
            RtcStats::RemoteInboundRtp(remote) if remote.stream.kind == "video" => {
                snapshot.video.remote_reports += remote.remote_inbound.round_trip_time_measurements;
                snapshot.video.remote_round_trip_ms = snapshot.video.remote_round_trip_ms.max(remote.remote_inbound.round_trip_time * 1000.0);
            }
            RtcStats::OutboundRtp(outbound) if outbound.stream.kind == "video" => {
                snapshot.video.outbound_packets += outbound.sent.packets_sent;
                snapshot.video.outbound_frames_encoded += u64::from(outbound.outbound.frames_encoded);
                snapshot.video.outbound_width = snapshot.video.outbound_width.max(outbound.outbound.frame_width);
                snapshot.video.outbound_height = snapshot.video.outbound_height.max(outbound.outbound.frame_height);
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
    fn keep_alive_fires_at_ninety_percent_of_the_interval() {
        assert_eq!(keep_alive_period(Some(2700)), Duration::from_secs(2430));
        assert_eq!(keep_alive_period(None), Duration::from_secs(2430));
        assert_eq!(keep_alive_period(Some(10)), Duration::from_secs(54));
    }

    #[test]
    fn specs_map_to_invitation_targets() {
        assert_eq!(CallSpec::Echo.target(), InviteTarget::Echo);
        let people = CallSpec::People {
            callees: vec![Callee { mri: "8:orgid:b".into(), display_name: "Bea".into() }],
            thread_id: "19:t@thread.v2".into(),
        };
        assert!(people.is_people() && !people.is_meeting());
        assert!(matches!(people.target(), InviteTarget::People { thread_id, .. } if thread_id == "19:t@thread.v2"));
        let meeting = CallSpec::Meeting(MeetingTarget {
            thread_id: "19:meeting@thread.v2".into(),
            tenant_id: "t".into(),
            organizer_id: "o".into(),
            meeting_data: None,
        });
        assert!(meeting.is_meeting() && !meeting.is_people());
    }
}
