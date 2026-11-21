mod audio_switch;
mod bots;
mod escalation;
mod peer_sdp;
mod stats;
mod teardown;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use self::{audio_switch::*, bots::*, escalation::*, peer_sdp::*, stats::*, teardown::*};

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

use crate::active_speakers;
use crate::audio::{SAMPLE_RATE, rms, tone_ratio, write_wav};
use crate::audio_io::{AudioMode, AudioSetup};
use crate::background::{BackgroundChoice, BackgroundPicture};
use crate::breakout::{self, RoomWatcher};
use crate::callee_gate::{CalleeGate, EchoWatch, roster_has_callee};
use crate::camera::{CameraDevice, list_cameras};
use crate::candidates::{SdpKind, is_udp_relay, negotiated_setup, with_candidates};
use crate::captions::{
    BotAction, CAPTIONS_DATA_ID, CaptionEntry, CaptionFlow, CaptionState, CaptionStep,
    parse_caption_message,
};
use crate::control::{CallCommand, CallControl, CallUpdate, Progress};
use crate::data_channel::Depacketizer;
use crate::device_watch::{self, Debounce};
use crate::devices::{self, AppliedDevice, AudioDevice, DeviceChoice, Flow, Reselect};
use crate::diagnostics::{CallDiagnostics, StatsSample};
use crate::end::EndKind;
use crate::engine::{CallEngine, EngineConfig, Inner};
use crate::error::{Error, Result};
use crate::health::{
    CallHealth, HealthCounters, HealthSampler, QualityTracker, TransportDetails, health_from_stats,
};
use crate::hold::{
    HoldState, INACTIVE, MediaLine, accepted_modalities, direction_of, is_hold, keep_media_running,
    offer_modalities, offer_sends_video, rewrite_directions, stop_sending_on,
};
use crate::meeting_options::{self, OptionWrite};
use crate::mic_processing::MicSettings;
use crate::mute::{MuteCommand, MuteEffect};
use crate::notices::{
    ADMIT_ALL_FAILED_NOTICE, ADMIT_FAILED_NOTICE, ATTENDEE_SETTINGS_FAILED_NOTICE,
    BACKGROUND_IMAGE_FAILED_NOTICE, CALL_TRANSFERRED_NOTICE, CAMERA_RESTART_NOTICE,
    CAMERA_SETTING_FAILED_NOTICE, CAMERA_START_NOTICE, CAPTION_LANGUAGE_FAILED_NOTICE,
    CAPTIONS_JOIN_FAILED_NOTICE, CAPTIONS_START_FAILED_NOTICE, CHANGE_ROLE_FAILED_NOTICE,
    CHANGE_RUNNING_NOTICE, CONFIRM_RECORDING_FAILED_NOTICE, CONSENT_FIRST_NOTICE,
    DENY_FAILED_NOTICE, ECHO_TEST_FAILED_NOTICE, HOLD_CHANGE_FAILED_NOTICE, HOLD_FAILED_NOTICE,
    INVITE_PERSON_FAILED_NOTICE, LOWER_ALL_HANDS_FAILED_NOTICE, LOWER_HAND_FAILED_NOTICE,
    LOWER_THAT_HAND_FAILED_NOTICE, MEETING_LOCK_FAILED_NOTICE, MIC_SETTING_FAILED_NOTICE,
    MUTE_ALL_FAILED_NOTICE, MUTE_PARTICIPANT_FAILED_NOTICE, NOT_CONNECTED_FOR_VIDEO,
    ON_HOLD_NOTICE, RAISE_HAND_FAILED_NOTICE, REACTION_FAILED_NOTICE, RECORDING_JOIN_FAILED_NOTICE,
    RECORDING_NOT_STARTED_NOTICE, RECORDING_START_FAILED_NOTICE, RECORDING_STOP_FAILED_NOTICE,
    REMOVE_PARTICIPANT_FAILED_NOTICE, RESUME_FAILED_NOTICE, SHARE_QUEUED_NOTICE,
    SHARE_REFUSED_NOTICE, SHARING_START_NOTICE, SPOTLIGHT_FAILED_NOTICE,
    STOP_SPOTLIGHT_FAILED_NOTICE, TRANSFER_CALL_FAILED_NOTICE, TRANSFER_FAILED_NOTICE,
    TRANSFER_NOT_ALLOWED_NOTICE, TRANSFERRING_NOTICE, VIDEO_FAILED_NOTICE,
    WHITEBOARD_OPEN_FAILED_NOTICE, WHITEBOARDS_MEETINGS_ONLY_NOTICE, notice_with_detail,
};
use crate::organizer::{MeetingCapability, Modality, StateStep, Target, modality_steps};
use crate::outcome::{embedded_end, failed_participant_count, outcome_summary};
use crate::push::CallNotification;
use crate::recording::{Consent, RecordingPlan, RecordingStatus};
use crate::relay::{DEFAULT_RELAY_HOST, RelayGrant};
use crate::renegotiation::{MediaAction, MediaNegotiator, MediaPath, RejectedOffer};
use crate::roster::{Roster, direct_roster};
use crate::screen::{
    ShareKind, SharePicker, ShareSource, current_share_picker, list_share_sources,
    share_sources_for_menu, share_thumbnail,
};
use crate::sdp::{
    LineRole, OfferPlan, RemoteOffer, SignaledOffer, from_teams_offer, stream_lines,
    to_browser_answer, to_teams_answer, to_teams_offer,
};
use crate::send_control::{BitrateGovernor, RequestFilter, parse_request};
use crate::signaling::{
    Answer, Attached, Callee, Conversation, Invitation, InviteTarget, LeaveReason, MeetingTarget,
    NegotiationFailure, Participant, Signaling, TenantRouting, chat_thread_id,
    ensure_echo_bot_thread, escalation_answer_body, one_to_one_thread, renegotiation_body,
};
use crate::software_audio::{SoftwareAudio, SoftwareAudioPlan};
use crate::state::{CallSignal, CallState};
use crate::timeline::{Timeline, TimelineEntry};
use crate::transfer::{Replacement, TransferEvent, TransferTarget};
use crate::trouter_events::{
    CallEnd, CallEvent, CallbackLinks, ProgressStatus, callback_tag, classify, decode_body,
    nested_links, object_keys,
};
use crate::video_frame::{VideoKey, VideoPicture};
use crate::video_layout::{
    CAMERA_MID, MediaDescription, ONE_TO_ONE_SHARE_MID, SCREEN_STREAM, SHARE_MID,
    SendCapabilityReporter, SlotTable, VideoLines, VideoRouter, add_one_to_one_lines,
    add_share_line, add_video_lines, offer_plan, one_to_one_plan,
};
use crate::video_receive::{
    ReceiveChange, SourceRequester, VideoCounters, VideoReceive, spawn_video_pump,
};
use crate::video_send::{LocalVideo, SendSource, VideoMode};
use crate::whiteboard;

const END_CALLBACK_WAIT: Duration = Duration::from_secs(4);
const STATS_PERIOD: Duration = Duration::from_millis(200);
const TICKS_PER_SECOND: u32 = 5;
const RECONNECT_WINDOW: Duration = Duration::from_secs(30);
const OUTGOING_TIMEOUT: Duration = Duration::from_secs(90);
const BROKER_IDLE_WAIT: Duration = Duration::from_millis(500);
const BROKER_RETRY_WAIT: Duration = Duration::from_secs(3);
const CAMERA_EFFECTS_FAILED_NOTICE: &str =
    "Background effects could not start, your camera is sent without them";
const SETUP_STEP_TIMEOUT: Duration = Duration::from_secs(30);
const DIRECT_OUTGOING_TIMEOUT: Duration = Duration::from_secs(185);
const FAR_FUTURE: Duration = Duration::from_secs(60 * 60 * 24 * 365);
const DEFAULT_KEEP_ALIVE_SECONDS: u64 = 2700;
const MIN_KEEP_ALIVE_SECONDS: u64 = 60;
const KEEP_ALIVE_FRACTION: f64 = 0.9;
const MIC_STREAM: &str = "microphone";
const DATA_CHANNEL_LABEL: &str = "main-channel";
pub const DATA_CHANNEL_ENV: &str = "CALLING_DATA_CHANNEL";
const CAPTION_START_LIMIT: Duration = Duration::from_secs(40);
const SEND_CAPABILITIES_LABEL: &str = "POST applyChannelParameters";
const BLUR_TIMING_EVERY_TICKS: u32 = TICKS_PER_SECOND * 2;
const HEALTH_EVERY_TICKS: u32 = TICKS_PER_SECOND * 2;
const BOT_PREFIX: &str = "28:";
const RECORDING_START_LIMIT: Duration = Duration::from_secs(40);
const SAME_PEER_ANSWER_LIMIT: Duration = Duration::from_secs(40);
/// Teams sometimes answers a few seconds after the soft limit; keep the local offer until this grace ends.
const SAME_PEER_LATE_GRACE: Duration = Duration::from_secs(15);
const GATHERING_LIMIT: Duration = Duration::from_secs(5);
const RELAY_SETTLE: Duration = Duration::from_millis(200);
const GATHERING_POLL: Duration = Duration::from_millis(50);
const NEXT_PEER_LIMIT: Duration = Duration::from_secs(40);
const GLARE_RESEND_LIMIT: Duration = Duration::from_secs(3);
const GLARE_CODE: i64 = 491;
const RETARGET_WAIT: Duration = Duration::from_secs(20);
const CAMERA_STREAM: &str = "camera";

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
    pub start_camera: bool,
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
            start_camera: false,
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
    People {
        callees: Vec<Callee>,
        thread_id: String,
    },
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

    fn callee_count(&self) -> usize {
        match self {
            CallSpec::People { callees, .. } => callees.len(),
            CallSpec::Echo => 1,
            CallSpec::Meeting(_) => 0,
        }
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
    !matches!(
        std::env::var(DATA_CHANNEL_ENV).ok().as_deref(),
        Some("0" | "off")
    )
}

pub fn keep_alive_period(seconds: Option<u64>) -> Duration {
    let seconds = seconds
        .unwrap_or(DEFAULT_KEEP_ALIVE_SECONDS)
        .max(MIN_KEEP_ALIVE_SECONDS);
    Duration::from_secs_f64(seconds as f64 * KEEP_ALIVE_FRACTION)
}

fn random_u32() -> u32 {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) & 0x7fff_ffff
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

fn remember_call_leg(live: &mut Live, url: Option<&str>) {
    let Some(url) = url.filter(|url| !url.is_empty()) else {
        return;
    };
    if !live.call_legs.iter().any(|known| known == url) {
        live.call_legs.push(url.to_owned());
    }
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
        dump_dir: None,
        instance: chatsvc::InstanceNames {
            global: "__testCallTrouter".into(),
            binding: "__testCallRealtime".into(),
            endpoint_storage_key: "__testCallEpid".into(),
        },
    };
    let (engine, _events) =
        CallEngine::start(session.clone(), poll_session.clone(), config).await?;
    let report = engine
        .run(Plan::Outgoing(CallSpec::Echo), options, control)
        .await;
    engine.stop().await;
    report
}

impl CallEngine {
    pub(crate) async fn run(
        &self,
        plan: Plan,
        options: CallOptions,
        mut control: CallControl,
    ) -> Result<CallReport> {
        let inner = self.inner.as_ref();
        let _one_call_at_a_time = if options.concurrent {
            None
        } else {
            match control.until_hangup(inner.gate.lock()).await {
                Ok(guard) => Some(guard),
                Err(_) => {
                    if let Plan::Incoming(incoming) = &plan {
                        inner.unregister_route(incoming.links.call_id());
                        if let Some(call_leg) = incoming.attached.links.get("callLeg") {
                            let signaling = inner.signaling(Timeline::new(options.trace));
                            let _ = signaling.decline(call_leg).await;
                        }
                    }
                    let mut state = CallState::Idle;
                    advance(&mut state, CallSignal::Dial, &control);
                    advance(&mut state, CallSignal::LocalLeave, &control);
                    return Ok(CallReport::default());
                }
            }
        };
        let timeline = Timeline::new(options.trace);
        let stay_awake = crate::awake::StayAwake::new();
        timeline.record(
            "stay awake",
            if stay_awake.holding() {
                "system and display kept awake"
            } else {
                "not available"
            },
        );
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
        dump_timeline(&options, &timeline, failure.as_ref());
        let cancelled = live.cancel_leave
            || matches!(
                &state,
                CallState::Ended {
                    reason: crate::state::EndReason::Cancelled
                }
            );
        tear_down(live, inner, &timeline, &mut report, cancelled).await;
        let recording = recording.lock().await;
        report.recorded_samples = recording.samples.len();
        report.timeline = timeline.entries();
        control.send(CallUpdate::Diagnostics(Box::new(CallDiagnostics::new(
            &report,
            timeline.sdp_summaries(),
        ))));
        if let Some(path) = &options.wav_path {
            write_wav(path, &recording.samples)
                .map_err(|error| Error::Webrtc(format!("wav: {error}")))?;
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(report),
        }
    }
}

async fn setup_step<T, E>(
    control: &mut CallControl,
    step: impl std::future::Future<Output = std::result::Result<T, E>>,
) -> Result<T>
where
    Error: From<E>,
{
    setup_step_within(control, SETUP_STEP_TIMEOUT, step).await
}

async fn setup_step_within<T, E>(
    control: &mut CallControl,
    limit: Duration,
    step: impl std::future::Future<Output = std::result::Result<T, E>>,
) -> Result<T>
where
    Error: From<E>,
{
    match control.until_hangup(timeout(limit, step)).await? {
        Ok(result) => Ok(result?),
        Err(_) => Err(Error::Signaling("Teams did not answer in time".into())),
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
    control.send(CallUpdate::OwnIdentity {
        mri: own_mri.clone(),
    });
    let (spec, incoming) = match plan {
        Plan::Outgoing(spec) => (Some(spec), None),
        Plan::Incoming(incoming) => (None, Some(*incoming)),
    };
    let accept_mode = incoming
        .as_ref()
        .map_or(AcceptMode::Audio, |incoming| incoming.accept);
    let (links, participant, mut callbacks, incoming_media) = match incoming {
        Some(IncomingCall {
            accept: _,
            notification,
            attached,
            links,
            participant,
            callbacks,
        }) => (
            links,
            participant,
            callbacks,
            Some((notification, attached)),
        ),
        None => {
            let links = inner.callback_links();
            let participant = inner.participant(&uuid::Uuid::new_v4().to_string());
            let callbacks = inner.register_route(&links);
            (links, participant, callbacks, None)
        }
    };
    live.route_id = Some(links.call_id().to_owned());
    timeline.record("Trouter connected", "callback socket ready");

    let thread_id = if matches!(spec, Some(CallSpec::Echo)) {
        let thread_id = control
            .until_hangup(ensure_echo_bot_thread(
                &inner.session,
                &inner.identity.object_id,
            ))
            .await??;
        timeline.record("echo bot chat", thread_id.clone());
        Some(thread_id)
    } else {
        None
    };
    let grant = control
        .until_hangup(inner.relay.grant(&inner.session))
        .await??;
    timeline.record(
        "GET trap/tokens",
        format!("relay grant, expires in {} s", grant.expires_in_seconds),
    );

    let factory = devices::shared_factory();
    let mut input = options.input.clone();
    let mut output = options.output.clone();
    let audio = AudioSetup::start(&factory, options.audio, options.tone_hz, &input, &output);
    control.send(CallUpdate::ListenOnly(audio.listen_only));
    let mut device_lists = devices::list_devices(&factory);
    let mut applied_input = devices::applied_device(
        &device_lists.inputs,
        &input,
        devices::system_default_id(Flow::Capture).as_deref(),
    );
    let mut applied_output = devices::applied_device(
        &device_lists.outputs,
        &output,
        devices::system_default_id(Flow::Render).as_deref(),
    );
    let (_device_watcher, mut device_changes) = device_watch::watch();
    let mut device_debounce = Debounce::default();
    timeline.record(
        "audio setup",
        format!(
            "listen only {}, {} input(s), {} output(s); {}",
            audio.listen_only,
            device_lists.inputs.len(),
            device_lists.outputs.len(),
            audio.adm_state
        ),
    );
    record_audio_devices(
        timeline,
        &device_lists,
        applied_input.as_ref(),
        applied_output.as_ref(),
    );
    control.send(CallUpdate::Devices(device_lists.clone()));
    control.send(CallUpdate::Selected {
        input: input.clone(),
        output: output.clone(),
    });
    let mut track = audio.track.clone();
    live.audio = Some(audio);
    let mut local_video = LocalVideo::new(
        &factory,
        options.video,
        control.video.clone(),
        control.updates(),
    );
    local_video.attach_timeline(timeline.clone());
    local_video.attach_commands(control.command_sender());
    let mut camera_choice = options.camera.clone();
    if options.start_camera {
        match local_video.preview_camera(&camera_choice) {
            Ok(()) => control.send(CallUpdate::Camera(true)),
            Err(error) => timeline.record("camera preview failed", error.to_string()),
        }
    }
    let mut media_description_request = 1u32;
    refresh_cameras(control.updates(), options.video, timeline.clone());
    control.send(CallUpdate::SharePicker(current_share_picker()));
    refresh_share_sources(control.updates(), options.video, false);

    let wants_data_channel =
        options.data_channel && spec.as_ref().is_some_and(CallSpec::is_meeting);
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
    let mut outgoing_limit = OUTGOING_TIMEOUT;
    let is_meeting = spec.as_ref().is_some_and(CallSpec::is_meeting);
    let is_echo = matches!(spec, Some(CallSpec::Echo));
    let mut echo_watch = EchoWatch::default();
    let mut callee_gate = CalleeGate::new(spec.as_ref().map_or(0, CallSpec::callee_count));
    let meeting_target = match &spec {
        Some(CallSpec::Meeting(target)) => Some(target.clone()),
        _ => None,
    };
    let people_thread = match &spec {
        Some(CallSpec::People { thread_id, .. }) if !thread_id.is_empty() => {
            Some(thread_id.clone())
        }
        _ => incoming_media
            .as_ref()
            .filter(|(notification, _)| !notification.is_multi_party)
            .and_then(|(notification, _)| {
                notification
                    .thread_id
                    .clone()
                    .filter(|thread| *thread != notification.caller.mri)
                    .or_else(|| {
                        one_to_one_thread(&inner.identity.object_id, &notification.caller.mri)
                    })
            }),
    };
    let mut layout = match &spec {
        Some(CallSpec::People { callees, .. }) if callees.len() == 1 => Layout::OneToOne,
        _ => Layout::Conference,
    };
    let peer_mri = match &spec {
        Some(CallSpec::People { callees, .. }) if callees.len() == 1 => {
            Some(callees[0].mri.clone())
        }
        _ => incoming_media
            .as_ref()
            .map(|(notification, _)| notification.caller.mri.clone()),
    };
    let mut layout_plan: OfferPlan;
    let mut direct_video_mid: Option<String> = None;
    let mut sending_camera = false;
    let mut media_links: BTreeMap<String, String> = BTreeMap::new();

    let conversation = match (spec, incoming_media) {
        (Some(spec), None) => {
            let direct_invite = spec.target().is_one_to_one() && !options.concurrent;
            let offer_started = Instant::now();
            let media = control
                .until_hangup(create_offer(
                    &current.peer,
                    &factory,
                    &track,
                    layout,
                    direct_invite.then_some(&current.gathered),
                ))
                .await??;
            timeline.record(
                "offer created",
                format!("{} ms", offer_started.elapsed().as_millis()),
            );
            layout_plan = media.plan.clone();
            if layout == Layout::OneToOne {
                direct_video_mid = Some(CAMERA_MID.to_owned());
            }
            timeline.record(
                "setLocalDescription",
                format!("{} m-line(s) signaled", media.signaled.lines.len()),
            );
            timeline.record_sdp("local offer", &media.signaled.sdp);
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
                direct: !options.concurrent,
            };
            let threadless = invitation.is_direct();
            let conversation = match &target {
                InviteTarget::Meeting(meeting) => {
                    let subscribed = control
                        .until_hangup(signaling.subscribe_meeting(&participant, &links, meeting))
                        .await??;
                    setup_step(control, signaling.join_meeting(&subscribed, &invitation)).await?
                }
                _ => setup_step(control, signaling.create_conversation(&invitation)).await?,
            };
            // People and the Echo test call share outgoing ringback while connecting.
            if spec.is_people() || matches!(spec, CallSpec::Echo) {
                control.send(CallUpdate::Progress(Progress::Calling));
            }
            if spec.is_people() {
                if threadless {
                    outgoing_limit = DIRECT_OUTGOING_TIMEOUT;
                }
                outgoing_deadline = Some(Instant::now() + outgoing_limit);
            }
            if let Err(error) = local_video.set_lines(media.video.clone()) {
                timeline.record("video lines not attached", error.to_string());
            }
            offered = Some(media);
            live.remote = Some(RemoteCall {
                signaling: signaling.clone(),
                conversation: conversation.clone(),
                participant: participant.clone(),
                threadless,
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
                threadless: notification.thread_id.is_none(),
            });
            let one_to_one = !attached.from_mixer && !notification.is_multi_party;
            layout = if one_to_one {
                Layout::OneToOne
            } else {
                Layout::Conference
            };
            let camera_track = local_video.camera_track();
            let video_answer = match accept_mode {
                _ if !one_to_one => AnswerVideo::Reject,
                AcceptMode::Video if local_video.is_available() => AnswerVideo::Send(&camera_track),
                _ => AnswerVideo::Receive,
            };
            dump_sdp(options, "3-teams-offer.sdp", &teams_offer);
            let answered = answer_offer(
                &current.peer,
                Some(&track),
                &teams_offer,
                video_answer,
                None,
                false,
                one_to_one.then_some(&current.gathered),
            )
            .await?;
            layout_plan = answered.remote.plan();
            direct_video_mid = answered.camera_mid.clone();
            timeline.record(
                "setRemoteDescription",
                format!(
                    "offer applied, {} signaled line(s)",
                    answered.remote.lines.len()
                ),
            );
            if one_to_one {
                timeline.record("ice gathered", current.gathered.summary());
            }
            timeline.record_sdp("remote offer", &teams_offer);
            timeline.record_sdp("local answer", &answered.answer_sdp);
            dump_sdp(options, "4-teams-answer.sdp", &answered.answer_sdp);
            let acceptance_url = attached.links.get("acceptance").ok_or_else(|| {
                Error::Signaling("attach answer without an acceptance link".into())
            })?;
            let modalities = accepted_modalities(
                matches!(video_answer, AnswerVideo::Send(_)) && answered.camera_mid.is_some(),
            );
            let acknowledged = setup_step(
                control,
                signaling.accept(
                    acceptance_url,
                    &Answer {
                        from: &participant,
                        answer_sdp: &answered.answer_sdp,
                        media_leg_id: &media_leg_id,
                        callbacks: &links,
                        modalities: &modalities,
                    },
                ),
            )
            .await?;
            negotiator.answered_incoming(attached.from_mixer);
            remote_set = true;
            media_links.extend(attached.links.clone());
            media_links.extend(acknowledged);
            if let (AnswerVideo::Send(_), Some(mid)) = (video_answer, &answered.camera_mid) {
                let camera = peer_transceiver(&current.peer, mid);
                match camera.map(|camera| {
                    local_video.set_lines(VideoLines {
                        camera,
                        share: None,
                    })
                }) {
                    Some(Ok(())) => match local_video.start_camera(&camera_choice) {
                        Ok(()) => sending_camera = true,
                        Err(error) => control.send(CallUpdate::Notice(notice_with_detail(
                            CAMERA_START_NOTICE,
                            error,
                        ))),
                    },
                    Some(Err(error)) => {
                        timeline.record("video lines not attached", error.to_string())
                    }
                    None => timeline.record(
                        "video lines not attached",
                        "no transceiver for the camera line",
                    ),
                }
            }
            keep_alive_link = media_links.get("callLeg").cloned();
            remember_call_leg(live, keep_alive_link.as_deref());
            keep_alive_at = Some(Instant::now() + keep_alive_period(None));
            conversation
        }
        _ => {
            return Err(Error::Callback(
                "call plan without an offer or an invitation".into(),
            ));
        }
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
    live.audio
        .as_ref()
        .expect("audio set above")
        .track
        .set_enabled(initial.track_enabled);
    if muted {
        control.send(CallUpdate::Muted(true));
    }
    match setup_step(
        control,
        signaling.update_endpoint_state(&conversation, &participant, initial.endpoint_is_muted),
    )
    .await
    {
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(error) => timeline.record("updateEndpointState failed", error.to_string()),
        Ok(()) => {}
    }

    let mut stats_tick = interval(STATS_PERIOD);
    let mut speaking_sources: Vec<u32> = Vec::new();
    let mut speaker_sources_seen = false;
    let hard_deadline = options.hard_limit.map(|limit| call_started + limit);
    let mut leave_at = hard_deadline;
    let mut reconnect_deadline: Option<Instant> = None;
    let mut next: Option<NextPeer> = None;
    let mut roster = Roster::default();
    if let Some(snapshot) = &conversation.roster
        && roster.apply_snapshot(snapshot)
    {
        let entries = roster.entries();
        timeline.record(
            "roster",
            format!(
                "{} participant(s) from the conversation answer",
                entries.len()
            ),
        );
        control.send(CallUpdate::Roster(entries));
    }
    let mut lobby = false;
    let mut capability = MeetingCapability::default();
    let mut previous_packets = 0u64;
    let mut ticks = 0u32;
    let mut segmentation_model: Option<&'static str> = None;
    let mut health_sampler = HealthSampler::default();
    let mut quality_tracker = QualityTracker::default();
    let mut second = 0u32;
    let echo_thread = thread_id;
    let mut video = VideoReceive::new(own_mri.clone(), VideoRouter::default());
    if let (Some(mid), Some(mri)) = (&direct_video_mid, &peer_mri) {
        video.router().set(mid, Some(VideoKey::Person(mri.clone())));
    }
    if sending_camera {
        control.send(CallUpdate::Camera(true));
    }
    let mut software_audio: Option<SoftwareAudio> = None;
    let mut share_sound_wanted = false;
    let mut mic_settings = MicSettings::default();
    let mut audio_disabled = false;
    let mut remote_audio: Option<RtcAudioTrack> = None;
    sync_software_audio(
        &factory,
        options,
        &input,
        muted,
        None,
        &current.peer,
        &track,
        software_wanted(
            mic_settings,
            share_sound_wanted,
            &local_video,
            live.audio.as_ref(),
        ),
        false,
        &mut software_audio,
        mic_settings,
        control,
        timeline,
    );
    let mut own_hand: Option<String> = None;
    let mut depacketizer = Depacketizer::default();
    let mut caption_flow = CaptionFlow::default();
    let mut caption_deadline: Option<Instant> = None;
    let mut recording_flow = CaptionFlow::default();
    let mut recording_deadline: Option<Instant> = None;
    let mut recording_plan = RecordingPlan::default();
    let mut recording_audio_only = false;
    let mut recording_status = RecordingStatus::default();
    let mut bot_pending = false;
    let mut consent_pending = false;
    let mut room_watcher = RoomWatcher::default();
    let mut main_meeting_sent = false;
    let mut local_hold = false;
    let mut remote_hold = false;
    let mut same_peer: Option<SamePeerOffer> = None;
    let mut send_requests = RequestFilter::default();
    let mut send_capabilities = SendCapabilityReporter::default();
    let mut bitrate_governor = BitrateGovernor::default();
    let mut same_peer_deadline: Option<Instant> = None;
    let mut same_peer_grace_until: Option<Instant> = None;
    let mut meeting_chat = conversation.chat_thread.clone();
    if let Some(thread) = &meeting_chat {
        control.send(CallUpdate::MeetingChat(thread.clone()));
    }
    if let Some(target) = &meeting_target {
        control.send(CallUpdate::MeetingTenant(target.tenant_id.clone()));
    }
    if meeting_chat.is_none() {
        meeting_chat = people_thread;
    }
    let mut start_camera_pending = options.start_camera;
    let mut share_pending: Option<ShareSource> = None;
    let mut portal_pending: Option<ShareKind> = None;
    let mut glare_resend: Option<GlareResend> = None;
    let mut deferred_changes: Vec<Change> = Vec::new();
    let mut remote_direct_share: Option<String> = None;
    let mut sent_replacement: Option<Replacement> = None;
    let (echo_add_tx, mut echo_add_rx) = mpsc::unbounded_channel::<Result<()>>();
    let mut echo_add_pending = false;
    loop {
        let possible = video_possible(remote_set, layout, negotiator.path());
        if camera_may_start(start_camera_pending, next.is_some(), possible) {
            start_camera_pending = false;
            control.defer(CallCommand::SetCamera(true));
        }
        if camera_may_start(share_pending.is_some(), next.is_some(), possible)
            && let Some(source) = share_pending.take()
        {
            control.defer(CallCommand::StartShare(source));
        }
        if camera_may_start(portal_pending.is_some(), next.is_some(), possible)
            && let Some(kind) = portal_pending.take()
        {
            control.defer(CallCommand::StartPortalShare(kind));
        }
        if next.as_ref().is_some_and(|pending| {
            pending.connected && (pending.offered.is_some() || pending.retargeted)
        }) && let Some(pending) = next.take()
        {
            timeline.record(
                "escalation",
                "new peer connection connected, old one dropped",
            );
            deferred_changes.clear();
            negotiator.server_escalation_done();
            // New mixer leg starts sequence numbers from 1 and has not heard our send capabilities yet.
            send_requests = RequestFilter::default();
            send_capabilities = SendCapabilityReporter::default();
            if let Some(mid) = remote_direct_share.take() {
                video.router().set(&mid, None);
                control.send(CallUpdate::ScreenShare(None));
            }
            // Detach local senders before closing: otherwise peer.close can hang
            // while the camera/share capture threads still feed the old track.
            local_video.clear_lines();
            replace_leg_media_links(&mut media_links, &pending.leg_links);
            let leg_names = pending
                .leg_links
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            timeline.record(
                "leg links",
                if leg_names.is_empty() {
                    "none".to_owned()
                } else {
                    leg_names
                },
            );
            if let Some(url) = media_links.get("callLeg").cloned()
                && keep_alive_link.as_deref() != Some(url.as_str())
            {
                keep_alive_link = Some(url.clone());
                remember_call_leg(live, Some(&url));
            }
            if let Some(track) = pending.remote_audio {
                remote_audio = Some(track.clone());
                if options.record_remote {
                    tokio::spawn(record_track(track, recording.clone(), options.tone_hz));
                }
            }
            let old = std::mem::replace(&mut current, pending.session);
            old.peer.close();
            reconnect_deadline = None;
            if callee_gate.media_connected() {
                advance(state, CallSignal::MediaConnected, control);
            }
            if let Some(audio) = &software_audio
                && let Err(error) = swap_audio_track(&current.peer, &audio.track)
            {
                timeline.record("computer sound not carried over", error.to_string());
            }
            if remote_audio.is_some() {
                sync_software_audio(
                    &factory,
                    options,
                    &input,
                    muted,
                    remote_audio.clone(),
                    &current.peer,
                    &track,
                    software_wanted(
                        mic_settings,
                        share_sound_wanted,
                        &local_video,
                        live.audio.as_ref(),
                    ),
                    true,
                    &mut software_audio,
                    mic_settings,
                    control,
                    timeline,
                );
            }
            offered = pending.offered;
            let answered = pending.answered;
            if let Some(mid) = direct_video_mid.take() {
                video.router().set(&mid, None);
            }
            if let Some(media) = &offered {
                layout = Layout::Conference;
                layout_plan = media.plan.clone();
                if let Err(error) = local_video.set_lines(media.video.clone()) {
                    timeline.record("video lines not attached", error.to_string());
                }
                tell_bound_camera(
                    &remote,
                    &media_links,
                    &local_video,
                    &mut media_description_request,
                    timeline,
                );
                tell_bound_share(
                    &remote,
                    &media_links,
                    &local_video,
                    &mut media_description_request,
                    timeline,
                );
            } else if let Some(answered) = answered {
                // Server escalation answered on a new peer: drop the old peer's
                // senders and bind the camera/share lines that stayed alive there.
                layout = Layout::Conference;
                layout_plan = answered.remote.plan();
                match video_lines_from_answer(&current.peer, &answered) {
                    Some(lines) => {
                        if let Err(error) = local_video.set_lines(lines) {
                            timeline.record("video lines not attached", error.to_string());
                        }
                    }
                    None => {
                        local_video.clear_lines();
                        timeline.record(
                            "video lines not attached",
                            "no transceiver for the answered camera line",
                        );
                    }
                }
                negotiate_video_lines(
                    &mut video,
                    &signaling,
                    control,
                    timeline,
                    &answered.offer_sdp,
                    &answered.remote.lines,
                    true,
                    &media_links,
                    &roster,
                );
                tell_bound_camera(
                    &remote,
                    &media_links,
                    &local_video,
                    &mut media_description_request,
                    timeline,
                );
                tell_bound_share(
                    &remote,
                    &media_links,
                    &local_video,
                    &mut media_description_request,
                    timeline,
                );
            } else {
                local_video.clear_lines();
            }
            remote_set = true;
            if layout == Layout::OneToOne && negotiator.begin_conference_upgrade() {
                let pending = escalate(
                    &factory,
                    &grant,
                    options,
                    outgoing_track(software_audio.as_ref(), &track),
                    &remote,
                    &links,
                    &media_leg_id,
                    &media_links,
                    &mut negotiator,
                    live,
                    timeline,
                )
                .await;
                if pending.is_none() {
                    drop_local_media(&mut local_video, control);
                }
                next = pending.or(next);
            }
        }
        let next_peer_deadline = next.as_ref().and_then(|pending| pending.deadline);
        let glare_at = glare_resend.as_ref().map(|resend| resend.at);
        tokio::select! {
            Some(callback) = callbacks.recv() => {
                match handle_callback(&callback, timeline) {
                    Ok(CallEvent::Acceptance(acceptance)) => {
                        if !acceptance.from_mixer
                            && let Some(accepted_by) = &acceptance.accepted_by
                            && roster.apply(&direct_roster(&[(own_mri.as_str(), participant.display_name.as_str()), (accepted_by.mri.as_str(), accepted_by.display_name.as_str())]))
                        {
                            control.send(CallUpdate::Roster(roster.entries()));
                        }
                        if !acceptance.from_mixer && callee_gate.callee_joined() {
                            advance(state, CallSignal::MediaConnected, control);
                        }
                        let action = negotiator.on_acceptance(acceptance.from_mixer, layout == Layout::OneToOne);
                        match action {
                            MediaAction::ApplyAnswer | MediaAction::ApplyAnswerThenEscalate => {
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
                                if let Some(link) = acceptance.links.get("replacement")
                                    && sent_replacement.as_ref().is_none_or(|sent| sent.participant_id.is_none())
                                {
                                    let replacement = Replacement::of_call(link.clone());
                                    sent_replacement = Some(replacement.clone());
                                    control.send(CallUpdate::ReplacementLink(replacement));
                                }
                                if !callee_gate.is_waiting() {
                                    outgoing_deadline = None;
                                }
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
                                remember_call_leg(live, keep_alive_link.as_deref());
                                keep_alive_seconds = acceptance.keep_alive_seconds;
                                keep_alive_at = Some(Instant::now() + keep_alive_period(keep_alive_seconds));
                                if action == MediaAction::ApplyAnswerThenEscalate {
                                    let pending = escalate(&factory, &grant, options, outgoing_track(software_audio.as_ref(), &track), &remote, &links, &media_leg_id, &acceptance.links, &mut negotiator, live, timeline).await;
                                    next = pending.or(next);
                                }
                                let remote = remote.clone();
                                let links = links.clone();
                                let thread_id = echo_thread.clone();
                                let echo_add_tx = echo_add_tx.clone();
                                if is_echo {
                                    echo_add_pending = true;
                                }
                                tokio::spawn(async move {
                                    if is_echo {
                                        let result = remote
                                            .signaling
                                            .add_echo_bot(
                                                &remote.conversation,
                                                &remote.participant,
                                                thread_id.as_deref(),
                                                &links,
                                            )
                                            .await;
                                        let ok = result.is_ok();
                                        let _ = echo_add_tx.send(result);
                                        if !ok {
                                            return;
                                        }
                                    }
                                    let _ = remote
                                        .signaling
                                        .update_endpoint_metadata(
                                            &remote.conversation,
                                            &remote.participant,
                                        )
                                        .await;
                                });
                            }
                            MediaAction::Escalate => {
                                let pending = escalate(&factory, &grant, options, outgoing_track(software_audio.as_ref(), &track), &remote, &links, &media_leg_id, &acceptance.links, &mut negotiator, live, timeline).await;
                                next = pending.or(next);
                            }
                            _ => {}
                        }
                    }
                    Ok(CallEvent::MediaAnswer(answer)) if !remote_set && !answer.from_mixer && same_peer.is_none() && next.is_none() && outgoing_deadline.is_some() => {
                        timeline.record("provisional answer", if answer.no_ring_back { "no ringback" } else { "ringing" });
                        outgoing_deadline = Some(Instant::now() + OUTGOING_TIMEOUT);
                        if !answer.no_ring_back {
                            control.send(CallUpdate::Progress(Progress::Ringing));
                        }
                    }
                    Ok(CallEvent::MediaAnswer(answer))
                        if glare_resend.as_ref().is_some_and(|resend| !resend.reply_tags.is_empty() && answers_offer(&callback.path, &resend.reply_tags)) =>
                    {
                        if let Some(url) = answer.links.get("mediaAcknowledgement") {
                            let _ = remote.signaling.post_empty("POST mediaAcknowledgement", url).await;
                        }
                        if answer.from_mixer {
                            negotiator.note_mixer_path();
                        }
                        if let Some(resend) = glare_resend.as_mut() {
                            resend.at = Instant::now();
                        }
                    }
                    Ok(CallEvent::MediaAnswer(answer)) => {
                        let belongs = same_peer.as_ref().is_some_and(|pending| answers_offer(&callback.path, &pending.reply_tags))
                            || next.as_ref().is_some_and(|pending| answers_offer(&callback.path, &pending.reply_tags));
                        if (same_peer.is_some() || next.is_some()) && !belongs {
                            timeline.record("media answer ignored", "it answers an offer that is no longer pending");
                            if let Some(url) = answer.links.get("mediaAcknowledgement") {
                                let _ = remote.signaling.post_empty("POST mediaAcknowledgement", url).await;
                            }
                            continue;
                        }
                        let action = negotiator.on_media_answer();
                        if action == MediaAction::Ignore
                            && let Some(url) = answer.links.get("mediaAcknowledgement")
                        {
                            let _ = remote.signaling.post_empty("POST mediaAcknowledgement", url).await;
                        }
                        if action == MediaAction::ApplyOnSamePeer
                            && let Some(pending) = same_peer.take()
                        {
                            same_peer_deadline = None;
                            same_peer_grace_until = None;
                            let applied = match pending.change {
                                Change::Hold(_) => apply_hold_answer(&current.peer, &answer.sdp, &pending.signaled, &pending.browser_offer).await,
                                Change::Camera(_) | Change::Share(_) => apply_answer(&current.peer, &answer.sdp, &pending.signaled, &pending.browser_offer).await,
                            };
                            match applied {
                                Ok(()) => {
                                    if let Some(url) = answer.links.get("mediaAcknowledgement") {
                                        let _ = remote.signaling.post_empty("POST mediaAcknowledgement", url).await;
                                    }
                                    match pending.change {
                                        Change::Hold(hold) => {
                                            local_hold = hold;
                                            silence_for_hold(local_hold, muted, live.audio.as_ref(), software_audio.as_ref());
                                            control.send(CallUpdate::Hold(HoldState::from_flags(local_hold, remote_hold)));
                                        }
                                        Change::Camera(on) => control.send(CallUpdate::Camera(on)),
                                        Change::Share(true) if !answer.from_mixer && !local_video.share_sending() => {
                                            local_video.stop_share();
                                            sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), false, &mut software_audio, mic_settings, control, timeline);
                                            clear_share(control);
                                            control.send(CallUpdate::Notice(SHARE_REFUSED_NOTICE.into()));
                                        }
                                        Change::Share(_) => {}
                                    }
                                    if answer.from_mixer {
                                        media_links.extend(answer.links.clone());
                                        let wants_video = !local_hold && (local_video.camera_on() || local_video.sharing());
                                        let upgrade = wants_video
                                            && (negotiator.escalate_after_mixer_answer() || (layout == Layout::OneToOne && next.is_none() && negotiator.begin_conference_upgrade()));
                                        if !wants_video {
                                            negotiator.note_mixer_path();
                                        } else if upgrade {
                                            let pending = escalate(&factory, &grant, options, outgoing_track(software_audio.as_ref(), &track), &remote, &links, &media_leg_id, &media_links, &mut negotiator, live, timeline).await;
                                            if pending.is_none() {
                                                drop_local_media(&mut local_video, control);
                                            }
                                            next = pending.or(next);
                                        }
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
                                    if let Some(pending) = next.as_mut() {
                                        pending.leg_links.extend(answer.links.clone());
                                    }
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
                                    if let Some(pending) = next.take() {
                                        pending.session.peer.close();
                                    }
                                    if layout == Layout::OneToOne {
                                        drop_local_media(&mut local_video, control);
                                    }
                                    timeline.record("escalation answer rejected", error.to_string());
                                }
                            }
                        }
                    }
                    Ok(CallEvent::MediaRenegotiation(renegotiation)) => {
                        if !renegotiation.escalation
                            && !renegotiation.new_offer
                            && let Some(pending) = same_peer.take()
                        {
                            same_peer_deadline = None;
                            same_peer_grace_until = None;
                            negotiator.same_peer_offer_failed();
                            rollback_offer(&current.peer).await;
                            timeline.record("glare", "the remote offer is answered first, ours goes out again after");
                            glare_resend = Some(GlareResend { change: pending.change, reply_tags: pending.reply_tags, at: Instant::now() + GLARE_RESEND_LIMIT });
                        }
                        let action = negotiator.on_renegotiation(renegotiation.new_offer, renegotiation.escalation);
                        let rejection_link = renegotiation.links.get("rejection").cloned();
                        let result = match action {
                            MediaAction::AnswerOnNewPeer => {
                                if let Some(pending) = same_peer.take() {
                                    same_peer_deadline = None;
                                    same_peer_grace_until = None;
                                    timeline.record("same-peer offer", "superseded by the escalation offer");
                                    rollback_offer(&current.peer).await;
                                    defer_to_new_peer(pending.change, &mut deferred_changes, &mut local_video, control);
                                }
                                if let Some(resend) = glare_resend.take() {
                                    defer_to_new_peer(resend.change, &mut deferred_changes, &mut local_video, control);
                                }
                                let camera_track = local_video.camera_track();
                                let share_track = local_video.share_track();
                                match answer_on_new_peer(
                                    &factory,
                                    &grant,
                                    options,
                                    outgoing_track(software_audio.as_ref(), &track),
                                    &renegotiation.sdp,
                                    local_video.camera_on(),
                                    local_video.camera_on().then_some(&camera_track),
                                    local_video.sharing().then_some(&share_track),
                                )
                                .await
                                {
                                    Ok((session, answered)) => {
                                        timeline.record_sdp("local answer", &answered.answer_sdp);
                                        live.peers.push(session.peer.clone());
                                        let sent = send_media_answer(&remote, &links, &renegotiation.links, &answered, &media_leg_id).await;
                                        if let Some(replaced) = next.take() {
                                            replaced.session.peer.close();
                                        }
                                        let mut leg_links = renegotiation.links.clone();
                                        if let Ok((extra, keys)) = &sent {
                                            absorb_answered_links(&mut leg_links, extra, timeline, keys);
                                        }
                                        next = Some(NextPeer {
                                            session,
                                            offered: None,
                                            answered: Some(answered),
                                            leg_links,
                                            remote_audio: None,
                                            reply_tags: Vec::new(),
                                            deadline: Some(Instant::now() + NEXT_PEER_LIMIT),
                                            connected: false,
                                            retargeted: false,
                                        });
                                        sent.map(|_| ())
                                    }
                                    Err(error) => {
                                        if next.is_none() {
                                            negotiator.server_escalation_failed();
                                        }
                                        for change in deferred_changes.drain(..) {
                                            undo_change(change, &mut local_video, control);
                                        }
                                        if let Some(url) = &rejection_link {
                                            let _ = remote.signaling.reject_renegotiation(url, &remote.participant, links.call_id(), NegotiationFailure::AnswerError).await;
                                        }
                                        Err(error)
                                    }
                                }
                            }
                            MediaAction::AnswerOnSamePeer => {
                                // After a server escalation the follow-up offer matches the
                                // pending peer's m-line order: never answer it on the old peer.
                                let on_pending = next.as_ref().is_some_and(|pending| pending.answered.is_some());
                                let peer = if on_pending {
                                    &next.as_ref().unwrap().session.peer
                                } else {
                                    &current.peer
                                };
                                let camera_track = local_video.camera_track();
                                let share_track = local_video.share_track();
                                let video_answer = if local_video.camera_on() {
                                    AnswerVideo::Send(&camera_track)
                                } else {
                                    AnswerVideo::Receive
                                };
                                let keep_slots = on_pending || layout == Layout::Conference;
                                let gathered = (!on_pending
                                    && layout == Layout::OneToOne
                                    && negotiator.path() == MediaPath::Direct)
                                    .then_some(&current.gathered);
                                match answer_offer_with(
                                    peer,
                                    None,
                                    &renegotiation.sdp,
                                    video_answer,
                                    local_video.sharing().then(|| local_video.share_mid()).flatten(),
                                    local_video.sharing().then_some(&share_track),
                                    local_video.camera_on(),
                                    gathered,
                                    keep_slots,
                                    keep_slots,
                                )
                                .await
                                {
                                    Ok(answered) => {
                                        timeline.record_sdp("local answer", &answered.answer_sdp);
                                        if on_pending {
                                            if let Some(pending) = next.as_mut() {
                                                pending.leg_links.extend(renegotiation.links.clone());
                                            }
                                        } else {
                                            media_links.extend(renegotiation.links.clone());
                                        }
                                        let sent = send_media_answer(&remote, &links, &renegotiation.links, &answered, &media_leg_id).await;
                                        if let Ok((extra, keys)) = &sent {
                                            if on_pending {
                                                if let Some(pending) = next.as_mut() {
                                                    absorb_answered_links(&mut pending.leg_links, extra, timeline, keys);
                                                }
                                            } else {
                                                absorb_answered_links(&mut media_links, extra, timeline, keys);
                                            }
                                        }
                                        for mid in answered.remote.plan().screen_share_mids {
                                            if !layout_plan.screen_share_mids.contains(&mid) {
                                                layout_plan.screen_share_mids.push(mid);
                                            }
                                        }
                                        if on_pending {
                                            if let Some(pending) = next.as_mut() {
                                                pending.answered = Some(answered);
                                            }
                                        } else if layout == Layout::Conference {
                                            negotiate_video_lines(
                                                &mut video,
                                                &signaling,
                                                control,
                                                timeline,
                                                &answered.offer_sdp,
                                                &answered.remote.lines,
                                                true,
                                                &media_links,
                                                &roster,
                                            );
                                        } else if layout == Layout::OneToOne {
                                            if answered.receiving_share_mid != remote_direct_share {
                                                if let Some(mid) = &remote_direct_share {
                                                    video.router().set(mid, None);
                                                }
                                                if let Some(mid) = &answered.receiving_share_mid {
                                                    video.router().set(mid, Some(VideoKey::Screen));
                                                }
                                                remote_direct_share = answered.receiving_share_mid.clone();
                                                control.send(CallUpdate::ScreenShare(remote_direct_share.as_ref().and(peer_mri.clone())));
                                            }
                                            if !offer_sends_video(&renegotiation.sdp)
                                                && let Some(key) = main_video_mid(&answered.remote).and_then(|mid| video.router().key_for(&mid))
                                            {
                                                control.video.forget(&key);
                                                control.send(CallUpdate::VideoStopped(key));
                                            }
                                        }
                                        let held = is_hold(&renegotiation.sdp).unwrap_or(false);
                                        if held != remote_hold {
                                            remote_hold = held;
                                            control.send(CallUpdate::Hold(HoldState::from_flags(local_hold, remote_hold)));
                                        }
                                        sent.map(|_| ())
                                    }
                                    Err(error) => {
                                        if on_pending {
                                            if let Some(pending) = next.as_ref()
                                                && pending.session.peer.signaling_state()
                                                    == SignalingState::HaveRemoteOffer
                                            {
                                                rollback_remote_offer(&pending.session.peer).await;
                                            }
                                        } else if current.peer.signaling_state()
                                            == SignalingState::HaveRemoteOffer
                                        {
                                            rollback_remote_offer(&current.peer).await;
                                        }
                                        if let Some(url) = &rejection_link {
                                            let _ = remote.signaling.reject_renegotiation(url, &remote.participant, links.call_id(), NegotiationFailure::AnswerError).await;
                                        }
                                        Err(error)
                                    }
                                }
                            }
                            MediaAction::RejectGlare => {
                                if let Some(url) = &rejection_link {
                                    let _ = remote.signaling.reject_renegotiation(url, &remote.participant, links.call_id(), NegotiationFailure::InProgress).await;
                                }
                                Ok(())
                            }
                            _ => Ok(()),
                        };
                        if let Err(error) = result {
                            timeline.record("renegotiation failed", error.to_string());
                        }
                    }
                    Ok(CallEvent::RosterUpdate(body)) => {
                        if roster.apply(&body) {
                            let entries = roster.entries();
                            timeline.record("roster", format!("{} participant(s)", entries.len()));
                            control.send(CallUpdate::Roster(entries));
                            if is_echo && echo_watch.bot_left(roster.members().iter().map(|member| member.mri.as_str())) {
                                timeline.record("echo bot left", "test call ends like in Teams");
                                advance(state, CallSignal::RemoteEnd(EndKind::Normal), control);
                                break;
                            }
                            if callee_gate.is_waiting() && roster_has_callee(roster.members().iter().map(|member| member.mri.as_str()), &own_mri) {
                                outgoing_deadline = None;
                                if callee_gate.callee_joined() {
                                    advance(state, CallSignal::MediaConnected, control);
                                }
                            }
                            let own_streams = roster.stream_summaries(&own_mri);
                            if own_streams != report.own_streams {
                                timeline.record("own media streams", own_streams.join("; "));
                                report.own_streams = own_streams;
                            }
                            send_receive_change(control, video.reselect(&roster));
                            if let Some(replacement) = roster.transfer_replacement(&own_mri)
                                && sent_replacement.as_ref() != Some(&replacement)
                            {
                                sent_replacement = Some(replacement.clone());
                                control.send(CallUpdate::ReplacementLink(replacement));
                            }
                            if let Some(in_lobby) = roster.is_in_lobby(&own_mri)
                                && in_lobby != lobby
                            {
                                lobby = in_lobby;
                                control.send(CallUpdate::Lobby(lobby));
                            }
                            if roster.caption_bot().is_some() {
                                bot_pending = false;
                            }
                            let audio_only = roster.caption_bot().is_some_and(|bot| bot.recording && bot.audio_only);
                            if audio_only != recording_audio_only {
                                recording_audio_only = audio_only;
                                control.send(CallUpdate::RecordingAudioOnly(audio_only));
                            }
                            if caption_flow.wanted() {
                                run_bot_steps(BotJob::Captions, &mut caption_flow, &roster, &remote, &links, meeting_chat.as_deref(), &RecordingPlan::default(), &mut bot_pending, control, timeline).await;
                                if caption_flow.on() {
                                    caption_deadline = None;
                                }
                            }
                            if recording_flow.wanted() {
                                run_bot_steps(BotJob::Recording, &mut recording_flow, &roster, &remote, &links, meeting_chat.as_deref(), &recording_plan, &mut bot_pending, control, timeline).await;
                            }
                        }
                    }
                    Ok(CallEvent::AddParticipantFailure(body)) => {
                        let bot_add = bot_pending;
                        bot_pending = false;
                        if !bot_add && callee_gate.callees_failed(failed_participant_count(&body)) {
                            let end = embedded_end(&body);
                            let kind = end.as_ref().map_or(EndKind::Unavailable, setup_end_kind);
                            report.end = end;
                            advance(state, CallSignal::RemoteEnd(kind), control);
                            break;
                        }
                        if recording_flow.wanted() && roster.caption_bot().is_none() {
                            timeline.record("recorder bot failed", "addParticipantFailure");
                            recording_flow.failed();
                            recording_deadline = None;
                            control.send(CallUpdate::RecordingPending(false));
                            control.send(CallUpdate::Notice(RECORDING_JOIN_FAILED_NOTICE.into()));
                        }
                        if caption_flow.wanted() && roster.caption_bot().is_none() {
                            timeline.record("caption bot failed", "addParticipantFailure");
                            caption_flow.failed();
                            caption_deadline = None;
                            control.send(CallUpdate::Captions(CaptionState::Failed(
                                CAPTIONS_JOIN_FAILED_NOTICE.into(),
                            )));
                        }
                    }
                    Ok(CallEvent::ConversationUpdate(body)) => {
                        if let Some(thread) = chat_thread_id(&body)
                            && meeting_chat.as_ref() != Some(&thread)
                        {
                            control.send(CallUpdate::MeetingChat(thread.clone()));
                            meeting_chat = Some(thread);
                        }
                        if capability.apply(&body) {
                            control.send(CallUpdate::Capability(capability));
                        }
                        let change = recording_status.apply(&body);
                        if let Some(on) = change.recording {
                            control.send(CallUpdate::Recording(on));
                            if on {
                                recording_deadline = None;
                                control.send(CallUpdate::RecordingPending(false));
                            }
                        }
                        if let Some(on) = change.transcription {
                            control.send(CallUpdate::Transcribing(on));
                            if on {
                                recording_deadline = None;
                                control.send(CallUpdate::RecordingPending(false));
                            }
                        }
                        if let Some(required) = change.consent_required {
                            consent_pending = required;
                            control.send(CallUpdate::ConsentRequired(required));
                            if required {
                                withhold_media(&mut muted, live.audio.as_ref().expect("audio set above"), software_audio.as_ref(), &remote, &mut local_video, control);
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
                        let moved = room_watcher.apply(breakout::room_assignment(&body)).filter(|moved| !is_own_meeting(meeting_target.as_ref(), &moved.target));
                        if let Some(moved) = moved {
                            timeline.record("breakout", format!("moved to {}", moved.room_name));
                            control.send(CallUpdate::BreakoutMove(moved));
                        }
                    }
                    Ok(CallEvent::Replacement(body)) => {
                        let invited = breakout::invite_move(&body).filter(|moved| !is_own_meeting(meeting_target.as_ref(), &moved.target));
                        if let Some(moved) = invited {
                            timeline.record("breakout", format!("invited to {}", moved.room_name));
                            control.send(CallUpdate::BreakoutMove(moved));
                        }
                    }
                    Ok(CallEvent::ContentShareUpdate(body)) => {
                        if let Some(share) = whiteboard::content_share(&body).filter(|share| share.whiteboard) {
                            control.send(CallUpdate::Whiteboard(Some(share)));
                        }
                    }
                    Ok(CallEvent::ContentShareEnd(_)) => {
                        control.send(CallUpdate::Whiteboard(None));
                    }
                    Ok(CallEvent::Transfer(event)) => {
                        match event {
                            TransferEvent::Accepted => timeline.record("transfer accepted", "by the target"),
                            completed if completed.succeeded() => {
                                timeline.record("transfer", "completed");
                                control.send(CallUpdate::TransferResult { succeeded: true });
                                control.send(CallUpdate::Notice(CALL_TRANSFERRED_NOTICE.into()));
                                advance(state, CallSignal::LocalLeave, control);
                                break;
                            }
                            TransferEvent::Completed { code, sub_code } => {
                                timeline.record("transfer failed", format!("code {code} subCode {sub_code}"));
                                control.send(CallUpdate::TransferResult { succeeded: false });
                                control.send(CallUpdate::Notice(TRANSFER_FAILED_NOTICE.into()));
                            }
                        }
                    }
                    Ok(CallEvent::Reactions(events)) => {
                        for event in events {
                            control.send(CallUpdate::Reaction { mri: event.mri, reaction: event.reaction });
                        }
                    }
                    Ok(CallEvent::Progress(status)) => {
                        if status == ProgressStatus::Ringing {
                            control.send(CallUpdate::Progress(Progress::Ringing));
                            if outgoing_deadline.is_some() {
                                outgoing_deadline = Some(Instant::now() + OUTGOING_TIMEOUT);
                            }
                        }
                    }
                    Ok(CallEvent::ControlVideoStreaming(body)) => {
                        let Some(request) = parse_request(&body) else { continue };
                        let Some(limits) = send_requests.admit(&request) else { continue };
                        let own_screen = roster.screen_source_of(&own_mri).map(i64::from);
                        let source = if request.source_id.is_some() && request.source_id == own_screen { SendSource::Screen } else { SendSource::Camera };
                        match local_video.apply_send_limit(source, &limits) {
                            Ok(Some(plans)) => timeline.record("send limits", format!("{source:?} {plans:?}")),
                            Ok(None) => {}
                            Err(error) => timeline.record("send limits failed", error.to_string()),
                        }
                    }
                    Ok(CallEvent::Speakers(sources)) => {
                        let speakers = roster.mris_for_sources(&sources);
                        if video.note_speakers(&speakers) {
                            send_receive_change(control, video.reselect(&roster));
                        }
                        control.send(CallUpdate::Speakers(speakers));
                    }
                    Ok(CallEvent::ConversationEnd(body)) if callee_gate.is_waiting() => {
                        live.skip_leave = true;
                        let end = embedded_end(&body);
                        let kind = end.as_ref().map_or(EndKind::Unavailable, setup_end_kind);
                        report.end = end;
                        advance(state, CallSignal::RemoteEnd(kind), control);
                        break;
                    }
                    Ok(CallEvent::Rejection(_))
                        if glare_resend.as_ref().is_some_and(|resend| !resend.reply_tags.is_empty() && answers_offer(&callback.path, &resend.reply_tags)) =>
                    {
                        if let Some(resend) = glare_resend.as_mut() {
                            resend.at = Instant::now();
                        }
                    }
                    Ok(CallEvent::Rejection(_))
                        if !same_peer.as_ref().is_some_and(|pending| answers_offer(&callback.path, &pending.reply_tags))
                            && !next.as_ref().is_some_and(|pending| answers_offer(&callback.path, &pending.reply_tags)) =>
                    {
                        timeline.record("rejection ignored", "it names an offer that is no longer pending");
                    }
                    Ok(CallEvent::Rejection(body)) => match negotiator.on_rejection() {
                        RejectedOffer::SamePeer => {
                            same_peer_deadline = None;
                            same_peer_grace_until = None;
                            if let Some(pending) = same_peer.take() {
                                rollback_offer(&current.peer).await;
                                if is_glare_rejection(&body) {
                                    timeline.record("glare", "our offer met theirs, it goes out again");
                                    glare_resend = Some(GlareResend { change: pending.change, reply_tags: Vec::new(), at: Instant::now() + GLARE_RESEND_LIMIT });
                                } else {
                                    undo_change(pending.change, &mut local_video, control);
                                }
                            }
                        }
                        RejectedOffer::NewPeer => {
                            if let Some(pending) = next.take() {
                                pending.session.peer.close();
                            }
                            if layout == Layout::OneToOne {
                                drop_local_media(&mut local_video, control);
                            }
                            timeline.record("escalation failed", "offer rejected, staying on the old peer connection");
                        }
                        RejectedOffer::None => {}
                    },
                    Ok(CallEvent::MediaAcknowledgement(ack)) => {
                        if let Some(pending) = next.as_mut() {
                            pending.leg_links.extend(ack.links);
                            if pending.answered.is_some() {
                                // Server-initiated mixer leg switch sends mediaAcknowledgement
                                // instead of retargetCompletion; switch as soon as ICE connects.
                                pending.retargeted = true;
                            }
                        } else if !ack.links.is_empty() {
                            let old_request = media_links
                                .get("applyChannelParameters")
                                .or_else(|| media_links.get("controlVideoStreaming"))
                                .cloned();
                            let old_update = media_links.get("updateMediaDescriptions").cloned();
                            replace_leg_media_links(&mut media_links, &ack.links);
                            if let Some(url) = media_links.get("callLeg").cloned()
                                && keep_alive_link.as_deref() != Some(url.as_str())
                            {
                                keep_alive_link = Some(url.clone());
                                remember_call_leg(live, Some(&url));
                            }
                            let new_request = media_links
                                .get("applyChannelParameters")
                                .or_else(|| media_links.get("controlVideoStreaming"))
                                .cloned();
                            if layout == Layout::Conference
                                && new_request != old_request
                                && let Some(url) = new_request
                            {
                                video.replace_requester(SourceRequester::start(
                                    signaling.clone(),
                                    url,
                                    video.router().clone(),
                                    control.video.clone(),
                                    timeline.clone(),
                                ));
                                send_receive_change(control, video.reselect(&roster));
                            }
                            if layout == Layout::Conference
                                && media_links.get("updateMediaDescriptions") != old_update.as_ref()
                                && local_video.has_lines()
                            {
                                send_capabilities = SendCapabilityReporter::default();
                                if local_video.camera_on() {
                                    tell_bound_camera(
                                        &remote,
                                        &media_links,
                                        &local_video,
                                        &mut media_description_request,
                                        timeline,
                                    );
                                }
                                if local_video.sharing() {
                                    tell_bound_share(
                                        &remote,
                                        &media_links,
                                        &local_video,
                                        &mut media_description_request,
                                        timeline,
                                    );
                                }
                            }
                        }
                    }
                    Ok(CallEvent::RetargetCompletion(body)) if body["retargetCompleted"]["code"].as_i64() == Some(0) => {
                        if let Some(pending) = next.as_mut() {
                            pending.retargeted = true;
                        }
                    }
                    Ok(CallEvent::RetargetCompletion(body)) => {
                        if body["retargetCompleted"]["code"].as_i64().is_some() && let Some(pending) = next.take() {
                            pending.session.peer.close();
                            negotiator.escalation_failed();
                            settle_failed_escalation(pending.offered.is_none(), layout, &mut negotiator, &mut deferred_changes, &mut local_video, control);
                            timeline.record("escalation failed", "retarget rejected, staying on the old peer connection");
                        }
                    }
                    Ok(CallEvent::End(end)) => {
                        let kind = end.kind();
                        report.end = Some(end);
                        advance(state, CallSignal::RemoteEnd(kind), control);
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        timeline.record("callback not understood", error.to_string());
                    }
                }
            }
            Some(result) = echo_add_rx.recv(), if echo_add_pending => {
                echo_add_pending = false;
                if let Err(error) = result {
                    timeline.record("echo bot add failed", error.to_string());
                    control.send(CallUpdate::Notice(ECHO_TEST_FAILED_NOTICE.into()));
                    live.cancel_leave = true;
                    advance(state, CallSignal::RemoteEnd(EndKind::Unavailable), control);
                    break;
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
                            if callee_gate.media_connected() {
                                advance(state, CallSignal::MediaConnected, control);
                            }
                            if report.peer_connected_ms.is_none() {
                                report.peer_connected_ms = Some(timeline.elapsed_ms());
                                if let Some(hold) = options.hold_after_connected {
                                    let hold_until = Instant::now() + hold;
                                    leave_at = Some(hard_deadline.map_or(hold_until, |deadline| deadline.min(hold_until)));
                                }
                            }
                        }
                        // Planned mixer escalation: ignore old-peer death while the new peer is pending.
                        PeerConnectionState::Disconnected | PeerConnectionState::Failed
                            if next.is_some() => {}
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
                    let had_far_end = remote_audio.replace(remote_track.clone()).is_some();
                    if !had_far_end && software_audio.is_some() {
                        sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), true, &mut software_audio, mic_settings, control, timeline);
                    }
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
                    if let Some(pending) = next.as_mut() {
                        pending.connected = true;
                        if pending.offered.is_none() && !pending.retargeted {
                            pending.deadline = Some(Instant::now() + RETARGET_WAIT);
                            timeline.record("escalation", "new peer connection up, waiting for retargetCompletion");
                        }
                    }
                }
                PeerEvent::VideoTrack { mid, track } => {
                    timeline.record("remote video track", format!("mid {mid} (new peer)"));
                    live.video_tasks.push(spawn_video_pump(track, mid, video.router().clone(), control.video.clone(), live.video_counters.clone()));
                }
                PeerEvent::Track(remote_track) => {
                    timeline.record("remote audio track", "receiving (new peer)");
                    if let Some(pending) = next.as_mut() {
                        pending.remote_audio = Some(remote_track);
                    }
                }
                PeerEvent::Connection(PeerConnectionState::Failed) => {
                    negotiator.escalation_failed();
                    if let Some(pending) = next.take() {
                        pending.session.peer.close();
                        settle_failed_escalation(pending.offered.is_none(), layout, &mut negotiator, &mut deferred_changes, &mut local_video, control);
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
                        muted = apply_mute(mute, muted, live.audio.as_ref().expect("audio set above"), software_audio.as_ref(), &remote, control);
                        silence_for_hold(local_hold, muted, live.audio.as_ref(), software_audio.as_ref());
                    }
                }
                Some(CallCommand::DisableAudio) => {
                    let audio = live.audio.as_ref().expect("audio set above");
                    muted = apply_mute(MuteCommand::Mute, muted, audio, software_audio.as_ref(), &remote, control);
                    audio_disabled = true;
                    sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, None, false, &mut software_audio, mic_settings, control, timeline);
                    if audio.uses_devices() {
                        use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
                        factory.set_adm_recording_enabled(false);
                        factory.set_adm_playout_enabled(false);
                    }
                    control.send(CallUpdate::ListenOnly(true));
                }
                Some(CallCommand::SelectInput(choice)) => {
                    if live.audio.as_ref().is_some_and(AudioSetup::uses_devices) {
                        timeline.record("input device", format!("{choice:?} applied={}", devices::select_input(&factory, &choice)));
                        applied_input = devices::applied_device(&device_lists.inputs, &choice, devices::system_default_id(Flow::Capture).as_deref());
                        record_audio_devices(timeline, &device_lists, applied_input.as_ref(), applied_output.as_ref());
                    }
                    input = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                    if software_audio.is_some() {
                        sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), true, &mut software_audio, mic_settings, control, timeline);
                    }
                }
                Some(CallCommand::SetMicSettings(settings)) if !audio_disabled => {
                    let device_ns_changed =
                        mic_settings.ml_noise_suppression() != settings.ml_noise_suppression();
                    mic_settings = settings;
                    if let Some(audio) = live.audio.as_ref() {
                        track = audio.device_track(mic_settings).clone();
                    }
                    sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), device_ns_changed, &mut software_audio, mic_settings, control, timeline);
                }
                Some(CallCommand::SetMicSettings(settings)) => {
                    mic_settings = settings;
                    crate::noise_stage::apply_for_outgoing(mic_settings, false);
                }
                Some(CallCommand::SelectOutput(choice)) => {
                    if live.audio.as_ref().is_some_and(AudioSetup::uses_devices) {
                        timeline.record("output device", format!("{choice:?} applied={}", devices::select_output(&factory, &choice)));
                        applied_output = devices::applied_device(&device_lists.outputs, &choice, devices::system_default_id(Flow::Render).as_deref());
                        record_audio_devices(timeline, &device_lists, applied_input.as_ref(), applied_output.as_ref());
                    }
                    output = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                }
                Some(CallCommand::RefreshDevices) => {
                    device_lists = devices::list_devices(&factory);
                    control.send(CallUpdate::Devices(device_lists.clone()));
                }
                Some(CallCommand::SetCamera(false)) if local_video.camera_previewing() => {
                    start_camera_pending = false;
                    local_video.stop_camera();
                    control.send(CallUpdate::Camera(false));
                }
                Some(CallCommand::SetCamera(on)) => {
                    let direct = layout == Layout::OneToOne && negotiator.path() != MediaPath::Mixer;
                    if on && next.is_some() {
                        let first = !start_camera_pending;
                        start_camera_pending = true;
                        if local_video.is_available()
                            && !local_video.camera_on()
                            && !local_video.camera_previewing()
                            && let Err(error) = local_video.preview_camera(&camera_choice)
                        {
                            control.send(CallUpdate::Notice(notice_with_detail(CAMERA_START_NOTICE, error)));
                        }
                        if first {
                            control.send(CallUpdate::Camera(true));
                        }
                    } else if !remote_set || (on && !video_possible(remote_set, layout, negotiator.path())) {
                        if remote_set && next.is_none() && negotiator.begin_conference_upgrade() {
                            let pending = escalate(&factory, &grant, options, outgoing_track(software_audio.as_ref(), &track), &remote, &links, &media_leg_id, &media_links, &mut negotiator, live, timeline).await;
                            next = pending.or(next);
                        }
                        if remote_set && on && next.is_some() {
                            start_camera_pending = true;
                        }
                        control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                    } else if on && consent_pending {
                        control.send(CallUpdate::Notice(CONSENT_FIRST_NOTICE.into()));
                    } else if local_hold {
                        control.send(CallUpdate::Notice(ON_HOLD_NOTICE.into()));
                    } else if on && direct && (same_peer.is_some() || glare_resend.is_some()) {
                        control.send(CallUpdate::Notice(CHANGE_RUNNING_NOTICE.into()));
                    } else if on {
                        let started = start_camera_line(&mut local_video, &current.peer, direct.then_some(direct_video_mid.as_deref()).flatten(), &camera_choice);
                        match started {
                            Ok(()) if direct => {
                                match offer_same_peer(&current.peer, &current.gathered, local_video.share_mid(), &layout_plan, layout, direct_video_mid.as_deref().unwrap_or(CAMERA_MID), false, true, local_video.sharing(), &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                                    Ok((browser_offer, signaled, reply_tags)) => {
                                        timeline.record_sdp("local offer", &signaled.sdp);
                                        same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Camera(true), reply_tags });
                                        same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                                        same_peer_grace_until = None;
                                    }
                                    Err(error) => {
                                        timeline.record("camera offer failed", error.to_string());
                                        undo_change(Change::Camera(true), &mut local_video, control);
                                    }
                                }
                            }
                            Ok(()) => {
                                control.send(CallUpdate::Camera(true));
                                tell_media_descriptions(
                                    &remote,
                                    &media_links,
                                    MediaDescription::camera_line(
                                        local_video.camera_mid().unwrap_or_else(|| CAMERA_MID.to_owned()),
                                        true,
                                    ),
                                    &mut media_description_request,
                                    timeline,
                                );
                            }
                            Err(error) => control.send(CallUpdate::Notice(notice_with_detail(CAMERA_START_NOTICE, error))),
                        }
                    } else if direct && (same_peer.is_some() || glare_resend.is_some()) {
                        control.send(CallUpdate::Notice(CHANGE_RUNNING_NOTICE.into()));
                    } else if direct {
                        local_video.stop_camera();
                        match offer_same_peer(&current.peer, &current.gathered, local_video.share_mid(), &layout_plan, layout, direct_video_mid.as_deref().unwrap_or(CAMERA_MID), false, false, local_video.sharing(), &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                            Ok((browser_offer, signaled, reply_tags)) => {
                                timeline.record_sdp("local offer", &signaled.sdp);
                                same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Camera(false), reply_tags });
                                same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                                same_peer_grace_until = None;
                            }
                            Err(error) => {
                                timeline.record("camera offer failed", error.to_string());
                                control.send(CallUpdate::Camera(false));
                            }
                        }
                    } else {
                        local_video.stop_camera();
                        control.send(CallUpdate::Camera(false));
                        tell_media_descriptions(
                            &remote,
                            &media_links,
                            MediaDescription::camera_line(
                                local_video.camera_mid().unwrap_or_else(|| CAMERA_MID.to_owned()),
                                false,
                            ),
                            &mut media_description_request,
                            timeline,
                        );
                    }
                }
                Some(CallCommand::SelectCamera(choice)) => {
                    camera_choice = choice;
                    if local_video.camera_previewing()
                        && let Err(error) = local_video.preview_camera(&camera_choice)
                    {
                        control.send(CallUpdate::Notice(notice_with_detail(CAMERA_START_NOTICE, error)));
                    } else if local_video.camera_on()
                        && let Err(error) = local_video.start_camera(&camera_choice)
                    {
                        control.send(CallUpdate::Notice(notice_with_detail(CAMERA_START_NOTICE, error)));
                    }
                }
                Some(CallCommand::SetVideoQuality(quality)) => {
                    local_video.set_video_quality(quality);
                    if let Err(error) = local_video.refresh_camera_capture(&camera_choice) {
                        control.send(CallUpdate::Notice(notice_with_detail(CAMERA_RESTART_NOTICE, error)));
                    }
                    if video.set_video_quality(quality) {
                        send_receive_change(control, video.reselect(&roster));
                    }
                }
                Some(CallCommand::StartPortalShare(kind)) => {
                    let direct = layout == Layout::OneToOne && negotiator.path() == MediaPath::Direct;
                    if !remote_set {
                        control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                    } else if local_hold {
                        control.send(CallUpdate::Notice(ON_HOLD_NOTICE.into()));
                    } else if !local_video.sharing() && next.is_some() {
                        portal_pending = Some(kind);
                        control.send(CallUpdate::Notice(SHARE_QUEUED_NOTICE.into()));
                    } else if direct && !local_video.sharing() && (same_peer.is_some() || glare_resend.is_some()) {
                        control.send(CallUpdate::Notice(CHANGE_RUNNING_NOTICE.into()));
                    } else if direct
                        && !local_video.sharing()
                        && !local_video.has_share_line()
                        && let Err(error) = attach_share_line(&current.peer, &factory, &mut local_video, direct_video_mid.as_deref())
                    {
                        control.send(CallUpdate::Notice(notice_with_detail(SHARING_START_NOTICE, error)));
                    } else if !local_video.sharing()
                        && (!video_possible(remote_set, layout, negotiator.path())
                            || !local_video.share_sending())
                    {
                        if next.is_none() && negotiator.begin_conference_upgrade() {
                            let pending = escalate(&factory, &grant, options, outgoing_track(software_audio.as_ref(), &track), &remote, &links, &media_leg_id, &media_links, &mut negotiator, live, timeline).await;
                            next = pending.or(next);
                        }
                        if next.is_some() {
                            portal_pending = Some(kind);
                            control.send(CallUpdate::Notice(SHARE_QUEUED_NOTICE.into()));
                        } else {
                            control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                        }
                    } else if let Err(error) = local_video.open_portal(kind, control.command_sender()) {
                        control.send(CallUpdate::Notice(notice_with_detail(SHARING_START_NOTICE, error)));
                    }
                }
                Some(CallCommand::FinishPortalShare { source, generation }) => {
                    if local_video.accept_portal(generation) {
                        control.defer(CallCommand::StartShare(source));
                    }
                }
                Some(CallCommand::AbandonPortalShare) => {
                    if local_video.portal_is_open() {
                        local_video.stop_share();
                    }
                }
                Some(CallCommand::StartShare(source)) => {
                    let direct = layout == Layout::OneToOne && negotiator.path() == MediaPath::Direct;
                    if !remote_set {
                        control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                    } else if local_hold {
                        control.send(CallUpdate::Notice(ON_HOLD_NOTICE.into()));
                    } else if next.is_some() {
                        share_pending = Some(source);
                        control.send(CallUpdate::Notice(SHARE_QUEUED_NOTICE.into()));
                    } else if direct && local_video.share_sending() {
                        match local_video.start_share(&source) {
                            Ok(()) => announce_share(control, &source),
                            Err(error) => control.send(CallUpdate::Notice(notice_with_detail(SHARING_START_NOTICE, error))),
                        }
                    } else if direct && (same_peer.is_some() || glare_resend.is_some()) {
                        control.send(CallUpdate::Notice(CHANGE_RUNNING_NOTICE.into()));
                    } else if direct
                        && !local_video.has_share_line()
                        && let Err(error) = attach_share_line(&current.peer, &factory, &mut local_video, direct_video_mid.as_deref())
                    {
                        control.send(CallUpdate::Notice(notice_with_detail(SHARING_START_NOTICE, error)));
                    } else if direct {
                        match local_video.start_share(&source) {
                            Ok(()) => {
                                announce_share(control, &source);
                                sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), false, &mut software_audio, mic_settings, control, timeline);
                                match offer_same_peer(&current.peer, &current.gathered, local_video.share_mid(), &layout_plan, layout, direct_video_mid.as_deref().unwrap_or(CAMERA_MID), false, local_video.camera_on(), true, &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                                    Ok((browser_offer, signaled, reply_tags)) => {
                                        timeline.record_sdp("local offer", &signaled.sdp);
                                        same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Share(true), reply_tags });
                                        same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                                        same_peer_grace_until = None;
                                    }
                                    Err(error) => {
                                        timeline.record("share offer failed", error.to_string());
                                        undo_change(Change::Share(true), &mut local_video, control);
                                    }
                                }
                            }
                            Err(error) => control.send(CallUpdate::Notice(notice_with_detail(SHARING_START_NOTICE, error))),
                        }
                    } else if !video_possible(remote_set, layout, negotiator.path())
                        || !local_video.share_sending()
                    {
                        if next.is_none() && negotiator.begin_conference_upgrade() {
                            let pending = escalate(&factory, &grant, options, outgoing_track(software_audio.as_ref(), &track), &remote, &links, &media_leg_id, &media_links, &mut negotiator, live, timeline).await;
                            next = pending.or(next);
                        }
                        if next.is_some() {
                            share_pending = Some(source);
                            control.send(CallUpdate::Notice(SHARE_QUEUED_NOTICE.into()));
                        } else {
                            control.send(CallUpdate::Notice(NOT_CONNECTED_FOR_VIDEO.into()));
                        }
                    } else {
                        match local_video.start_share(&source) {
                            Ok(()) => {
                                announce_share(control, &source);
                                tell_media_descriptions(
                                    &remote,
                                    &media_links,
                                    MediaDescription::share_line(
                                        local_video.share_mid().unwrap_or_else(|| SHARE_MID.to_owned()),
                                        true,
                                    ),
                                    &mut media_description_request,
                                    timeline,
                                );
                                sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), false, &mut software_audio, mic_settings, control, timeline);
                            }
                            Err(error) => control.send(CallUpdate::Notice(notice_with_detail(SHARING_START_NOTICE, error))),
                        }
                    }
                }
                Some(CallCommand::StopShare) => {
                    let direct = layout == Layout::OneToOne && negotiator.path() == MediaPath::Direct;
                    share_pending = None;
                    portal_pending = None;
                    if local_video.portal_is_open() {
                        local_video.stop_share();
                    } else if direct && local_video.sharing() && (same_peer.is_some() || glare_resend.is_some()) {
                        control.send(CallUpdate::Notice(CHANGE_RUNNING_NOTICE.into()));
                    } else if direct && local_video.sharing() {
                        local_video.stop_share();
                        clear_share(control);
                        sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), false, &mut software_audio, mic_settings, control, timeline);
                        match offer_same_peer(&current.peer, &current.gathered, local_video.share_mid(), &layout_plan, layout, direct_video_mid.as_deref().unwrap_or(CAMERA_MID), local_hold, local_video.camera_on(), false, &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                            Ok((browser_offer, signaled, reply_tags)) => {
                                timeline.record_sdp("local offer", &signaled.sdp);
                                same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Share(false), reply_tags });
                                same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                                same_peer_grace_until = None;
                            }
                            Err(error) => timeline.record("share offer failed", error.to_string()),
                        }
                    } else if local_video.sharing() {
                        local_video.stop_share();
                        sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), false, &mut software_audio, mic_settings, control, timeline);
                        clear_share(control);
                        tell_media_descriptions(
                            &remote,
                            &media_links,
                            MediaDescription::share_line(
                                local_video.share_mid().unwrap_or_else(|| SHARE_MID.to_owned()),
                                false,
                            ),
                            &mut media_description_request,
                            timeline,
                        );
                    }
                }
                Some(CallCommand::SetShareSound(on)) => {
                    share_sound_wanted = on;
                    sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), false, &mut software_audio, mic_settings, control, timeline);
                }
                Some(CallCommand::RefreshShareSources) => refresh_share_sources(control.updates(), options.video, true),
                Some(CallCommand::SetHand(true)) => match remote.signaling.raise_hand(&remote.conversation, &remote.participant).await {
                    Ok(receipt) => {
                        own_hand = receipt.state_id;
                        control.send(CallUpdate::OwnHand(Some(receipt.rank)));
                    }
                    Err(error) => {
                        timeline.record("raise hand failed", error.to_string());
                        control.send(CallUpdate::OwnHand(None));
                        control.send(CallUpdate::Notice(RAISE_HAND_FAILED_NOTICE.into()));
                    }
                },
                Some(CallCommand::SetHand(false)) => {
                    let state_ids: Vec<String> = roster.hand_of(&own_mri).map(|hand| hand.state_id.clone()).or(own_hand.take()).into_iter().collect();
                    own_hand = None;
                    control.send(CallUpdate::OwnHand(None));
                    if !state_ids.is_empty()
                        && let Err(error) = remote.signaling.lower_hands(&remote.conversation, &remote.participant, &state_ids).await
                    {
                        timeline.record("lower hand failed", error.to_string());
                        control.send(CallUpdate::Notice(LOWER_HAND_FAILED_NOTICE.into()));
                    }
                }
                Some(CallCommand::LowerHand { mri }) => {
                    let state_ids: Vec<String> = roster.hand_of(&mri).map(|hand| hand.state_id.clone()).into_iter().collect();
                    if !state_ids.is_empty()
                        && let Err(error) = remote.signaling.lower_hands(&remote.conversation, &remote.participant, &state_ids).await
                    {
                        timeline.record("lower hand failed", error.to_string());
                        control.send(CallUpdate::Notice(LOWER_THAT_HAND_FAILED_NOTICE.into()));
                    }
                }
                Some(CallCommand::LowerAllHands) => {
                    if let Err(error) = remote.signaling.lower_all_hands(&remote.conversation, &remote.participant).await {
                        timeline.record("lower all hands failed", error.to_string());
                        control.send(CallUpdate::Notice(LOWER_ALL_HANDS_FAILED_NOTICE.into()));
                    }
                }
                Some(CallCommand::Admit { mri }) => {
                    let result = remote.signaling.admit(&remote.conversation, &remote.participant, target_of(&roster, &mri), &links).await;
                    report_organizer_result(result, "admit", ADMIT_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::AdmitAll) => {
                    let mut result = Ok(202);
                    for mri in roster.lobby_mris() {
                        result = remote.signaling.admit(&remote.conversation, &remote.participant, target_of(&roster, &mri), &links).await;
                        if result.is_err() {
                            break;
                        }
                    }
                    report_organizer_result(result, "admit all", ADMIT_ALL_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::Deny { mri }) => {
                    let result = remote.signaling.remove_participant(&remote.conversation, &remote.participant, target_of(&roster, &mri), &links).await;
                    report_organizer_result(result, "deny", DENY_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::RemoveParticipant { mri }) => {
                    let result = remote.signaling.remove_participant(&remote.conversation, &remote.participant, target_of(&roster, &mri), &links).await;
                    report_organizer_result(result, "remove participant", REMOVE_PARTICIPANT_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::MuteParticipant { mri }) => {
                    let result = remote.signaling.mute_participant(&remote.conversation, &remote.participant, &mri).await;
                    report_organizer_result(result, "mute participant", MUTE_PARTICIPANT_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::MuteAll) => {
                    let others: Vec<String> = roster
                        .members()
                        .iter()
                        .filter(|member| member.mri != own_mri && !member.in_lobby && !member.mri.starts_with(BOT_PREFIX))
                        .map(|member| member.mri.clone())
                        .collect();
                    let result = remote.signaling.mute_everyone(&remote.conversation, &remote.participant, &others).await;
                    report_organizer_result(result, "mute all", MUTE_ALL_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::SetLock(locked)) => {
                    let result = remote.signaling.set_lock(&remote.conversation, &remote.participant, locked).await;
                    if result.is_ok() {
                        capability.locked = locked;
                        control.send(CallUpdate::Capability(capability));
                    }
                    report_organizer_result(result, "lock meeting", MEETING_LOCK_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::SetRole { mri, role }) => {
                    let result = remote.signaling.change_role(&remote.conversation, &remote.participant, &mri, role).await;
                    report_organizer_result(result, "change role", CHANGE_ROLE_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::SetAttendeeModality { mri, modality, allowed }) => {
                    let limits = roster.entries().into_iter().find(|entry| entry.mri == mri).map(|entry| entry.limits).unwrap_or_default();
                    let mut result = Ok(200);
                    for step in modality_steps(modality, allowed, capability, &limits) {
                        result = match step {
                            StateStep::Publish(state_type) => remote.signaling.publish_attendee_state(&remote.conversation, &remote.participant, state_type, &mri).await,
                            StateStep::Remove(state_id) => remote.signaling.lower_hands(&remote.conversation, &remote.participant, &[state_id]).await.map(|()| 200),
                        };
                        if result.is_err() {
                            break;
                        }
                    }
                    let notice = if modality == Modality::Mic {
                        MIC_SETTING_FAILED_NOTICE
                    } else {
                        CAMERA_SETTING_FAILED_NOTICE
                    };
                    report_organizer_result(result, "attendee modality", notice, control, timeline);
                }
                Some(CallCommand::SetAttendeeRestrictions { mic_off, camera_off }) => {
                    let result = match &meeting_target {
                        Some(target) => meeting_options::write(&inner.session, target, &OptionWrite::restrictions(mic_off, camera_off)).await.map(|()| 200),
                        None => Err(Error::Signaling("no meeting coordinates for the options".into())),
                    };
                    if result.is_ok() {
                        capability.attendee_mic_off = mic_off;
                        capability.attendee_camera_off = camera_off;
                        control.send(CallUpdate::Capability(capability));
                    }
                    report_organizer_result(result, "attendee restrictions", ATTENDEE_SETTINGS_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::Invite { callees, group_thread }) => {
                    let thread = group_thread.or_else(|| meeting_chat.clone());
                    let result = remote.signaling.add_participants(&remote.conversation, &remote.participant, &callees, thread.as_deref(), meeting_target.as_ref(), &links).await;
                    report_organizer_result(result, "invite", INVITE_PERSON_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::LoadMeetingOptions) => {
                    let update = match &meeting_target {
                        Some(target) => meeting_options::fetch(&inner.session, target).await.map_err(|error| error.to_string()),
                        None => Err("This meeting has no options page".to_owned()),
                    };
                    control.send(CallUpdate::MeetingOptions(update));
                }
                Some(CallCommand::SaveMeetingOptions(writes)) => {
                    let update = match &meeting_target {
                        Some(target) => save_meeting_options(&inner.session, target, &writes).await,
                        None => Err("This meeting has no options page".to_owned()),
                    };
                    if let Ok(options) = &update {
                        let (mic_off, camera_off) = options.attendees_off();
                        capability.attendee_mic_off = mic_off;
                        capability.attendee_camera_off = camera_off;
                        control.send(CallUpdate::Capability(capability));
                    }
                    control.send(CallUpdate::MeetingOptionsSaved(update));
                }
                Some(CallCommand::Spotlight { mri }) => {
                    let result = remote.signaling.spotlight(&remote.conversation, &remote.participant, &mri).await;
                    report_organizer_result(result, "spotlight", SPOTLIGHT_FAILED_NOTICE, control, timeline);
                }
                Some(CallCommand::StopSpotlight { mri }) => {
                    let state_ids: Vec<String> = roster.spotlight_of(&mri).map(|spotlight| spotlight.state_id.clone()).into_iter().collect();
                    if !state_ids.is_empty() {
                        let result = remote.signaling.lower_hands(&remote.conversation, &remote.participant, &state_ids).await.map(|()| 200);
                        report_organizer_result(result, "stop spotlight", STOP_SPOTLIGHT_FAILED_NOTICE, control, timeline);
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
                    run_bot_steps(BotJob::Captions, &mut caption_flow, &roster, &remote, &links, meeting_chat.as_deref(), &RecordingPlan::default(), &mut bot_pending, control, timeline).await;
                    if caption_flow.on() {
                        caption_deadline = None;
                    }
                }
                Some(CallCommand::StartRecording { kind, title }) => {
                    if !is_echo {
                        recording_plan = RecordingPlan { title, kind, ..RecordingPlan::default() };
                        recording_flow.want(true);
                        recording_deadline = Some(Instant::now() + RECORDING_START_LIMIT);
                        control.send(CallUpdate::RecordingPending(true));
                        run_bot_steps(BotJob::Recording, &mut recording_flow, &roster, &remote, &links, meeting_chat.as_deref(), &recording_plan, &mut bot_pending, control, timeline).await;
                    }
                }
                Some(CallCommand::StopRecording { scope }) => {
                    if !is_echo {
                        recording_plan.scope = scope;
                        recording_flow.stop();
                        recording_deadline = None;
                        run_bot_steps(BotJob::Recording, &mut recording_flow, &roster, &remote, &links, meeting_chat.as_deref(), &recording_plan, &mut bot_pending, control, timeline).await;
                    }
                }
                Some(CallCommand::SetCaptionLanguage(language)) => {
                    if let Some(url) = roster.caption_bot().and_then(|bot| bot.command_url.clone())
                        && let Err(error) = remote.signaling.caption_language_command(&url, &remote.participant, &language).await
                    {
                        timeline.record("caption language failed", error.to_string());
                        control.send(CallUpdate::Notice(CAPTION_LANGUAGE_FAILED_NOTICE.into()));
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
                            control.send(CallUpdate::Notice(CONFIRM_RECORDING_FAILED_NOTICE.into()));
                        }
                    }
                }
                Some(CallCommand::Hold(hold)) => {
                    if is_meeting || !remote_set || local_hold == hold || same_peer.is_some() || glare_resend.is_some() || next.is_some() {
                        continue;
                    }
                    if hold && local_video.camera_on() {
                        local_video.stop_camera();
                        control.send(CallUpdate::Camera(false));
                    }
                    let camera = !hold && local_video.camera_on();
                    match offer_same_peer(&current.peer, &current.gathered, local_video.share_mid(), &layout_plan, layout, direct_video_mid.as_deref().unwrap_or(CAMERA_MID), hold, camera, local_video.sharing(), &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                        Ok((browser_offer, signaled, reply_tags)) => {
                            timeline.record_sdp("local offer", &signaled.sdp);
                            same_peer = Some(SamePeerOffer { browser_offer, signaled, change: Change::Hold(hold), reply_tags });
                            same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                            same_peer_grace_until = None;
                        }
                        Err(error) => {
                            timeline.record("hold failed", error.to_string());
                            control.send(CallUpdate::Notice(if hold {
                                HOLD_FAILED_NOTICE.into()
                            } else {
                                RESUME_FAILED_NOTICE.into()
                            }));
                        }
                    }
                }
                Some(CallCommand::Transfer { target, replaces }) => {
                    let Some(url) = media_links.get("transfer").cloned() else {
                        control.send(CallUpdate::TransferResult { succeeded: false });
                        control.send(CallUpdate::Notice(TRANSFER_NOT_ALLOWED_NOTICE.into()));
                        continue;
                    };
                    let transfer_target = TransferTarget { mri: &target.mri, replaces: replaces.as_ref() };
                    match remote.signaling.transfer(&url, &remote.participant, transfer_target, &links).await {
                        Ok(()) => control.send(CallUpdate::Notice(notice_with_detail(
                            TRANSFERRING_NOTICE,
                            target.display_name,
                        ))),
                        Err(error) => {
                            timeline.record("transfer failed", error.to_string());
                            control.send(CallUpdate::TransferResult { succeeded: false });
                            control.send(CallUpdate::Notice(TRANSFER_CALL_FAILED_NOTICE.into()));
                        }
                    }
                }
                Some(CallCommand::OpenWhiteboard { title }) => {
                    let Some(target) = meeting_target.clone() else {
                        control.send(CallUpdate::Notice(WHITEBOARDS_MEETINGS_ONLY_NOTICE.into()));
                        continue;
                    };
                    let session = inner.session.clone();
                    let updates = control.updates();
                    tokio::spawn(async move {
                        let update = match whiteboard::fetch_board(&session, &target, &title).await {
                            Ok(url) => CallUpdate::WhiteboardUrl(url),
                            Err(_) => CallUpdate::Notice(WHITEBOARD_OPEN_FAILED_NOTICE.into()),
                        };
                        let _ = updates.send(update);
                    });
                }
                Some(CallCommand::SetBackground(choice)) => apply_background(choice, &local_video, control, timeline).await,
                Some(CallCommand::SetIncomingVideo(enabled)) => {
                    video.set_incoming_enabled(enabled);
                    if let (Some(mid), Some(mri)) = (&direct_video_mid, &peer_mri) {
                        let key = VideoKey::Person(mri.clone());
                        control.video.forget(&key);
                        video.router().set(mid, enabled.then_some(key));
                    }
                    send_receive_change(control, video.reselect(&roster));
                    control.send(CallUpdate::IncomingVideo(enabled));
                }
                Some(CallCommand::SetVideoWants(wants)) => {
                    if video.set_wants(wants) {
                        send_receive_change(control, video.reselect(&roster));
                    }
                }
                Some(CallCommand::SetShareMotion(on)) => local_video.set_motion(on),
                Some(CallCommand::SetShareStandout(on)) => local_video.set_standout(on),
                Some(CallCommand::SetCameraEffects(effects)) => local_video.set_effects(effects),
                Some(CallCommand::SendReaction(reaction)) => {
                    if let Err(error) = remote.signaling.send_reaction(&remote.conversation, &remote.participant, reaction).await {
                        timeline.record("reaction failed", error.to_string());
                        control.send(CallUpdate::Notice(REACTION_FAILED_NOTICE.into()));
                    }
                }
                Some(CallCommand::EndMeeting) => {
                    stop_local_publish(
                        &mut local_video,
                        &remote,
                        &media_links,
                        &mut media_description_request,
                        control,
                        timeline,
                    )
                    .await;
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
                    // Stop publishing before leave so remotes do not keep a frozen camera tile.
                    stop_local_publish(
                        &mut local_video,
                        &remote,
                        &media_links,
                        &mut media_description_request,
                        control,
                        timeline,
                    )
                    .await;
                    advance(state, CallSignal::LocalLeave, control);
                    break;
                }
            },
            _ = stats_tick.tick(), if remote_set => {
                if local_video.take_effects_failure() {
                    timeline.record("camera effects failed", "segmentation model did not load");
                    control.send(CallUpdate::Notice(CAMERA_EFFECTS_FAILED_NOTICE.into()));
                }
                let speaking = active_speakers::speaking_sources(&active_speakers::audio_sources(&current.peer));
                if speaking != speaking_sources {
                    if speaking_sources.is_empty() && !speaker_sources_seen {
                        speaker_sources_seen = true;
                        timeline.record("active speakers", format!("{} source(s) from RTP", speaking.len()));
                    }
                    speaking_sources = speaking;
                    let mut speakers = roster.mris_for_sources(&speaking_sources);
                    if speakers.is_empty() && !speaking_sources.is_empty() && layout == Layout::OneToOne {
                        speakers = roster.entries().into_iter().map(|entry| entry.mri).filter(|mri| *mri != own_mri).collect();
                    }
                    if video.note_speakers(&speakers) {
                        send_receive_change(control, video.reselect(&roster));
                    }
                    control.send(CallUpdate::Speakers(speakers));
                }
                let snapshot = collect_stats(&current.peer, &layout_plan.screen_share_mids).await;
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
                    control.send(CallUpdate::RecordingPending(false));
                    control.send(CallUpdate::Notice(RECORDING_NOT_STARTED_NOTICE.into()));
                }
                if layout == Layout::Conference
                    && let Some(size) = local_video.camera_size()
                    && let Some(url) = media_links.get("applyChannelParameters").cloned()
                    && let Some(body) = send_capabilities.body_for_mid(
                        local_video
                            .camera_mid()
                            .as_deref()
                            .unwrap_or(CAMERA_MID),
                        size,
                        local_video.video_quality(),
                    )
                {
                    let signaling = signaling.clone();
                    let timeline = timeline.clone();
                    tokio::spawn(async move {
                        match signaling.post_json(SEND_CAPABILITIES_LABEL, &url, body).await {
                            Ok(()) => timeline.record("send capabilities", format!("{}x{} accepted", size.0, size.1)),
                            Err(error) => timeline.record("send capabilities failed", error.to_string()),
                        }
                    });
                }
                for (source, refreshed) in local_video.refresh_send_limits() {
                    match refreshed {
                        Ok(Some(plans)) => timeline.record("send limits", format!("{source:?} {plans:?} after a frame size change")),
                        Ok(None) => {}
                        Err(error) => timeline.record("send limits failed", error.to_string()),
                    }
                }
                if ticks.is_multiple_of(BLUR_TIMING_EVERY_TICKS)
                    && let Some(average_ms) = local_video.blur_average_ms()
                {
                    control.send(CallUpdate::BlurTiming(average_ms));
                    if let Some(model) = local_video.blur_model().filter(|model| Some(*model) != segmentation_model) {
                        timeline.record("segmentation model", format!("{model}, effects {average_ms:.1} ms per frame"));
                        segmentation_model = Some(model);
                    }
                }
                if ticks.is_multiple_of(HEALTH_EVERY_TICKS) {
                    control.send(CallUpdate::Sample(StatsSample {
                        audio_in_packets: snapshot.inbound_packets,
                        audio_out_packets: snapshot.outbound_packets,
                        audio_in_level: snapshot.audio_level,
                        audio_out_level: snapshot.local_level,
                        video: VideoReport {
                            frames_received: live.video_counters.received.load(Ordering::Relaxed),
                            frames_published: live.video_counters.published.load(Ordering::Relaxed),
                            ..snapshot.video.clone()
                        },
                        share_in_packets: snapshot.share_inbound_packets,
                        share_out_packets: snapshot.share_outbound_packets,
                    }));
                }
                if ticks.is_multiple_of(HEALTH_EVERY_TICKS)
                    && snapshot.selected_pair.is_some()
                    && let Some(sample) = health_sampler.sample(std::time::Instant::now(), snapshot.health)
                {
                    let quality = quality_tracker.observe(std::time::Instant::now(), &sample);
                    if local_video.camera_on()
                        && snapshot.camera_target_bitrate > 0
                        && bitrate_governor.observe(
                            std::time::Instant::now(),
                            snapshot.video.remote_round_trip_ms,
                            sample.loss_percent,
                            snapshot.camera_target_bitrate,
                        )
                    {
                        let cap = bitrate_governor.cap_bps();
                        if let Err(error) = local_video.cap_camera_bitrate(cap) {
                            timeline.record("bitrate cap failed", error.to_string());
                        } else {
                            timeline.record(
                                "bitrate cap",
                                match cap {
                                    Some(bps) => format!("{} kbit/s", bps / 1000),
                                    None => "cleared".into(),
                                },
                            );
                        }
                    }
                    control.send(CallUpdate::Health(CallHealth {
                        level: quality.level,
                        cpu_limited: quality.cpu_limited,
                        sample,
                        transport: snapshot.transport.clone(),
                        path: negotiator.path(),
                    }));
                }
                let local_level = if muted && software_audio.is_none() { 0.0 } else { snapshot.local_level as f32 };
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
                            "in packets={} bytes={} level={:.3} rms={decoded_rms:.3} tone={ratio:.2} out packets={} mic={:.3} erle={:.1} dtls={} video in packets={} decoded={} {}x{} out packets={} encoded={} {}x{} mixer reports {} (rtt {:.0} ms) out bytes={} sources={:?} video senders={:?}{}{}",
                            snapshot.inbound_packets, snapshot.inbound_bytes, snapshot.audio_level, snapshot.outbound_packets, snapshot.local_level, snapshot.echo_return_loss_enhancement, snapshot.dtls_state,
                            snapshot.video.inbound_packets, snapshot.video.inbound_frames_decoded, snapshot.video.inbound_width, snapshot.video.inbound_height,
                            snapshot.video.outbound_packets, snapshot.video.outbound_frames_encoded, snapshot.video.outbound_width, snapshot.video.outbound_height,
                            snapshot.video.remote_reports, snapshot.video.remote_round_trip_ms, snapshot.outbound_bytes, snapshot.source_levels, snapshot.video_senders,
                            snapshot.share_receive.as_ref().filter(|_| snapshot.share_inbound_packets > 0).map(ShareReceive::summary).unwrap_or_default(),
                            local_video.camera_capture_summary().unwrap_or_default()
                        ),
                    );
                }
            }
            Some(device_id) = device_changes.recv() => {
                device_debounce.signal_device(Instant::now(), device_id);
            }
            _ = sleep_until(device_debounce.deadline().unwrap_or_else(far_future)), if device_debounce.deadline().is_some() => {
                if let Some(touched_ids) = device_debounce.take_due_devices(Instant::now()) {
                    let previous = std::mem::replace(&mut device_lists, devices::list_devices(&factory));
                    control.send(CallUpdate::Devices(device_lists.clone()));
                    if live.audio.as_ref().is_some_and(AudioSetup::uses_devices) {
                        let input_changed = follow_device_change(Flow::Capture, (&previous.inputs, &device_lists.inputs), &input, &mut applied_input, &touched_ids, |index| devices::select_input_index(&factory, index), timeline, control);
                        let output_changed = follow_device_change(Flow::Render, (&previous.outputs, &device_lists.outputs), &output, &mut applied_output, &touched_ids, |index| devices::select_output_index(&factory, index), timeline, control);
                        if input_changed || output_changed {
                            record_audio_devices(timeline, &device_lists, applied_input.as_ref(), applied_output.as_ref());
                        }
                        if input_changed && software_audio.is_some() {
                            sync_software_audio(&factory, options, &input, muted, remote_audio.clone(), &current.peer, &track, software_wanted(mic_settings, share_sound_wanted, &local_video, live.audio.as_ref()), true, &mut software_audio, mic_settings, control, timeline);
                        }
                    }
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
            _ = sleep_until(next_peer_deadline.unwrap_or_else(far_future)), if next_peer_deadline.is_some() => {
                if let Some(pending) = next.as_mut().filter(|pending| pending.connected && pending.offered.is_none()) {
                    pending.retargeted = true;
                    timeline.record("escalation", "no retargetCompletion, switching anyway");
                } else if let Some(pending) = next.take() {
                    pending.session.peer.close();
                    negotiator.escalation_failed();
                    start_camera_pending = false;
                    if local_video.camera_previewing() {
                        local_video.stop_camera();
                        control.send(CallUpdate::Camera(false));
                    }
                    share_pending = None;
                    portal_pending = None;
                    settle_failed_escalation(pending.offered.is_none(), layout, &mut negotiator, &mut deferred_changes, &mut local_video, control);
                    timeline.record("escalation failed", "no answer for the new peer connection in time");
                    control.send(CallUpdate::Notice(VIDEO_FAILED_NOTICE.into()));
                }
            }
            _ = sleep_until(glare_at.unwrap_or_else(far_future)), if glare_at.is_some() => {
                let resend = glare_resend.take();
                if let Some(resend) = resend.as_ref().filter(|_| same_peer.is_some() || next.is_some()) {
                    timeline.record("glare", "another change took over, ours is dropped");
                    undo_change(resend.change, &mut local_video, control);
                } else if let Some(resend) = resend.as_ref().filter(|resend| layout == Layout::OneToOne && negotiator.path() == MediaPath::Mixer && !matches!(resend.change, Change::Hold(_))) {
                    if negotiator.begin_conference_upgrade() {
                        let pending = escalate(&factory, &grant, options, outgoing_track(software_audio.as_ref(), &track), &remote, &links, &media_leg_id, &media_links, &mut negotiator, live, timeline).await;
                        if pending.is_none() {
                            drop_local_media(&mut local_video, control);
                        }
                        next = pending.or(next);
                    } else {
                        undo_change(resend.change, &mut local_video, control);
                    }
                } else if let Some(resend) = resend {
                    let (hold, camera, share) = match resend.change {
                        Change::Hold(hold) => (hold, !hold && local_video.camera_on(), local_video.sharing()),
                        _ => (local_hold, local_video.camera_on(), local_video.sharing()),
                    };
                    match offer_same_peer(&current.peer, &current.gathered, local_video.share_mid(), &layout_plan, layout, direct_video_mid.as_deref().unwrap_or(CAMERA_MID), hold, camera, share, &remote, &links, &media_leg_id, &media_links, &mut negotiator).await {
                        Ok((browser_offer, signaled, reply_tags)) => {
                            timeline.record_sdp("local offer", &signaled.sdp);
                            same_peer = Some(SamePeerOffer { browser_offer, signaled, change: resend.change, reply_tags });
                            same_peer_deadline = Some(Instant::now() + SAME_PEER_ANSWER_LIMIT);
                            same_peer_grace_until = None;
                        }
                        Err(error) => {
                            timeline.record("offer after glare failed", error.to_string());
                            undo_change(resend.change, &mut local_video, control);
                        }
                    }
                }
            }
            _ = sleep_until(same_peer_deadline.unwrap_or_else(far_future)), if same_peer_deadline.is_some() => {
                same_peer_deadline = None;
                if same_peer.is_some() {
                    same_peer_grace_until = Some(Instant::now() + SAME_PEER_LATE_GRACE);
                    timeline.record(
                        "same-peer offer",
                        "no answer yet, waiting a little longer",
                    );
                }
            }
            _ = sleep_until(same_peer_grace_until.unwrap_or_else(far_future)), if same_peer_grace_until.is_some() => {
                same_peer_grace_until = None;
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
    if software_audio.is_some() {
        timeline.record("software audio", "stopping before tear down");
        drop(software_audio);
    }
    live.callbacks = Some(callbacks);
    Ok(())
}

fn far_future() -> Instant {
    Instant::now() + FAR_FUTURE
}

#[cfg(test)]
mod tests {
    #[test]
    fn gathering_settles_shortly_after_a_udp_relay_or_when_complete() {
        assert!(!gathering_settled(false, None));
        assert!(!gathering_settled(false, Some(Duration::from_millis(50))));
        assert!(gathering_settled(false, Some(RELAY_SETTLE)));
        assert!(gathering_settled(true, None));
    }

    #[tokio::test]
    async fn a_hanging_setup_step_ends_on_cancel_or_after_the_step_timeout() {
        let (handle, mut control) = crate::control::call_channel();
        handle.commands.send(CallCommand::Hangup).unwrap();
        let limit = Duration::from_millis(20);
        let cancelled = setup_step_within(
            &mut control,
            limit,
            std::future::pending::<std::result::Result<(), Error>>(),
        )
        .await;
        assert!(matches!(cancelled, Err(Error::Cancelled)));
        let timed_out = setup_step_within(
            &mut control,
            limit,
            std::future::pending::<std::result::Result<(), Error>>(),
        )
        .await;
        assert!(matches!(timed_out, Err(Error::Signaling(_))));
    }

    use super::*;

    #[test]
    fn an_echo_call_waits_for_the_echo_bot_before_it_counts_as_connected() {
        let mut gate = CalleeGate::new(CallSpec::Echo.callee_count());
        assert!(!gate.media_connected());
        assert!(gate.callee_joined());
    }

    #[test]
    fn only_a_conflict_answer_retries_a_recording_stop() {
        assert!(recorder_busy(&Error::Signaling(
            "recording command answered HTTP 409 no answer".into()
        )));
        assert!(!recorder_busy(&Error::Signaling(
            "recording command answered HTTP 500 no answer".into()
        )));
        assert!(!recorder_busy(&Error::Cancelled));
    }

    #[test]
    fn escalation_keeps_the_software_track_when_it_runs() {
        assert_eq!(*pick_track(Some(&1), &0), 1);
        assert_eq!(*pick_track(None, &0), 0);
    }

    #[test]
    fn video_is_possible_once_connected_and_not_waiting_on_a_mixer_escalation() {
        assert!(video_possible(true, Layout::OneToOne, MediaPath::Direct));
        assert!(video_possible(true, Layout::Conference, MediaPath::Mixer));
        assert!(!video_possible(true, Layout::OneToOne, MediaPath::Mixer));
        assert!(!video_possible(
            false,
            Layout::Conference,
            MediaPath::Direct
        ));
    }

    #[test]
    fn camera_does_not_start_while_a_peer_escalation_is_pending() {
        assert!(camera_may_start(true, false, true));
        assert!(!camera_may_start(true, true, true));
        assert!(!camera_may_start(false, false, true));
        assert!(!camera_may_start(true, false, false));
    }

    #[test]
    fn leg_switch_keeps_mixer_control_links_when_renegotiation_omits_them() {
        let mut media_links = BTreeMap::from([
            (
                "updateMediaDescriptions".to_owned(),
                "https://m/update".to_owned(),
            ),
            (
                "applyChannelParameters".to_owned(),
                "https://m/apply".to_owned(),
            ),
            ("callLeg".to_owned(), "https://m/leg".to_owned()),
        ]);
        replace_leg_media_links(
            &mut media_links,
            &BTreeMap::from([("mediaAnswer".to_owned(), "https://m/answer".to_owned())]),
        );
        assert_eq!(
            media_links
                .get("updateMediaDescriptions")
                .map(String::as_str),
            Some("https://m/update")
        );
        assert_eq!(
            media_links
                .get("applyChannelParameters")
                .map(String::as_str),
            Some("https://m/apply")
        );
        assert_eq!(
            media_links.get("mediaAnswer").map(String::as_str),
            Some("https://m/answer")
        );
        replace_leg_media_links(
            &mut media_links,
            &BTreeMap::from([(
                "updateMediaDescriptions".to_owned(),
                "https://m/update-new".to_owned(),
            )]),
        );
        assert_eq!(
            media_links
                .get("updateMediaDescriptions")
                .map(String::as_str),
            Some("https://m/update-new")
        );
    }

    #[test]
    fn keep_alive_fires_at_ninety_percent_of_the_interval() {
        assert_eq!(keep_alive_period(Some(2700)), Duration::from_secs(2430));
        assert_eq!(keep_alive_period(None), Duration::from_secs(2430));
        assert_eq!(keep_alive_period(Some(10)), Duration::from_secs(54));
    }

    const BROWSER_OFFER: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\na=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\na=mid:1\r\na=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\na=mid:2\r\na=recvonly\r\n";

    fn directions(sdp: &str) -> Vec<String> {
        crate::sdp::parse(sdp)
            .unwrap()
            .media
            .iter()
            .map(|line| line.direction().unwrap_or("none").to_owned())
            .collect()
    }

    async fn local_offer(layout: Layout) -> Result<OfferedMedia> {
        use libwebrtc::audio_source::native::NativeAudioSource;
        use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
        let factory = devices::shared_factory();
        let peer = factory
            .create_peer_connection(RtcConfiguration::default())
            .unwrap();
        let source = NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, 100);
        let track = factory.create_audio_track("microphone", source);
        let offer = create_offer(&peer, &factory, &track, layout, None).await;
        peer.close();
        offer
    }

    #[tokio::test]
    async fn libwebrtc_accepts_its_own_offer_after_the_direction_rewrite() {
        for layout in [Layout::OneToOne, Layout::Conference] {
            if let Err(error) = local_offer(layout).await {
                panic!("{layout:?}: {error}");
            }
        }
    }

    #[tokio::test]
    async fn a_peer_to_peer_offer_from_the_teams_client_gets_an_answer() {
        use libwebrtc::audio_source::native::NativeAudioSource;
        use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
        let factory = devices::shared_factory();
        let peer = factory
            .create_peer_connection(RtcConfiguration::default())
            .unwrap();
        let source = NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, 100);
        let track = factory.create_audio_track("microphone", source);
        let offer = include_str!("../../tests/fixtures/teams_p2p_offer.sdp");
        let answered = answer_offer(
            &peer,
            Some(&track),
            offer,
            AnswerVideo::Receive,
            None,
            false,
            None,
        )
        .await;
        peer.close();
        if let Err(error) = answered {
            panic!("{error}");
        }
    }

    #[test]
    fn a_one_to_one_offer_has_audio_and_an_inactive_camera_until_the_camera_is_on() {
        assert_eq!(
            directions(&directions_for(
                Layout::OneToOne,
                false,
                false,
                false,
                CAMERA_MID,
                Some(ONE_TO_ONE_SHARE_MID),
                false,
                BROWSER_OFFER
            )),
            vec!["sendrecv", "inactive", "inactive"]
        );
        assert_eq!(
            directions(&directions_for(
                Layout::OneToOne,
                false,
                true,
                false,
                CAMERA_MID,
                Some(ONE_TO_ONE_SHARE_MID),
                false,
                BROWSER_OFFER
            )),
            vec!["sendrecv", "sendrecv", "inactive"]
        );
        assert_eq!(
            directions(&directions_for(
                Layout::OneToOne,
                false,
                false,
                true,
                CAMERA_MID,
                Some(ONE_TO_ONE_SHARE_MID),
                false,
                BROWSER_OFFER
            )),
            vec!["sendrecv", "inactive", "sendonly"]
        );
    }

    #[test]
    fn a_hold_silences_every_line_and_a_conference_offer_is_left_alone() {
        for layout in [Layout::OneToOne, Layout::Conference] {
            assert_eq!(
                directions(&directions_for(
                    layout,
                    true,
                    true,
                    false,
                    CAMERA_MID,
                    None,
                    false,
                    BROWSER_OFFER
                )),
                vec!["inactive", "inactive", "inactive"]
            );
        }
        assert_eq!(
            directions_for(
                Layout::Conference,
                false,
                true,
                false,
                CAMERA_MID,
                None,
                false,
                BROWSER_OFFER
            ),
            BROWSER_OFFER
        );
    }

    #[test]
    fn a_hold_offer_body_is_inactive_on_every_teams_line_with_one_placeholder_candidate() {
        let held = directions_for(
            Layout::Conference,
            true,
            false,
            false,
            CAMERA_MID,
            None,
            false,
            BROWSER_OFFER,
        );
        let mut numbers = || 7;
        let signaled = to_teams_offer(&held, &offer_plan(), &mut numbers)
            .unwrap()
            .sdp;
        assert_eq!(
            directions(&signaled),
            vec!["inactive", "inactive", "inactive"]
        );
        assert_eq!(signaled.matches("a=candidate:").count(), 1);
    }

    #[test]
    fn a_move_into_the_meeting_you_are_already_in_is_ignored() {
        let target = |thread: &str| MeetingTarget {
            thread_id: thread.into(),
            tenant_id: "t".into(),
            organizer_id: "o".into(),
            meeting_data: None,
        };
        assert!(is_own_meeting(
            Some(&target("19:meeting_a@thread.v2")),
            &target("19:meeting_a@thread.v2")
        ));
        assert!(!is_own_meeting(
            Some(&target("19:meeting_a@thread.v2")),
            &target("19:meeting_b@thread.v2")
        ));
        assert!(!is_own_meeting(None, &target("19:meeting_a@thread.v2")));
    }

    #[test]
    fn a_video_call_names_the_video_modality_only_when_the_camera_sends() {
        assert_eq!(accepted_modalities(false), vec!["Audio".to_owned()]);
        assert_eq!(
            accepted_modalities(true),
            vec!["Audio".to_owned(), "Video".to_owned()]
        );
    }

    #[test]
    fn specs_map_to_invitation_targets() {
        assert_eq!(CallSpec::Echo.target(), InviteTarget::Echo);
        let people = CallSpec::People {
            callees: vec![Callee {
                mri: "8:orgid:b".into(),
                display_name: "Bea".into(),
            }],
            thread_id: "19:t@thread.v2".into(),
        };
        assert!(people.is_people() && !people.is_meeting());
        assert!(
            matches!(people.target(), InviteTarget::People { thread_id, .. } if thread_id == "19:t@thread.v2")
        );
        let meeting = CallSpec::Meeting(MeetingTarget {
            thread_id: "19:meeting@thread.v2".into(),
            tenant_id: "t".into(),
            organizer_id: "o".into(),
            meeting_data: None,
        });
        assert!(meeting.is_meeting() && !meeting.is_people());
    }
}
