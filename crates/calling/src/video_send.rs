use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::rtp_sender::RtpSender;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
use libwebrtc::video_source::VideoResolution;
use libwebrtc::video_source::native::NativeVideoSource;
use libwebrtc::video_track::RtcVideoTrack;
use tokio::sync::mpsc::UnboundedSender;

use crate::blur::{BlurSettings, BlurStage};
use crate::camera::start_camera;
use crate::capture::Capture;
use crate::control::CallUpdate;
use crate::devices::DeviceChoice;
use crate::error::{Error, Result};
use crate::screen::{ShareSource, start_screen_capture};
use crate::video_frame::{FrameGate, SCREEN_FPS, VideoHub, VideoKey, picture_from_i420};
use crate::video_layout::VideoLines;
use crate::video_pattern::{PatternKind, spawn_pattern};

pub const VIDEO_MODE_ENV: &str = "CALLING_VIDEO";
const CAMERA_LABEL: &str = "camera";
const SCREEN_LABEL: &str = "screen";
const SELF_VIEW_FPS: u32 = 15;

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
}

impl LocalSink {
    pub fn new(source: NativeVideoSource, hub: Option<(Arc<VideoHub>, VideoKey)>) -> Self {
        let self_view = hub.map(|(hub, key)| {
            let fps = if matches!(key, VideoKey::LocalScreen) { SCREEN_FPS } else { SELF_VIEW_FPS };
            (hub, key, FrameGate::new(fps))
        });
        LocalSink { source, self_view }
    }

    pub fn push(&mut self, buffer: I420Buffer) {
        let mut frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        frame.timestamp_us = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros() as i64);
        self.source.capture_frame(&frame);
        let Some((hub, key, gate)) = &mut self.self_view else { return };
        if gate.admit(Instant::now())
            && let Some(picture) = picture_from_i420(frame.buffer, key.max_width())
        {
            hub.publish(key.clone(), picture);
        }
    }
}

struct Outgoing {
    source: NativeVideoSource,
    track: RtcVideoTrack,
    capture: Option<Capture>,
}

impl Outgoing {
    fn new(factory: &PeerConnectionFactory, label: &str, kind: PatternKind, screencast: bool) -> Self {
        let (width, height) = kind.size();
        let source = NativeVideoSource::new(VideoResolution { width, height }, screencast);
        let track = factory.create_video_track(label, source.clone());
        Outgoing { source, track, capture: None }
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
}

impl LocalVideo {
    pub fn new(factory: &PeerConnectionFactory, mode: VideoMode, hub: Arc<VideoHub>, updates: UnboundedSender<CallUpdate>) -> Self {
        LocalVideo {
            mode,
            hub,
            updates,
            camera: Outgoing::new(factory, CAMERA_LABEL, PatternKind::Camera, false),
            screen: Outgoing::new(factory, SCREEN_LABEL, PatternKind::Screen, true),
            lines: None,
            blur: Arc::new(BlurSettings::default()),
        }
    }

    pub fn set_blur(&self, enabled: bool) {
        self.blur.set_enabled(enabled);
    }

    pub fn blur_average_ms(&self) -> Option<f32> {
        (self.camera_on() && self.blur.enabled()).then(|| self.blur.average_ms())
    }

    pub fn is_available(&self) -> bool {
        self.mode != VideoMode::Off
    }

    pub fn camera_on(&self) -> bool {
        self.camera.capture.is_some()
    }

    pub fn sharing(&self) -> bool {
        self.screen.capture.is_some()
    }

    pub fn set_lines(&mut self, lines: VideoLines) -> Result<()> {
        if self.camera_on() {
            Self::begin_sending(&lines.camera.sender(), &self.camera.track)?;
        }
        if self.sharing() {
            Self::begin_sending(&lines.share.sender(), &self.screen.track)?;
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
        sender.set_parameters(parameters).map_err(|error| Error::Webrtc(error.to_string()))
    }

    fn sink(&self, outgoing: &Outgoing, key: VideoKey) -> LocalSink {
        LocalSink::new(outgoing.source.clone(), Some((self.hub.clone(), key)))
    }

    fn report_failure(&self, what: &'static str) -> impl FnOnce(String) + Send + 'static {
        let updates = self.updates.clone();
        move |reason| {
            let _ = updates.send(CallUpdate::Notice(format!("{what} stopped: {reason}")));
        }
    }

    pub fn start_camera(&mut self, choice: &DeviceChoice) -> Result<()> {
        let lines = self.lines.as_ref().ok_or_else(|| Error::Webrtc("no video line negotiated".into()))?;
        if !self.is_available() {
            return Err(Error::Webrtc("video is switched off".into()));
        }
        self.camera.capture = None;
        let mut sink = self.sink(&self.camera, VideoKey::LocalCamera);
        let capture = match self.mode {
            VideoMode::Pattern => Capture::Task(spawn_pattern(sink, PatternKind::Camera, Some(BlurStage::new(self.blur.clone())))),
            _ => start_camera(choice, self.blur.clone(), move |buffer| sink.push(buffer), self.report_failure("Camera"))?,
        };
        Self::begin_sending(&lines.camera.sender(), &self.camera.track)?;
        self.camera.capture = Some(capture);
        Ok(())
    }

    pub fn stop_camera(&mut self) {
        self.camera.capture = None;
        if let Some(lines) = &self.lines {
            let _ = Self::set_active(&lines.camera.sender(), false);
        }
        self.hub.forget(&VideoKey::LocalCamera);
    }

    pub fn start_share(&mut self, source: &ShareSource) -> Result<()> {
        let lines = self.lines.as_ref().ok_or_else(|| Error::Webrtc("no video line negotiated".into()))?;
        if !self.is_available() {
            return Err(Error::Webrtc("video is switched off".into()));
        }
        self.screen.capture = None;
        let mut sink = self.sink(&self.screen, VideoKey::LocalScreen);
        let capture = match self.mode {
            VideoMode::Pattern => Capture::Task(spawn_pattern(sink, PatternKind::Screen, None)),
            _ => start_screen_capture(source.clone(), move |buffer| sink.push(buffer), self.report_failure("Screen share"))?,
        };
        Self::begin_sending(&lines.share.sender(), &self.screen.track)?;
        self.screen.capture = Some(capture);
        Ok(())
    }

    pub fn stop_share(&mut self) {
        self.screen.capture = None;
        if let Some(lines) = &self.lines {
            let _ = Self::set_active(&lines.share.sender(), false);
        }
        self.hub.forget(&VideoKey::LocalScreen);
    }

    pub fn stop_all(&mut self) {
        self.stop_camera();
        self.stop_share();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_mode_follows_the_environment_value() {
        assert_eq!(VideoMode::parse(None), VideoMode::Platform);
        assert_eq!(VideoMode::parse(Some("Pattern")), VideoMode::Pattern);
        assert_eq!(VideoMode::parse(Some("off")), VideoMode::Off);
        assert_eq!(VideoMode::parse(Some("whatever")), VideoMode::Platform);
    }
}
