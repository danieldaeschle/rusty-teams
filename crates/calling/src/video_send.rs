use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::rtp_parameters::DegradationPreference;
use libwebrtc::rtp_sender::RtpSender;
use libwebrtc::rtp_transceiver::{RtpTransceiver, RtpTransceiverDirection};
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _, VideoFrame, VideoRotation};
use libwebrtc::video_source::VideoResolution;
use libwebrtc::video_source::native::NativeVideoSource;
use libwebrtc::video_track::RtcVideoTrack;
use tokio::sync::mpsc::UnboundedSender;

use crate::background::BackgroundPicture;
use crate::blur::BlurSettings;
use crate::camera::{CameraCaptureStats, start_camera};
use crate::video_quality::VideoQuality;
use crate::capture::Capture;
use crate::control::{CallCommand, CallUpdate};
use crate::notices::{CAMERA_STOPPED_NOTICE, SCREEN_SHARE_STOPPED_NOTICE, notice_with_detail};
use crate::devices::DeviceChoice;
use crate::effects::CameraEffects;
use crate::error::{Error, Result};
use crate::timeline::Timeline;
use crate::screen::{
    PortalFollowUp, ShareKind, ShareSource, start_portal_capture, start_screen_capture,
};
use crate::send_control::{
    EncoderLimits, KEYFRAME_NUDGE, LayerPlan, SendLimit, encoder_limits, plan_layers,
};
use crate::standout::StandoutOverlay;
use crate::video_frame::{
    FrameGate, PERSON_FPS, SCREEN_FPS, VideoHub, VideoKey, picture_from_i420,
};
use crate::video_layout::VideoLines;
use crate::video_pattern::{PatternKind, spawn_pattern};

pub const VIDEO_MODE_ENV: &str = "CALLING_VIDEO";
const CAMERA_LABEL: &str = "camera";
const SCREEN_LABEL: &str = "screen";
pub const SCREEN_SHARE_FPS: u32 = 15;
pub const MOTION_SHARE_FPS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoMode {
    #[default]
    Platform,
    Pattern,
    Off,
}

impl VideoMode {
    pub fn from_env() -> Self {
        Self::parse(std::env::var(VIDEO_MODE_ENV).ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("pattern") => VideoMode::Pattern,
            Some("off") => VideoMode::Off,
            _ => VideoMode::Platform,
        }
    }
}

/// Where a captured frame goes: the encoder source and, for the self view, the UI hub.
pub struct LocalSink {
    source: NativeVideoSource,
    self_view: Option<(Arc<VideoHub>, VideoKey, FrameGate)>,
    mirror: Option<Arc<BlurSettings>>,
    standout: Option<StandoutOverlay>,
    frame_size: Option<Arc<AtomicU64>>,
}

impl LocalSink {
    pub fn new(source: NativeVideoSource, hub: Option<(Arc<VideoHub>, VideoKey)>) -> Self {
        let self_view = hub.map(|(hub, key)| {
            let fps = if matches!(key, VideoKey::LocalScreen) {
                SCREEN_FPS
            } else {
                PERSON_FPS
            };
            (hub, key, FrameGate::new(fps))
        });
        LocalSink {
            source,
            self_view,
            mirror: None,
            standout: None,
            frame_size: None,
        }
    }

    pub fn reporting_size(mut self, frame_size: Arc<AtomicU64>) -> Self {
        self.frame_size = Some(frame_size);
        self
    }

    pub fn mirrored_by(mut self, settings: Arc<BlurSettings>) -> Self {
        self.mirror = Some(settings);
        self
    }

    pub fn with_standout(mut self, settings: Arc<BlurSettings>) -> Self {
        self.standout = Some(StandoutOverlay::new(settings));
        self
    }

    pub fn push(&mut self, buffer: I420Buffer) {
        let buffer = match &mut self.standout {
            Some(overlay) => overlay.apply(buffer),
            None => buffer,
        };
        if let Some(frame_size) = &self.frame_size {
            frame_size.store(
                pack_size(buffer.width(), buffer.height()),
                Ordering::Relaxed,
            );
        }
        let mut frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        frame.timestamp_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_micros() as i64);
        self.source.capture_frame(&frame);
        let Some((hub, key, gate)) = &mut self.self_view else {
            return;
        };
        if gate.admit(Instant::now())
            && let Some(mut picture) = picture_from_i420(frame.buffer, key.max_width())
        {
            if self
                .mirror
                .as_ref()
                .is_some_and(|settings| settings.effects().mirror)
            {
                picture.flip_horizontally();
            }
            hub.publish(key.clone(), picture);
        }
    }
}

fn pack_size(width: u32, height: u32) -> u64 {
    (u64::from(width) << 32) | u64::from(height)
}

fn unpack_size(packed: u64) -> Option<(u32, u32)> {
    let (width, height) = ((packed >> 32) as u32, packed as u32);
    (width > 0 && height > 0).then_some((width, height))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendSource {
    Camera,
    Screen,
}

struct Outgoing {
    source: NativeVideoSource,
    track: RtcVideoTrack,
    capture: Option<Capture>,
    frame_size: Arc<AtomicU64>,
    requested: Option<RequestedLimits>,
}

struct RequestedLimits {
    limits: Vec<SendLimit>,
    applied_for: Option<(u32, u32)>,
}

impl Outgoing {
    fn new(
        factory: &PeerConnectionFactory,
        label: &str,
        kind: PatternKind,
        screencast: bool,
    ) -> Self {
        let (width, height) = kind.size();
        let source = NativeVideoSource::new(VideoResolution { width, height }, screencast);
        let track = factory.create_video_track(label, source.clone());
        Outgoing {
            source,
            track,
            capture: None,
            frame_size: Arc::new(AtomicU64::new(0)),
            requested: None,
        }
    }
}

/// Camera and screen share of the local user; frames come from devices or a generated pattern.
pub struct LocalVideo {
    mode: VideoMode,
    hub: Arc<VideoHub>,
    updates: UnboundedSender<CallUpdate>,
    camera: Outgoing,
    screen: Outgoing,
    lines: Option<VideoLines>,
    blur: Arc<BlurSettings>,
    motion: bool,
    share_fps: Arc<AtomicU32>,
    camera_preview: bool,
    quality: VideoQuality,
    camera_stats: Arc<CameraCaptureStats>,
    timeline: Option<Timeline>,
    portal_generation: u64,
    portal_armed: bool,
    portal_keep: bool,
    /// The desktop dialog already bound the PipeWire stream. Later `StartShare`
    /// commands must attach that capturer, not replace it.
    portal_bound: bool,
}

impl LocalVideo {
    pub fn new(
        factory: &PeerConnectionFactory,
        mode: VideoMode,
        hub: Arc<VideoHub>,
        updates: UnboundedSender<CallUpdate>,
    ) -> Self {
        LocalVideo {
            mode,
            hub,
            updates,
            camera: Outgoing::new(factory, CAMERA_LABEL, PatternKind::Camera, false),
            screen: Outgoing::new(factory, SCREEN_LABEL, PatternKind::Screen, true),
            lines: None,
            blur: Arc::new(BlurSettings::default()),
            motion: false,
            share_fps: Arc::new(AtomicU32::new(SCREEN_SHARE_FPS)),
            camera_preview: false,
            quality: VideoQuality::default(),
            camera_stats: CameraCaptureStats::new(),
            timeline: None,
            portal_generation: 0,
            portal_armed: false,
            portal_keep: false,
            portal_bound: false,
        }
    }

    pub fn attach_timeline(&mut self, timeline: Timeline) {
        self.timeline = Some(timeline);
    }

    pub fn video_quality(&self) -> VideoQuality {
        self.quality
    }

    pub fn set_video_quality(&mut self, quality: VideoQuality) {
        self.quality = quality;
    }

    pub fn set_motion(&mut self, motion: bool) {
        self.motion = motion;
        self.share_fps.store(
            if motion {
                MOTION_SHARE_FPS
            } else {
                SCREEN_SHARE_FPS
            },
            Ordering::Relaxed,
        );
        if let Some(share) = self
            .lines
            .as_ref()
            .and_then(|lines| lines.share.as_ref())
            .filter(|_| self.sharing())
        {
            let _ = self.tune_share(&share.sender());
        }
    }

    pub fn set_standout(&self, standout: bool) {
        self.blur.set_standout(standout);
    }

    fn tune_share(&self, sender: &RtpSender) -> Result<()> {
        let mut parameters = sender.parameters();
        parameters.set_degradation_preference(if self.motion {
            DegradationPreference::MaintainFramerate
        } else {
            DegradationPreference::MaintainResolution
        });
        let fps = f64::from(self.share_fps.load(Ordering::Relaxed));
        for encoding in &mut parameters.encodings {
            encoding.max_framerate = Some(fps);
        }
        sender
            .set_parameters(parameters)
            .map_err(|error| Error::Webrtc(error.to_string()))
    }

    fn sender_of(&self, source: SendSource) -> Option<RtpSender> {
        let lines = self.lines.as_ref()?;
        match source {
            SendSource::Camera => Some(lines.camera.sender()),
            SendSource::Screen => lines.share.as_ref().map(|share| share.sender()),
        }
    }

    pub fn apply_send_limit(
        &mut self,
        source: SendSource,
        limits: &[SendLimit],
    ) -> Result<Option<Vec<LayerPlan>>> {
        let sending = match source {
            SendSource::Camera => self.camera_on(),
            SendSource::Screen => self.sharing(),
        };
        let Some(sender) = self.sender_of(source).filter(|_| sending) else {
            return Ok(None);
        };
        let outgoing = match source {
            SendSource::Camera => &mut self.camera,
            SendSource::Screen => &mut self.screen,
        };
        let frame = unpack_size(outgoing.frame_size.load(Ordering::Relaxed));
        outgoing.requested = Some(RequestedLimits {
            limits: limits.to_vec(),
            applied_for: frame,
        });
        let rids: Vec<String> = sender
            .parameters()
            .encodings
            .into_iter()
            .map(|encoding| encoding.rid)
            .collect();
        if rids.len() > 1 {
            let plans = plan_layers(limits, frame, &rids);
            if plans.iter().any(|plan| plan.keyframe) {
                Self::set_layers(&sender, &plans, KEYFRAME_NUDGE)?;
            }
            Self::set_layers(&sender, &plans, 1.0)?;
            return Ok(Some(plans));
        }
        let Some(limit) = limits.first() else {
            return Ok(None);
        };
        let encoder = encoder_limits(limit, frame);
        if limit.keyframe {
            Self::set_limits(&sender, &encoder, KEYFRAME_NUDGE)?;
        }
        Self::set_limits(&sender, &encoder, 1.0)?;
        Ok(Some(vec![LayerPlan {
            rid: rids.into_iter().next().unwrap_or_default(),
            active: true,
            limits: Some(encoder),
            keyframe: limit.keyframe,
        }]))
    }

    /// The mixer sends its limits once, often before the first frame tells the size to scale from.
    pub fn refresh_send_limits(&mut self) -> Vec<(SendSource, Result<Option<Vec<LayerPlan>>>)> {
        let mut refreshed = Vec::new();
        for source in [SendSource::Camera, SendSource::Screen] {
            let outgoing = match source {
                SendSource::Camera => &self.camera,
                SendSource::Screen => &self.screen,
            };
            let frame = unpack_size(outgoing.frame_size.load(Ordering::Relaxed));
            let Some(requested) = outgoing
                .requested
                .as_ref()
                .filter(|requested| frame.is_some() && requested.applied_for != frame)
            else {
                continue;
            };
            let limits = requested.limits.clone();
            refreshed.push((source, self.apply_send_limit(source, &limits)));
        }
        refreshed
    }

    fn set_layers(sender: &RtpSender, plans: &[LayerPlan], nudge: f64) -> Result<()> {
        let mut parameters = sender.parameters();
        for encoding in &mut parameters.encodings {
            let Some(plan) = plans.iter().find(|plan| plan.rid == encoding.rid) else {
                continue;
            };
            let nudged = nudge != 1.0 && plan.keyframe;
            encoding.active = plan.active;
            let Some(limits) = &plan.limits else { continue };
            encoding.scale_resolution_down_by =
                Some(limits.scale_down_by * if nudged { nudge } else { 1.0 });
            if let Some(framerate) = limits.max_framerate {
                encoding.max_framerate = Some(framerate);
            }
            encoding.max_bitrate = limits.max_bitrate;
        }
        sender
            .set_parameters(parameters)
            .map_err(|error| Error::Webrtc(error.to_string()))
    }

    fn set_limits(sender: &RtpSender, limits: &EncoderLimits, nudge: f64) -> Result<()> {
        let mut parameters = sender.parameters();
        for encoding in &mut parameters.encodings {
            encoding.scale_resolution_down_by = Some(limits.scale_down_by * nudge);
            if let Some(framerate) = limits.max_framerate {
                encoding.max_framerate = Some(framerate);
            }
            encoding.max_bitrate = limits.max_bitrate;
        }
        sender
            .set_parameters(parameters)
            .map_err(|error| Error::Webrtc(error.to_string()))
    }

    pub fn camera_size(&self) -> Option<(u32, u32)> {
        unpack_size(self.camera.frame_size.load(Ordering::Relaxed)).filter(|_| self.camera_on())
    }

    pub fn set_background(&self, enabled: bool, picture: Option<Arc<BackgroundPicture>>) {
        self.blur.set_picture(picture);
        self.blur.set_enabled(enabled);
    }

    pub fn set_effects(&self, effects: CameraEffects) {
        self.blur.set_effects(effects);
    }

    pub fn camera_track(&self) -> RtcVideoTrack {
        self.camera.track.clone()
    }

    pub fn blur_average_ms(&self) -> Option<f32> {
        (self.camera_on() && self.blur.active()).then(|| self.blur.average_ms())
    }

    pub fn blur_model(&self) -> Option<&'static str> {
        (self.camera_on() && self.blur.active())
            .then(|| self.blur.model())
            .filter(|model| !model.is_empty())
    }

    pub fn has_lines(&self) -> bool {
        self.lines.is_some()
    }

    pub fn share_mid(&self) -> Option<String> {
        self.lines
            .as_ref()
            .and_then(|lines| lines.share.as_ref())
            .and_then(|share| share.mid())
    }

    pub fn has_share_line(&self) -> bool {
        self.lines
            .as_ref()
            .is_some_and(|lines| lines.share.is_some())
    }

    pub fn add_share_line(
        &mut self,
        share: RtpTransceiver,
        camera: Option<RtpTransceiver>,
    ) -> Result<()> {
        match (self.lines.as_mut(), camera) {
            (Some(lines), _) => lines.share = Some(share),
            (None, Some(camera)) => {
                self.lines = Some(VideoLines {
                    camera,
                    share: Some(share),
                })
            }
            (None, None) => return Err(Error::Webrtc("no video line negotiated".into())),
        }
        Ok(())
    }

    pub fn share_sending(&self) -> bool {
        self.lines
            .as_ref()
            .and_then(|lines| lines.share.as_ref())
            .and_then(|share| share.current_direction())
            .is_some_and(|direction| {
                matches!(
                    direction,
                    RtpTransceiverDirection::SendOnly | RtpTransceiverDirection::SendRecv
                )
            })
    }

    pub fn is_available(&self) -> bool {
        self.mode != VideoMode::Off
    }

    pub fn camera_on(&self) -> bool {
        self.camera.capture.is_some() && !self.camera_preview
    }

    pub fn camera_previewing(&self) -> bool {
        self.camera.capture.is_some() && self.camera_preview
    }

    pub fn sharing(&self) -> bool {
        self.screen.capture.is_some()
    }

    pub fn portal_is_open(&self) -> bool {
        self.portal_armed
    }

    pub fn set_lines(&mut self, lines: VideoLines) -> Result<()> {
        if self.camera_on() {
            Self::begin_sending(&lines.camera.sender(), &self.camera.track)?;
        }
        if self.sharing()
            && let Some(share) = &lines.share
        {
            Self::begin_sending(&share.sender(), &self.screen.track)?;
        }
        self.lines = Some(lines);
        Ok(())
    }

    fn begin_sending(sender: &RtpSender, track: &RtcVideoTrack) -> Result<()> {
        sender
            .set_track(Some(MediaStreamTrack::Video(track.clone())))
            .map_err(|error| Error::Webrtc(error.to_string()))?;
        Self::set_active(sender, true)
    }

    fn set_active(sender: &RtpSender, active: bool) -> Result<()> {
        let mut parameters = sender.parameters();
        for encoding in &mut parameters.encodings {
            encoding.active = active;
        }
        sender
            .set_parameters(parameters)
            .map_err(|error| Error::Webrtc(error.to_string()))
    }

    fn camera_sink(&self) -> LocalSink {
        LocalSink::new(
            self.camera.source.clone(),
            Some((self.hub.clone(), VideoKey::LocalCamera)),
        )
        .mirrored_by(self.blur.clone())
        .reporting_size(self.camera.frame_size.clone())
    }

    /// Local share frames go to the encoder and to the window in the chat.
    fn screen_sink(&self) -> LocalSink {
        LocalSink::new(
            self.screen.source.clone(),
            Some((self.hub.clone(), VideoKey::LocalScreen)),
        )
        .with_standout(self.blur.clone())
        .reporting_size(self.screen.frame_size.clone())
    }

    fn report_failure(&self, notice_key: &'static str) -> impl FnOnce(String) + Send + 'static {
        let updates = self.updates.clone();
        move |reason| {
            let _ = updates.send(CallUpdate::Notice(notice_with_detail(notice_key, reason)));
        }
    }

    pub fn preview_camera(&mut self, choice: &DeviceChoice) -> Result<()> {
        if !self.is_available() {
            return Err(Error::Webrtc("video is switched off".into()));
        }
        self.camera.capture = None;
        self.camera.capture = Some(self.capture_camera(choice)?);
        self.camera_preview = true;
        Ok(())
    }

    pub fn start_camera(&mut self, choice: &DeviceChoice) -> Result<()> {
        let lines = self
            .lines
            .as_ref()
            .ok_or_else(|| Error::Webrtc("no video line negotiated".into()))?;
        if !self.is_available() {
            return Err(Error::Webrtc("video is switched off".into()));
        }
        let sender = lines.camera.sender();
        if !self.camera_previewing() {
            self.camera.capture = None;
            self.camera.capture = Some(self.capture_camera(choice)?);
        }
        Self::begin_sending(&sender, &self.camera.track)?;
        self.camera_preview = false;
        Ok(())
    }

    pub fn camera_capture_summary(&self) -> Option<String> {
        self.camera_stats.summary()
    }

    fn capture_camera(&self, choice: &DeviceChoice) -> Result<Capture> {
        let mut sink = self.camera_sink();
        Ok(match self.mode {
            VideoMode::Pattern => Capture::Task(spawn_pattern(
                sink,
                PatternKind::Camera,
                Some(self.blur.clone()),
            )),
            _ => {
                let timeline = self.timeline.clone();
                start_camera(
                    choice,
                    self.quality,
                    self.blur.clone(),
                    self.camera_stats.clone(),
                    move |label| {
                        if let Some(timeline) = timeline {
                            timeline.record("camera capture", label);
                        } else {
                            eprintln!("calls: camera capture {label}");
                        }
                    },
                    move |buffer| sink.push(buffer),
                    self.report_failure(CAMERA_STOPPED_NOTICE),
                )?
            }
        })
    }

    /// Restart capture with the current quality if the camera is already running.
    pub fn refresh_camera_capture(&mut self, choice: &DeviceChoice) -> Result<()> {
        if self.camera.capture.is_none() {
            return Ok(());
        }
        let preview = self.camera_preview;
        self.camera.capture = None;
        self.camera.capture = Some(self.capture_camera(choice)?);
        self.camera_preview = preview;
        if !preview {
            if let Some(lines) = self.lines.as_ref() {
                Self::begin_sending(&lines.camera.sender(), &self.camera.track)?;
            }
        }
        Ok(())
    }

    pub fn stop_camera(&mut self) {
        self.camera_preview = false;
        self.camera.capture = None;
        self.blur.set_cutout(None);
        if let Some(lines) = &self.lines {
            let _ = Self::set_active(&lines.camera.sender(), false);
        }
        self.hub.forget(&VideoKey::LocalCamera);
    }

    pub fn accept_portal(&mut self, generation: u64) -> bool {
        if self.portal_armed && self.portal_generation == generation {
            self.portal_armed = false;
            self.portal_keep = true;
            self.portal_bound = true;
            true
        } else {
            false
        }
    }

    pub fn open_portal(
        &mut self,
        kind: ShareKind,
        commands: UnboundedSender<CallCommand>,
    ) -> Result<()> {
        if !self.is_available() {
            return Err(Error::Webrtc("video is switched off".into()));
        }
        if self.mode == VideoMode::Pattern {
            let source = ShareSource {
                id: if kind == ShareKind::Screen { 1 } else { 2 },
                kind,
                title: if kind == ShareKind::Screen {
                    "Test screen"
                } else {
                    "Test window"
                }
                .into(),
            };
            let _ = commands.send(CallCommand::StartShare(source));
            return Ok(());
        }
        self.portal_generation = self.portal_generation.wrapping_add(1);
        let generation = self.portal_generation;
        self.portal_armed = false;
        self.portal_keep = false;
        self.portal_bound = false;
        self.screen.capture = None;
        let mut sink = self.screen_sink();
        let updates = self.updates.clone();
        let capture = start_portal_capture(
            kind,
            self.share_fps.clone(),
            move |buffer| sink.push(buffer),
            move |follow| match follow {
                PortalFollowUp::Share(source) => {
                    let _ = commands.send(CallCommand::FinishPortalShare { source, generation });
                }
                PortalFollowUp::Quiet => {
                    let _ = commands.send(CallCommand::AbandonPortalShare);
                }
                PortalFollowUp::Notice(text) => {
                    let _ = updates.send(CallUpdate::Notice(text.to_owned()));
                    let _ = commands.send(CallCommand::AbandonPortalShare);
                }
            },
        )?;
        self.portal_armed = true;
        self.screen.capture = Some(capture);
        Ok(())
    }

    pub fn start_share(&mut self, source: &ShareSource) -> Result<()> {
        let lines = self
            .lines
            .as_ref()
            .ok_or_else(|| Error::Webrtc("no video line negotiated".into()))?;
        let share = lines
            .share
            .as_ref()
            .ok_or_else(|| Error::Webrtc("sharing needs a meeting".into()))?;
        if !self.is_available() {
            return Err(Error::Webrtc("video is switched off".into()));
        }
        let keep = std::mem::take(&mut self.portal_keep) || self.portal_bound;
        if !keep {
            self.portal_armed = false;
            self.portal_generation = self.portal_generation.wrapping_add(1);
            self.screen.capture = None;
            let mut sink = self.screen_sink();
            let capture = match self.mode {
                VideoMode::Pattern => Capture::Task(spawn_pattern(sink, PatternKind::Screen, None)),
                _ => start_screen_capture(
                    source.clone(),
                    self.share_fps.clone(),
                    move |buffer| sink.push(buffer),
                    self.report_failure(SCREEN_SHARE_STOPPED_NOTICE),
                )?,
            };
            self.screen.capture = Some(capture);
        }
        Self::begin_sending(&share.sender(), &self.screen.track)?;
        let _ = self.tune_share(&share.sender());
        Ok(())
    }

    pub fn stop_share(&mut self) {
        self.portal_generation = self.portal_generation.wrapping_add(1);
        self.portal_armed = false;
        self.portal_keep = false;
        self.portal_bound = false;
        self.screen.capture = None;
        self.hub.forget(&VideoKey::LocalScreen);
        if let Some(share) = self.lines.as_ref().and_then(|lines| lines.share.as_ref()) {
            let _ = Self::set_active(&share.sender(), false);
        }
    }

    pub fn stop_all(&mut self) {
        self.stop_camera();
        self.stop_share();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_camera_preview_runs_without_a_line_and_stops_cleanly() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let factory = crate::devices::shared_factory();
        let mut video = LocalVideo::new(
            &factory,
            VideoMode::Pattern,
            VideoHub::new(sender.clone()),
            sender,
        );
        video.preview_camera(&DeviceChoice::SystemDefault).unwrap();
        assert!(video.camera_previewing() && !video.camera_on());
        assert!(
            video.start_camera(&DeviceChoice::SystemDefault).is_err(),
            "sending needs a negotiated line"
        );
        assert!(video.camera_previewing());
        video.stop_camera();
        assert!(!video.camera_previewing() && !video.camera_on());
    }

    #[test]
    fn frame_sizes_survive_packing() {
        assert_eq!(unpack_size(pack_size(1280, 720)), Some((1280, 720)));
        assert_eq!(unpack_size(0), None);
    }

    #[test]
    fn the_video_mode_follows_the_environment_value() {
        assert_eq!(VideoMode::parse(None), VideoMode::Platform);
        assert_eq!(VideoMode::parse(Some("Pattern")), VideoMode::Pattern);
        assert_eq!(VideoMode::parse(Some("off")), VideoMode::Off);
        assert_eq!(VideoMode::parse(Some("whatever")), VideoMode::Platform);
    }
}
