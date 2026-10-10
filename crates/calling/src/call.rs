use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chatsvc::TrouterCallback;
use futures_util::StreamExt;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::data_channel::{DataChannel, DataChannelInit};
use libwebrtc::prelude::*;
use libwebrtc::rtp_transceiver::{RtpTransceiver, RtpTransceiverDirection, RtpTransceiverInit};
use libwebrtc::session_description::{SdpType, SessionDescription};
use libwebrtc::stats::RtcStats;
use session::Session;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio::time::{Instant, interval, sleep_until, timeout};

use crate::audio::{SAMPLE_RATE, rms, tone_ratio, write_wav};
use crate::audio_io::{AudioMode, AudioSetup};
use crate::background::{BackgroundChoice, BackgroundPicture};
use crate::breakout::{self, RoomWatcher};
use crate::captions::{CAPTIONS_DATA_ID, CaptionEntry, CaptionFlow, CaptionState, CaptionStep, BotAction, parse_caption_message};
use crate::data_channel::Depacketizer;
use crate::control::{CallCommand, CallControl, CallUpdate, Progress};
use crate::devices::{self, DeviceChoice};
use crate::end::EndKind;
use crate::engine::{CallEngine, EngineConfig, Inner};
use crate::error::{Error, Result};
use crate::hold::{HoldState, INACTIVE, MediaLine, accepted_modalities, is_hold, rewrite_directions};
use crate::mute::{MuteCommand, MuteEffect};
use crate::push::CallNotification;
use crate::share_audio::ShareAudio;
use crate::relay::{DEFAULT_RELAY_HOST, RelayGrant, fetch_relay_grant};
use crate::renegotiation::{MediaAction, MediaNegotiator, MediaPath};
use crate::organizer::Target;
use crate::recording::{Consent, RecordingStatus};
use crate::roster::Roster;
use crate::sdp::{LineRole, OfferPlan, RemoteOffer, SignaledOffer, from_teams_offer, stream_lines, to_browser_answer, to_teams_answer, to_teams_offer};
use crate::signaling::{
    Answer, Attached, Callee, Conversation, Invitation, InviteTarget, LeaveReason, MeetingTarget, Participant, Signaling,
    TenantRouting, chat_thread_id, find_echo_bot_thread, escalation_answer_body, renegotiation_body,
};
use crate::state::{CallSignal, CallState};
use crate::timeline::{Timeline, TimelineEntry};
use crate::transfer::{TransferEvent, TransferTarget};
use crate::video_frame::VideoKey;
use crate::video_layout::{
    CAMERA_MID, MediaDescription, ONE_TO_ONE_SHARE_MID, SlotTable, VideoLines, VideoRouter, add_one_to_one_lines, add_video_lines, offer_plan, one_to_one_plan,
};
use crate::video_send::{LocalVideo, VideoMode};
use crate::camera::{CameraDevice, list_cameras};
use crate::screen::{ShareKind, ShareSource, list_share_sources};
use crate::video_receive::{ReceiveChange, SourceRequester, VideoCounters, VideoReceive, spawn_video_pump};
use crate::trouter_events::{
    CallEnd, CallEvent, CallbackLinks, ProgressStatus, acceptance_acknowledgement, classify, decode_body,
};
use crate::whiteboard;

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
const DATA_CHANNEL_LABEL: &str = "main-channel";
pub const DATA_CHANNEL_ENV: &str = "CALLING_DATA_CHANNEL";
const CAPTION_START_LIMIT: Duration = Duration::from_secs(40);
const BLUR_TIMING_EVERY_TICKS: u32 = TICKS_PER_SECOND * 2;
const BOT_PREFIX: &str = "28:";
const RECORDING_START_LIMIT: Duration = Duration::from_secs(40);
const SAME_PEER_ANSWER_LIMIT: Duration = Duration::from_secs(15);
const CAMERA_STREAM: &str = "camera";
const CONSENT_FIRST_NOTICE: &str = "Accept the recording notice first";

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
    pub data_channel: bool,
    pub concurrent: bool,
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
            data_channel: data_channel_from_env(),
            concurrent: false,
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
    pub accept: AcceptMode,
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

fn data_channel_from_env() -> bool {
    !matches!(std::env::var(DATA_CHANNEL_ENV).ok().as_deref(), Some("0" | "off"))
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
    Data(Vec<u8>),
}

struct PeerSession {
    peer: PeerConnection,
    events: mpsc::UnboundedReceiver<PeerEvent>,
    _data_channel: Option<DataChannel>,
}

struct OfferedMedia {
    browser_offer: String,
    signaled: SignaledOffer,
    video: VideoLines,
    plan: OfferPlan,
    modalities: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Conference,
    OneToOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptMode {
    Audio,
    Video,
}

#[derive(Clone, Copy)]
enum AnswerVideo<'a> {
    Reject,
    Receive,
    Send(&'a RtcVideoTrack),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Hold(bool),
    Camera(bool),
}

struct SamePeerOffer {
    browser_offer: String,
    signaled: SignaledOffer,
    change: Change,
}

struct NextPeer {
    session: PeerSession,
    offered: Option<OfferedMedia>,
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
        let _one_call_at_a_time = if options.concurrent { None } else { Some(inner.gate.lock().await) };
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

fn open_peer(factory: &PeerConnectionFactory, grant: &RelayGrant, relay_host: &str, with_data_channel: bool) -> Result<PeerSession> {
    let mut configuration = RtcConfiguration::default();
    configuration.ice_servers = vec![grant.ice_server(relay_host)];
    let peer = factory
        .create_peer_connection(configuration)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let (sender, events) = mpsc::unbounded_channel();
    let data_channel = with_data_channel.then(|| open_data_channel(&peer, sender.clone())).transpose()?;
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
    Ok(PeerSession { peer, events, _data_channel: data_channel })
}

fn open_data_channel(peer: &PeerConnection, sender: mpsc::UnboundedSender<PeerEvent>) -> Result<DataChannel> {
    let channel = peer
        .create_data_channel(DATA_CHANNEL_LABEL, DataChannelInit::default())
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    channel.on_message(Some(Box::new(move |buffer| {
        let _ = sender.send(PeerEvent::Data(buffer.data.to_vec()));
    })));
    Ok(channel)
}

fn one_to_one_directions(camera: bool) -> impl Fn(MediaLine<'_>) -> Option<&'static str> {
    move |line| match (line.kind, line.mid) {
        ("video", CAMERA_MID) => Some(if camera { "sendrecv" } else { INACTIVE }),
        ("video", ONE_TO_ONE_SHARE_MID) => Some(INACTIVE),
        _ => None,
    }
}

fn directions_for(layout: Layout, hold: bool, camera: bool, text: &str) -> String {
    match (hold, layout) {
        (true, _) => rewrite_directions(text, |_| Some(INACTIVE)),
        (false, Layout::OneToOne) => rewrite_directions(text, one_to_one_directions(camera)),
        (false, Layout::Conference) => text.to_owned(),
    }
}

async fn create_offer(peer: &PeerConnection, factory: &PeerConnectionFactory, track: &RtcAudioTrack, layout: Layout) -> Result<OfferedMedia> {
    let transceiver_init = RtpTransceiverInit {
        direction: RtpTransceiverDirection::SendRecv,
        stream_ids: vec![MIC_STREAM.into()],
        send_encodings: Vec::new(),
    };
    peer.add_transceiver(MediaStreamTrack::Audio(track.clone()), transceiver_init)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let (video, plan) = match layout {
        Layout::Conference => (add_video_lines(peer, factory)?, offer_plan()),
        Layout::OneToOne => (add_one_to_one_lines(peer, factory)?, one_to_one_plan()),
    };
    let offer = peer
        .create_offer(OfferOptions {
            offer_to_receive_audio: true,
            offer_to_receive_video: true,
            ..OfferOptions::default()
        })
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let browser_offer = directions_for(layout, false, false, &offer.to_string());
    let description = SessionDescription::parse(&browser_offer, SdpType::Offer).map_err(|error| Error::Sdp(error.description))?;
    peer.set_local_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let mut numbers = random_u32;
    let signaled = to_teams_offer(&browser_offer, &plan, &mut numbers)?;
    Ok(OfferedMedia { browser_offer, signaled, video, plan, modalities: accepted_modalities(false) })
}

struct AnsweredMedia {
    remote: RemoteOffer,
    answer_sdp: String,
    camera_mid: Option<String>,
}

fn main_video_mid(remote: &RemoteOffer) -> Option<String> {
    remote.lines.iter().find(|line| line.role == LineRole::MainVideo).and_then(|line| line.browser_mids.first().cloned())
}

async fn answer_offer(
    peer: &PeerConnection,
    track: Option<&RtcAudioTrack>,
    teams_offer: &str,
    video: AnswerVideo<'_>,
) -> Result<AnsweredMedia> {
    let remote = from_teams_offer(teams_offer)?;
    let description =
        SessionDescription::parse(&remote.browser_sdp, SdpType::Offer).map_err(|error| Error::Sdp(error.description))?;
    peer.set_remote_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let camera_mid = main_video_mid(&remote).filter(|_| !matches!(video, AnswerVideo::Reject));
    for transceiver in peer.transceivers() {
        let is_video = matches!(transceiver.receiver().track(), Some(MediaStreamTrack::Video(_)));
        if is_video && transceiver.mid() != camera_mid {
            let _ = transceiver.stop();
        }
    }
    if let Some(track) = track {
        peer.add_track(MediaStreamTrack::Audio(track.clone()), &[MIC_STREAM])
            .map_err(|error| Error::Webrtc(error.to_string()))?;
    }
    if let (AnswerVideo::Send(camera), Some(_)) = (video, &camera_mid) {
        peer.add_track(MediaStreamTrack::Video(camera.clone()), &[CAMERA_STREAM])
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
        camera_mid,
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
    let accept_mode = incoming.as_ref().map_or(AcceptMode::Audio, |incoming| incoming.accept);
    let (links, participant, mut callbacks, incoming_media) = match incoming {
        Some(IncomingCall {
            accept: _,
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

    let wants_data_channel = options.data_channel && spec.as_ref().is_some_and(CallSpec::is_meeting);
    let mut current = open_peer(&factory, &grant, &options.relay_host, wants_data_channel)?;
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
    let meeting_target = match &spec {
        Some(CallSpec::Meeting(target)) => Some(target.clone()),
        _ => None,
    };
    let mut layout = match &spec {
        Some(CallSpec::People { callees, .. }) if callees.len() == 1 => Layout::OneToOne,
        _ => Layout::Conference,
    };
    let peer_mri = match &spec {
        Some(CallSpec::People { callees, .. }) if callees.len() == 1 => Some(callees[0].mri.clone()),
        _ => incoming_media.as_ref().map(|(notification, _)| notification.caller.mri.clone()),
    };
    let mut layout_plan: OfferPlan;
    let mut direct_video_mid: Option<String> = None;
    let mut sending_camera = false;
    let mut media_links: BTreeMap<String, String> = BTreeMap::new();

    let conversation = match (spec, incoming_media) {
        (Some(spec), None) => {
            let media = create_offer(&current.peer, &factory, &track, layout).await?;
            layout_plan = media.plan.clone();
            if layout == Layout::OneToOne {
                direct_video_mid = Some(CAMERA_MID.to_owned());
            }
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
                modalities: &media.modalities,
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
        (None, Some((notification, attached))) => {
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
            let one_to_one = !attached.from_mixer && !notification.is_multi_party;
            layout = if one_to_one { Layout::OneToOne } else { Layout::Conference };
            let camera_track = local_video.camera_track();
            let video_answer = match accept_mode {
                _ if !one_to_one => AnswerVideo::Reject,
                AcceptMode::Video if local_video.is_available() => AnswerVideo::Send(&camera_track),
                _ => AnswerVideo::Receive,
            };
            let answered = answer_offer(&current.peer, Some(&track), &teams_offer, video_answer).await?;
            layout_plan = answered.remote.plan();
            direct_video_mid = answered.camera_mid.clone();
            timeline.record("setRemoteDescription", format!("offer applied, {} signaled line(s)", answered.remote.lines.len()));
            dump_sdp(options, "3-teams-offer.sdp", &teams_offer);
            dump_sdp(options, "4-teams-answer.sdp", &answered.answer_sdp);
            let acceptance_url = attached
                .links
                .get("acceptance")
                .ok_or_else(|| Error::Signaling("attach answer without an acceptance link".into()))?;
            let modalities = accepted_modalities(matches!(video_answer, AnswerVideo::Send(_)) && answered.camera_mid.is_some());
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
            media_links.extend(attached.links.clone());
            if let (AnswerVideo::Send(_), Some(mid)) = (video_answer, &answered.camera_mid) {
                let camera = peer_transceiver(&current.peer, mid);
                match camera.map(|camera| local_video.set_lines(VideoLines { camera, share: None })) {
                    Some(Ok(())) => match local_video.start_camera(&camera_choice) {
                        Ok(()) => sending_camera = true,
                        Err(error) => control.send(CallUpdate::Notice(format!("Camera could not start: {error}"))),
                    },
                    Some(Err(error)) => timeline.record("video lines not attached", error.to_string()),
                    None => timeline.record("video lines not attached", "no transceiver for the camera line"),
                }
            }
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
    if let (Some(mid), Some(mri)) = (&direct_video_mid, &peer_mri) {
        video.router().set(mid, Some(VideoKey::Person(mri.clone())));
    }
    if sending_camera {
        control.send(CallUpdate::Camera(true));
    }
    let mut share_audio: Option<ShareAudio> = None;
    let mut share_sound_wanted = false;
    let mut remote_audio: Option<RtcAudioTrack> = None;
    let mut own_hand: Option<String> = None;
    let mut depacketizer = Depacketizer::default();
    let mut caption_flow = CaptionFlow::default();
    let mut caption_deadline: Option<Instant> = None;
    let mut recording_flow = CaptionFlow::default();
    let mut recording_deadline: Option<Instant> = None;
    let mut recording_title = String::new();
    let mut recording_status = RecordingStatus::default();
    let mut bot_pending = false;
    let mut consent_pending = false;
    let mut room_watcher = RoomWatcher::default();
    let mut main_meeting_sent = false;
    let mut local_hold = false;
    let mut remote_hold = false;
    let mut same_peer: Option<SamePeerOffer> = None;
    let mut same_peer_deadline: Option<Instant> = None;
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
                                if layout == Layout::Conference {
                                    negotiate_video(&mut video, &signaling, control, timeline, &acceptance.sdp, media, acceptance.from_mixer, &media_links, &roster);
                                }
                                if let Some(link) = acceptance.links.get("replacement") {
                                    control.send(CallUpdate::ReplacementLink(link.clone()));
                                }
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
                        let action = negotiator.on_media_answer();
                        if action == MediaAction::ApplyOnSamePeer
                            && let Some(pending) = same_peer.take()
                        {
                            same_peer_deadline = None;
                            let applied = apply_answer(&current.peer, &answer.sdp, &pending.signaled, &pending.browser_offer).await;
                            match applied {
                                Ok(()) => {
                                    if let Some(url) = answer.links.get("mediaAcknowledgement") {
                                        let _ = remote.signaling.post_empty("POST mediaAcknowledgement", url).await;
                                    }
                                    match pending.change {
                                        Change::Hold(hold) => {
                                            local_hold = hold;
                                            control.send(CallUpdate::Hold(HoldState::from_flags(local_hold, remote_hold)));
                                        }
                                        Change::Camera(on) => control.send(CallUpdate::Camera(on)),
                                    }
                                }
                                Err(error) => {
                                    timeline.record("same-peer answer rejected", error.to_string());
                                    undo_change(pending.change, &mut local_video, control);
                                }
                            }
                        }
                        if action == MediaAction::ApplyOnNewPeer {
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
                                let video_answer = if layout == Layout::OneToOne { AnswerVideo::Receive } else { AnswerVideo::Reject };
                                match answer_offer(&current.peer, None, &renegotiation.sdp, video_answer).await {
                                    Ok(answered) => {
                                        let sent = send_media_answer(&remote, &links, &renegotiation.links, &answered, &media_leg_id).await;
                                        let held = is_hold(&renegotiation.sdp).unwrap_or(false);
                                        if held != remote_hold {
                                            remote_hold = held;
                                            control.send(CallUpdate::Hold(HoldState::from_flags(local_hold, remote_hold)));
                                        }
                                        sent
                                    }
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
                            if roster.caption_bot().is_some() {
                                bot_pending = false;
                            }
                            if caption_flow.wanted() {
                                run_bot_steps(BotJob::Captions, &mut caption_flow, &roster, &remote, &links, meeting_chat.as_deref(), "", &mut bot_pending, control, timeline).await;
                                if caption_flow.on() {
                                    caption_deadline = None;
                                }
                            }
                            if recording_flow.wanted() {
                                run_bot_steps(BotJob::Recording, &mut recording_flow, &roster, &remote, &links, meeting_chat.as_deref(), &recording_title, &mut bot_pending, control, timeline).await;
                            }
                        }
                    }
                    Ok(CallEvent::AddParticipantFailure(_)) => {
                        replier.reply(request_id, 200, "");
                        bot_pending = false;
                        if recording_flow.wanted() && roster.caption_bot().is_none() {
                            timeline.record("recorder bot failed", "addParticipantFailure");
                            recording_flow.failed();
                            recording_deadline = None;
                            control.send(CallUpdate::Notice("The recording service could not join".into()));
                        }
                        if caption_flow.wanted() && roster.caption_bot().is_none() {
                            timeline.record("caption bot failed", "addParticipantFailure");
                            caption_flow.failed();
                            caption_deadline = None;
                            control.send(CallUpdate::Captions(CaptionState::Failed("The captions service could not join".into())));
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
                        let change = recording_status.apply(&body);
                        if let Some(on) = change.recording {
                            control.send(CallUpdate::Recording(on));
                            if on {
                                recording_deadline = None;
                            }
                        }
                        if let Some(required) = change.consent_required {
                            consent_pending = required;
                            control.send(CallUpdate::ConsentRequired(required));
                            if required {
                                withhold_media(&mut muted, live.audio.as_ref().expect("audio set above"), share_audio.as_ref(), &remote, &mut local_video, control);
                            }
                        }
                        if !main_meeting_sent
                            && let Some(main) = breakout::main_meeting_target(&body)
                        {
                            main_meeting_sent = true;
                            control.send(CallUpdate::BreakoutRoom { main });
                        }
                    }
                    Ok(CallEvent::LocalParticipantUpdate(body)) => {
                        replier.reply(request_id, 200, "");
                        let moved = room_watcher.apply(breakout::room_assignment(&body)).filter(|moved| !is_own_meeting(meeting_target.as_ref(), &moved.target));
                        if let Some(moved) = moved {
                            timeline.record("breakout", format!("moved to {}", moved.room_name));
                            control.send(CallUpdate::BreakoutMove(moved));
                        }
                    }
                    Ok(CallEvent::Replacement(body)) => {
                        replier.reply(request_id, 200, "");
                        let invited = breakout::invite_move(&body).filter(|moved| !is_own_meeting(meeting_target.as_ref(), &moved.target));
                        if let Some(moved) = invited {
                            timeline.record("breakout", format!("invited to {}", moved.room_name));
                            control.send(CallUpdate::BreakoutMove(moved));
                        }
                    }
                    Ok(CallEvent::ContentShareUpdate(body)) => {
                        replier.reply(request_id, 200, "");
                        if let Some(share) = whiteboard::content_share(&body).filter(|share| share.whiteboard) {
                            control.send(CallUpdate::Whiteboard(Some(share)));
                        }
                    }
                    Ok(CallEvent::ContentShareEnd(_)) => {
                        replier.reply(request_id, 200, "");
                        control.send(CallUpdate::Whiteboard(None));
                    }
                    Ok(CallEvent::Transfer(event)) => {
                        replier.reply(request_id, 200, "");
                        match event {
                            TransferEvent::Accepted => timeline.record("transfer", "accepted by the target"),
                            completed if completed.succeeded() => {
                                control.send(CallUpdate::Notice("Call transferred".into()));
                                advance(state, CallSignal::LocalLeave, control);
                                break;
                            }
                            _ => control.send(CallUpdate::Notice("The transfer did not go through".into())),
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
                PeerEvent::Data(frame) => {
                    if let Some(message) = depacketizer.push(&frame)
                        && message.data_id == CAPTIONS_DATA_ID
                    {
                        for entry in parse_caption_message(&message.payload) {
                            if caption_flow.text_arrived() {
                                caption_deadline = None;
                                control.send(CallUpdate::Captions(CaptionState::On));
                            }
                            control.send(CallUpdate::Caption(named_caption(entry, &roster)));
                        }
                    }
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
                        if let Some(mid) = direct_video_mid.take() {
                            video.router().set(&mid, None);
                        }
                        if let Some(media) = &offered {
                            layout = Layout::Conference;
                            layout_plan = media.plan.clone();
                            if let Err(error) = local_video.set_lines(media.video.clone()) {
                                timeline.record("video lines not attached", error.to_string());
                            }
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
                    if consent_pending && !mute.target(muted) {
                        control.send(CallUpdate::Notice(CONSENT_FIRST_NOTICE.into()));
                    } else {
                        muted = apply_mute(mute, muted, live.audio.as_ref().expect("audio set above"), share_audio.as_ref(), &remote, control);
                    }
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
                    let direct = layout == Layout::OneToOne && negotiator.path() != MediaPath::Mixer;
                    if !remote_set {
                        control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                    } else if on && consent_pending {
                        control.send(CallUpdate::Notice(CONSENT_FIRST_NOTICE.into()));
                    } else if local_hold {
                        control.send(CallUpdate::Notice(ON_HOLD_NOTICE.into()));
                    } else if on {
                        let started = start_camera_line(&mut local_video, &current.peer, direct.then_some(direct_video_mid.as_deref()).flatten(), &camera_choice);
                        match started {
                            Ok(()) if direct => {
                                match offer_same_peer(&current.peer, &layout_plan, layout, false, true, &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                                    Ok((browser_offer, signaled)) => {
                                        same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Camera(true) });
                                        same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                                    }
                                    Err(error) => {
                                        timeline.record("camera offer failed", error.to_string());
                                        undo_change(Change::Camera(true), &mut local_video, control);
                                    }
                                }
                            }
                            Ok(()) => {
                                control.send(CallUpdate::Camera(true));
                                tell_media_descriptions(&remote, &media_links, MediaDescription::camera(true), &mut media_description_request, timeline);
                            }
                            Err(error) => control.send(CallUpdate::Notice(format!("Camera could not start: {error}"))),
                        }
                    } else if direct {
                        local_video.stop_camera();
                        match offer_same_peer(&current.peer, &layout_plan, layout, false, false, &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                            Ok((browser_offer, signaled)) => {
                                same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Camera(false) });
                                same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                            }
                            Err(error) => {
                                timeline.record("camera offer failed", error.to_string());
                                control.send(CallUpdate::Camera(false));
                            }
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
                Some(CallCommand::Admit { mri }) => {
                    let result = remote.signaling.admit(&remote.conversation, &remote.participant, target_of(&roster, &mri), &links).await;
                    report_organizer_result(result, "admit", "Could not admit that person", control, timeline);
                }
                Some(CallCommand::AdmitAll) => {
                    let result = remote.signaling.admit_all(&remote.conversation, &remote.participant, &links).await;
                    report_organizer_result(result, "admit all", "Could not admit everyone", control, timeline);
                }
                Some(CallCommand::Deny { mri }) => {
                    let result = remote.signaling.remove_participant(&remote.conversation, &remote.participant, target_of(&roster, &mri), &links).await;
                    report_organizer_result(result, "deny", "Could not deny that person", control, timeline);
                }
                Some(CallCommand::RemoveParticipant { mri }) => {
                    let result = remote.signaling.remove_participant(&remote.conversation, &remote.participant, target_of(&roster, &mri), &links).await;
                    report_organizer_result(result, "remove participant", "Could not remove that person", control, timeline);
                }
                Some(CallCommand::MuteParticipant { mri }) => {
                    let result = remote.signaling.mute_participant(&remote.conversation, &remote.participant, &mri).await;
                    report_organizer_result(result, "mute participant", "Could not mute that person", control, timeline);
                }
                Some(CallCommand::MuteAll) => {
                    let others: Vec<String> = roster
                        .members()
                        .iter()
                        .filter(|member| member.mri != own_mri && !member.in_lobby && !member.mri.starts_with(BOT_PREFIX))
                        .map(|member| member.mri.clone())
                        .collect();
                    let result = remote.signaling.mute_everyone(&remote.conversation, &remote.participant, &others).await;
                    report_organizer_result(result, "mute all", "Could not mute everyone", control, timeline);
                }
                Some(CallCommand::Spotlight { mri }) => {
                    let result = remote.signaling.spotlight(&remote.conversation, &remote.participant, &mri).await;
                    report_organizer_result(result, "spotlight", "Could not spotlight that person", control, timeline);
                }
                Some(CallCommand::StopSpotlight { mri }) => {
                    let state_ids: Vec<String> = roster.spotlight_of(&mri).map(|spotlight| spotlight.state_id.clone()).into_iter().collect();
                    if !state_ids.is_empty() {
                        let result = remote.signaling.lower_hands(&remote.conversation, &remote.participant, &state_ids).await.map(|()| 200);
                        report_organizer_result(result, "stop spotlight", "Could not stop the spotlight", control, timeline);
                    }
                }
                Some(CallCommand::SetCaptions(on)) => {
                    caption_flow.want(on);
                    if on {
                        caption_deadline = Some(Instant::now() + CAPTION_START_LIMIT);
                        control.send(CallUpdate::Captions(CaptionState::Starting));
                    } else {
                        caption_deadline = None;
                        control.send(CallUpdate::Captions(CaptionState::Off));
                    }
                    run_bot_steps(BotJob::Captions, &mut caption_flow, &roster, &remote, &links, meeting_chat.as_deref(), "", &mut bot_pending, control, timeline).await;
                    if caption_flow.on() {
                        caption_deadline = None;
                    }
                }
                Some(CallCommand::SetRecording { on, title }) => {
                    if is_meeting {
                        recording_title = title;
                        recording_flow.want(on);
                        recording_deadline = on.then(|| Instant::now() + RECORDING_START_LIMIT);
                        run_bot_steps(BotJob::Recording, &mut recording_flow, &roster, &remote, &links, meeting_chat.as_deref(), &recording_title, &mut bot_pending, control, timeline).await;
                    }
                }
                Some(CallCommand::ConsentToRecording) => {
                    match remote.signaling.consent_to_recording(&remote.conversation, &remote.participant, Consent::Give).await {
                        Ok(_) => {
                            consent_pending = false;
                            control.send(CallUpdate::ConsentRequired(false));
                        }
                        Err(error) => {
                            timeline.record("recording consent failed", error.to_string());
                            control.send(CallUpdate::Notice("Could not confirm the recording notice".into()));
                        }
                    }
                }
                Some(CallCommand::Hold(hold)) => {
                    if is_meeting || !remote_set || local_hold == hold || same_peer.is_some() {
                        continue;
                    }
                    if hold && local_video.camera_on() {
                        local_video.stop_camera();
                        control.send(CallUpdate::Camera(false));
                    }
                    let camera = !hold && local_video.camera_on();
                    match offer_same_peer(&current.peer, &layout_plan, layout, hold, camera, &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                        Ok((browser_offer, signaled)) => {
                            same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Hold(hold) });
                            same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                        }
                        Err(error) => {
                            timeline.record("hold failed", error.to_string());
                            control.send(CallUpdate::Notice(if hold { "Could not put the call on hold".into() } else { "Could not resume the call".into() }));
                        }
                    }
                }
                Some(CallCommand::Transfer { target, replaces }) => {
                    let Some(url) = media_links.get("transfer").cloned() else {
                        control.send(CallUpdate::Notice("This call cannot be transferred".into()));
                        continue;
                    };
                    let transfer_target = TransferTarget { mri: &target.mri, replaces: replaces.as_deref() };
                    match remote.signaling.transfer(&url, &remote.participant, transfer_target, &links).await {
                        Ok(()) => control.send(CallUpdate::Notice(format!("Transferring to {}...", target.display_name))),
                        Err(error) => {
                            timeline.record("transfer failed", error.to_string());
                            control.send(CallUpdate::Notice("Could not transfer the call".into()));
                        }
                    }
                }
                Some(CallCommand::OpenWhiteboard { title }) => {
                    let Some(target) = meeting_target.clone() else {
                        control.send(CallUpdate::Notice("Whiteboards belong to meetings".into()));
                        continue;
                    };
                    let session = inner.session.clone();
                    let updates = control.updates();
                    tokio::spawn(async move {
                        let update = match whiteboard::fetch_board(&session, &target, &title).await {
                            Ok(url) => CallUpdate::WhiteboardUrl(url),
                            Err(_) => CallUpdate::Notice("The whiteboard could not be opened".into()),
                        };
                        let _ = updates.send(update);
                    });
                }
                Some(CallCommand::SetBackground(choice)) => apply_background(choice, &local_video, control, timeline).await,
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
                if caption_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    caption_deadline = None;
                    caption_flow.failed();
                    timeline.record("captions", "did not start in time");
                    control.send(CallUpdate::Captions(CaptionState::Failed("Captions did not start".into())));
                }
                if recording_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    recording_deadline = None;
                    recording_flow.failed();
                    timeline.record("recording", "did not start in time");
                    control.send(CallUpdate::Notice("The recording did not start".into()));
                }
                if ticks.is_multiple_of(BLUR_TIMING_EVERY_TICKS)
                    && let Some(average_ms) = local_video.blur_average_ms()
                {
                    control.send(CallUpdate::BlurTiming(average_ms));
                }
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
            _ = sleep_until(same_peer_deadline.unwrap_or_else(far_future)), if same_peer_deadline.is_some() => {
                same_peer_deadline = None;
                if let Some(pending) = same_peer.take() {
                    timeline.record("same-peer offer", "no answer in time");
                    negotiator.same_peer_offer_failed();
                    rollback_offer(&current.peer).await;
                    undo_change(pending.change, &mut local_video, control);
                }
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
    let session = open_peer(factory, grant, &options.relay_host, false)?;
    let media = create_offer(&session.peer, factory, track, Layout::Conference).await?;
    let body = renegotiation_body(&remote.participant, links, &media.signaled.sdp, media_leg_id, &media.modalities, true);
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
    let session = open_peer(factory, grant, &options.relay_host, false)?;
    let answered = answer_offer(&session.peer, Some(track), teams_offer, AnswerVideo::Reject).await?;
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
const ON_HOLD_NOTICE: &str = "Resume the call first";

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

fn is_own_meeting(own: Option<&MeetingTarget>, target: &MeetingTarget) -> bool {
    own.is_some_and(|own| own.thread_id == target.thread_id)
}

fn peer_transceiver(peer: &PeerConnection, mid: &str) -> Option<RtpTransceiver> {
    peer.transceivers().into_iter().find(|transceiver| transceiver.mid().as_deref() == Some(mid))
}

fn start_camera_line(local_video: &mut LocalVideo, peer: &PeerConnection, direct_mid: Option<&str>, choice: &DeviceChoice) -> Result<()> {
    if !local_video.has_lines() {
        let mid = direct_mid.ok_or_else(|| Error::Webrtc("no video line negotiated".into()))?;
        let camera = peer_transceiver(peer, mid).ok_or_else(|| Error::Webrtc("no transceiver for the camera line".into()))?;
        if !matches!(camera.direction(), RtpTransceiverDirection::SendRecv | RtpTransceiverDirection::SendOnly) {
            peer.add_track(MediaStreamTrack::Video(local_video.camera_track()), &[CAMERA_STREAM])
                .map_err(|error| Error::Webrtc(error.to_string()))?;
        }
        local_video.set_lines(VideoLines { camera, share: None })?;
    }
    local_video.start_camera(choice)
}

#[allow(clippy::too_many_arguments)]
async fn offer_same_peer(
    peer: &PeerConnection,
    plan: &OfferPlan,
    layout: Layout,
    hold: bool,
    camera: bool,
    remote: &RemoteCall,
    links: &CallbackLinks,
    media_leg_id: &str,
    media_links: &BTreeMap<String, String>,
    negotiator: &mut MediaNegotiator,
) -> Result<(String, SignaledOffer)> {
    let url = media_links
        .get("mediaRenegotiation")
        .cloned()
        .ok_or_else(|| Error::Signaling("this call has no mediaRenegotiation link".into()))?;
    if !negotiator.begin_same_peer_offer() {
        return Err(Error::Signaling("another media negotiation is running".into()));
    }
    let offered = build_same_peer_offer(peer, plan, layout, hold, camera).await;
    let (text, signaled) = match offered {
        Ok(offered) => offered,
        Err(error) => {
            negotiator.same_peer_offer_failed();
            return Err(error);
        }
    };
    let modalities = accepted_modalities(layout == Layout::OneToOne && camera && !hold);
    let body = renegotiation_body(&remote.participant, links, &signaled.sdp, media_leg_id, &modalities, false);
    if let Err(error) = remote.signaling.post_json("POST mediaRenegotiation (same peer)", &url, body).await {
        negotiator.same_peer_offer_failed();
        rollback_offer(peer).await;
        return Err(error);
    }
    Ok((text, signaled))
}

async fn build_same_peer_offer(peer: &PeerConnection, plan: &OfferPlan, layout: Layout, hold: bool, camera: bool) -> Result<(String, SignaledOffer)> {
    let offer = peer
        .create_offer(OfferOptions { offer_to_receive_audio: true, offer_to_receive_video: true, ..OfferOptions::default() })
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let text = directions_for(layout, hold, camera, &offer.to_string());
    let description = SessionDescription::parse(&text, SdpType::Offer).map_err(|error| Error::Sdp(error.description))?;
    peer.set_local_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let mut numbers = random_u32;
    let signaled = to_teams_offer(&text, plan, &mut numbers)?;
    Ok((text, signaled))
}

async fn rollback_offer(peer: &PeerConnection) {
    if let Ok(rollback) = SessionDescription::parse("", SdpType::Rollback) {
        let _ = peer.set_local_description(rollback).await;
    }
}

fn undo_change(change: Change, local_video: &mut LocalVideo, control: &CallControl) {
    match change {
        Change::Camera(true) => {
            local_video.stop_camera();
            control.send(CallUpdate::Camera(false));
        }
        Change::Camera(false) => control.send(CallUpdate::Camera(false)),
        Change::Hold(_) => control.send(CallUpdate::Notice("The hold did not go through".into())),
    }
}

fn withhold_media(
    muted: &mut bool,
    audio: &AudioSetup,
    share_audio: Option<&ShareAudio>,
    remote: &RemoteCall,
    local_video: &mut LocalVideo,
    control: &CallControl,
) {
    *muted = apply_mute(MuteCommand::Mute, *muted, audio, share_audio, remote, control);
    if local_video.camera_on() {
        local_video.stop_camera();
        control.send(CallUpdate::Camera(false));
    }
}

async fn apply_background(choice: BackgroundChoice, local_video: &LocalVideo, control: &CallControl, timeline: &Timeline) {
    match choice {
        BackgroundChoice::None => local_video.set_background(false, None),
        BackgroundChoice::Blur => local_video.set_background(true, None),
        BackgroundChoice::Image(path) => match tokio::task::spawn_blocking(move || BackgroundPicture::load(&path)).await {
            Ok(Ok(picture)) => local_video.set_background(true, Some(Arc::new(picture))),
            Ok(Err(error)) => {
                timeline.record("background image failed", error.to_string());
                control.send(CallUpdate::Notice("That background image could not be used".into()));
            }
            Err(_) => {}
        },
    }
}

fn target_of<'a>(roster: &'a Roster, mri: &'a str) -> Target<'a> {
    let display_name = roster.members().iter().find(|member| member.mri == mri).map_or("", |member| member.display_name.as_str());
    Target { mri, display_name }
}

fn report_organizer_result(result: Result<u16>, label: &str, notice: &str, control: &CallControl, timeline: &Timeline) {
    if let Err(error) = &result {
        timeline.record(format!("{label} failed"), error.to_string());
        control.send(CallUpdate::Notice(notice.into()));
    }
    control.send(CallUpdate::Organizer { action: label.to_owned(), outcome: result.map_err(|error| error.to_string()) });
}

fn named_caption(mut entry: CaptionEntry, roster: &Roster) -> CaptionEntry {
    if entry.display_name.is_empty() {
        entry.display_name = roster.name_of(&entry.user_id).unwrap_or_default().to_owned();
    }
    entry
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BotJob {
    Captions,
    Recording,
}

#[allow(clippy::too_many_arguments)]
async fn run_bot_steps(
    job: BotJob,
    flow: &mut CaptionFlow,
    roster: &Roster,
    remote: &RemoteCall,
    links: &CallbackLinks,
    meeting_chat: Option<&str>,
    title: &str,
    bot_pending: &mut bool,
    control: &CallControl,
    timeline: &Timeline,
) {
    let is_active: fn(&crate::roster::CaptionBot) -> bool = match job {
        BotJob::Captions => |bot| bot.active,
        BotJob::Recording => |bot| bot.recording,
    };
    let fail = |flow: &mut CaptionFlow, reason: &str| {
        flow.failed();
        control.send(match job {
            BotJob::Captions => CallUpdate::Captions(CaptionState::Failed(reason.to_owned())),
            BotJob::Recording => CallUpdate::Notice(reason.to_owned()),
        });
    };
    let (join_failure, start_failure) = match job {
        BotJob::Captions => ("The captions service could not join", "Captions could not start"),
        BotJob::Recording => ("The recording service could not join", "The recording could not start"),
    };
    loop {
        match flow.next_for(roster.caption_bot(), is_active) {
            CaptionStep::Wait => return,
            CaptionStep::AddBot => {
                if *bot_pending {
                    continue;
                }
                let Some(thread) = meeting_chat else {
                    return fail(flow, "This meeting has no chat for the captions and the recording");
                };
                *bot_pending = true;
                let added = remote.signaling.add_caption_bot(&remote.conversation, &remote.participant, thread, links).await;
                if job == BotJob::Recording {
                    report_organizer_result(added.as_ref().map(|status| *status).map_err(|error| Error::Signaling(error.to_string())), "recording add bot", join_failure, control, timeline);
                }
                if let Err(error) = added {
                    timeline.record("bot invitation failed", error.to_string());
                    *bot_pending = false;
                    return fail(flow, join_failure);
                }
            }
            CaptionStep::Start => {
                let Some(url) = roster.caption_bot().and_then(|bot| bot.command_url.clone()) else { return };
                match job {
                    BotJob::Captions => {
                        if let Err(error) = remote.signaling.caption_command(&url, BotAction::Start, &remote.participant).await {
                            timeline.record("caption start failed", error.to_string());
                            return fail(flow, start_failure);
                        }
                        if let Err(error) = remote.signaling.set_caption_preference(&remote.conversation, &remote.participant, true).await {
                            timeline.record("caption preference failed", error.to_string());
                        }
                    }
                    BotJob::Recording => {
                        let started = remote.signaling.recording_command(&url, BotAction::Start, &remote.participant, title).await;
                        let failed = started.is_err();
                        report_organizer_result(started, "recording start", start_failure, control, timeline);
                        if failed {
                            return fail(flow, start_failure);
                        }
                    }
                }
            }
            CaptionStep::Stop => {
                let Some(url) = roster.caption_bot().and_then(|bot| bot.command_url.clone()) else { continue };
                match job {
                    BotJob::Captions => {
                        if let Err(error) = remote.signaling.caption_command(&url, BotAction::Stop, &remote.participant).await {
                            timeline.record("caption stop failed", error.to_string());
                        }
                        if let Err(error) = remote.signaling.set_caption_preference(&remote.conversation, &remote.participant, false).await {
                            timeline.record("caption preference failed", error.to_string());
                        }
                    }
                    BotJob::Recording => {
                        let stopped = remote.signaling.recording_command(&url, BotAction::Stop, &remote.participant, title).await;
                        report_organizer_result(stopped, "recording stop", "The recording could not be stopped", control, timeline);
                    }
                }
            }
            CaptionStep::ReportOn => {
                if job == BotJob::Captions {
                    control.send(CallUpdate::Captions(CaptionState::On));
                }
            }
        }
    }
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

    const BROWSER_OFFER: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\na=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\na=mid:1\r\na=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\na=mid:2\r\na=recvonly\r\n";

    fn directions(sdp: &str) -> Vec<String> {
        crate::sdp::parse(sdp).unwrap().media.iter().map(|line| line.direction().unwrap_or("none").to_owned()).collect()
    }

    #[test]
    fn a_one_to_one_offer_has_audio_and_an_inactive_camera_until_the_camera_is_on() {
        assert_eq!(directions(&directions_for(Layout::OneToOne, false, false, BROWSER_OFFER)), vec!["sendrecv", "inactive", "inactive"]);
        assert_eq!(directions(&directions_for(Layout::OneToOne, false, true, BROWSER_OFFER)), vec!["sendrecv", "sendrecv", "inactive"]);
    }

    #[test]
    fn a_hold_silences_every_line_and_a_conference_offer_is_left_alone() {
        for layout in [Layout::OneToOne, Layout::Conference] {
            assert_eq!(directions(&directions_for(layout, true, true, BROWSER_OFFER)), vec!["inactive", "inactive", "inactive"]);
        }
        assert_eq!(directions_for(Layout::Conference, false, true, BROWSER_OFFER), BROWSER_OFFER);
    }

    #[test]
    fn a_move_into_the_meeting_you_are_already_in_is_ignored() {
        let target = |thread: &str| MeetingTarget { thread_id: thread.into(), tenant_id: "t".into(), organizer_id: "o".into(), meeting_data: None };
        assert!(is_own_meeting(Some(&target("19:meeting_a@thread.v2")), &target("19:meeting_a@thread.v2")));
        assert!(!is_own_meeting(Some(&target("19:meeting_a@thread.v2")), &target("19:meeting_b@thread.v2")));
        assert!(!is_own_meeting(None, &target("19:meeting_a@thread.v2")));
    }

    #[test]
    fn a_video_call_names_the_video_modality_only_when_the_camera_sends() {
        assert_eq!(accepted_modalities(false), vec!["Audio".to_owned()]);
        assert_eq!(accepted_modalities(true), vec!["Audio".to_owned(), "Video".to_owned()]);
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
