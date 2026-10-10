use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use libwebrtc::video_frame::{BoxVideoBuffer, BoxVideoFrame, I420Buffer, VideoBuffer, VideoFormatType};
use tokio::sync::mpsc::UnboundedSender;

use crate::control::CallUpdate;

pub const PERSON_FPS: u32 = 30;
pub const SCREEN_FPS: u32 = 15;
pub const PERSON_MAX_WIDTH: u32 = 640;
pub const SCREEN_MAX_WIDTH: u32 = 1600;
const BYTES_PER_PIXEL: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VideoKey {
    Person(String),
    Screen,
    LocalCamera,
    LocalScreen,
}

impl VideoKey {
    pub fn max_width(&self) -> u32 {
        match self {
            VideoKey::Screen | VideoKey::LocalScreen => SCREEN_MAX_WIDTH,
            VideoKey::Person(_) | VideoKey::LocalCamera => PERSON_MAX_WIDTH,
        }
    }

    pub fn max_fps(&self) -> u32 {
        match self {
            VideoKey::Screen | VideoKey::LocalScreen => SCREEN_FPS,
            VideoKey::Person(_) | VideoKey::LocalCamera => PERSON_FPS,
        }
    }
}

/// BGRA, tightly packed, the byte order the GPU atlas of the UI expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPicture {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl VideoPicture {
    pub fn solid(width: u32, height: u32, blue: u8, green: u8, red: u8) -> Self {
        let pixel = [blue, green, red, u8::MAX];
        VideoPicture {
            width,
            height,
            bgra: pixel.iter().copied().cycle().take(width as usize * height as usize * BYTES_PER_PIXEL).collect(),
        }
    }
}

pub struct VideoHub {
    frames: Mutex<HashMap<VideoKey, Arc<VideoPicture>>>,
    pending: AtomicBool,
    notify: UnboundedSender<CallUpdate>,
}

impl VideoHub {
    pub fn new(notify: UnboundedSender<CallUpdate>) -> Arc<Self> {
        Arc::new(VideoHub {
            frames: Mutex::default(),
            pending: AtomicBool::new(false),
            notify,
        })
    }

    pub fn publish(&self, key: VideoKey, picture: VideoPicture) {
        self.frames.lock().expect("video frames lock").insert(key, Arc::new(picture));
        if !self.pending.swap(true, Ordering::SeqCst) {
            let _ = self.notify.send(CallUpdate::VideoReady);
        }
    }

    pub fn drain(&self) -> Vec<(VideoKey, Arc<VideoPicture>)> {
        self.pending.store(false, Ordering::SeqCst);
        self.frames.lock().expect("video frames lock").drain().collect()
    }

    pub fn forget(&self, key: &VideoKey) {
        self.frames.lock().expect("video frames lock").remove(key);
    }
}

const GATE_SLACK: f64 = 0.8;

pub struct FrameGate {
    interval: Duration,
    last: Option<Instant>,
}

impl FrameGate {
    pub fn new(frames_per_second: u32) -> Self {
        FrameGate {
            interval: Duration::from_secs(1).mul_f64(GATE_SLACK) / frames_per_second.max(1),
            last: None,
        }
    }

    pub fn admit(&mut self, now: Instant) -> bool {
        let due = self.last.is_none_or(|last| now.duration_since(last) >= self.interval);
        if due {
            self.last = Some(now);
        }
        due
    }
}

pub fn fit_width(width: u32, height: u32, max_width: u32) -> (u32, u32) {
    if width <= max_width || width == 0 {
        return (width & !1, height & !1);
    }
    let scaled_height = (u64::from(height) * u64::from(max_width) / u64::from(width)) as u32;
    (max_width & !1, scaled_height.max(2) & !1)
}

pub fn picture_from_frame(frame: BoxVideoFrame, max_width: u32) -> Option<VideoPicture> {
    let buffer: BoxVideoBuffer = frame.buffer;
    picture_from_i420(buffer.to_i420(), max_width)
}

pub fn picture_from_i420(mut i420: I420Buffer, max_width: u32) -> Option<VideoPicture> {
    let (width, height) = (i420.width(), i420.height());
    if width < 2 || height < 2 {
        return None;
    }
    let (target_width, target_height) = fit_width(width, height, max_width);
    if (target_width, target_height) != (width, height) {
        i420 = i420.scale(target_width as i32, target_height as i32);
    }
    Some(bgra_from_i420(i420))
}

pub fn bgra_from_i420(i420: I420Buffer) -> VideoPicture {
    let (width, height) = (i420.width(), i420.height());
    let stride = width as usize * BYTES_PER_PIXEL;
    let mut bgra = vec![0u8; stride * height as usize];
    let buffer: BoxVideoBuffer = Box::new(i420);
    buffer.to_argb(VideoFormatType::ARGB, &mut bgra, stride as u32, width as i32, height as i32);
    VideoPicture { width, height, bgra }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_caps_a_fast_source_at_thirty_frames_a_second() {
        let mut gate = FrameGate::new(30);
        let start = Instant::now();
        let admitted = (0..120)
            .filter(|tick| gate.admit(start + Duration::from_millis(tick * 8)))
            .count();
        assert!((28..=32).contains(&admitted), "{admitted} frames in a second");
    }

    #[test]
    fn the_gate_keeps_every_frame_of_a_steady_thirty_fps_source() {
        let mut gate = FrameGate::new(30);
        let start = Instant::now();
        let admitted = (0..30)
            .filter(|tick| gate.admit(start + Duration::from_micros(tick * 33_000)))
            .count();
        assert_eq!(admitted, 30);
    }

    #[test]
    fn wide_frames_shrink_to_the_cap_with_even_sides() {
        assert_eq!(fit_width(1920, 1080, 640), (640, 360));
        assert_eq!(fit_width(1280, 720, 640), (640, 360));
        assert_eq!(fit_width(321, 241, 640), (320, 240));
        assert_eq!(fit_width(1000, 1001, 500), (500, 500));
    }

    #[test]
    fn the_hub_notifies_once_until_it_is_drained() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let hub = VideoHub::new(sender);
        hub.publish(VideoKey::Screen, VideoPicture::solid(2, 2, 0, 0, 0));
        hub.publish(VideoKey::LocalCamera, VideoPicture::solid(2, 2, 0, 0, 0));
        hub.publish(VideoKey::Screen, VideoPicture::solid(4, 4, 0, 0, 0));
        assert!(matches!(receiver.try_recv(), Ok(CallUpdate::VideoReady)));
        assert!(receiver.try_recv().is_err());
        let drained = hub.drain();
        assert_eq!(drained.len(), 2);
        assert!(drained.iter().any(|(key, picture)| *key == VideoKey::Screen && picture.width == 4));
        hub.publish(VideoKey::Screen, VideoPicture::solid(2, 2, 0, 0, 0));
        assert!(matches!(receiver.try_recv(), Ok(CallUpdate::VideoReady)));
    }

    #[test]
    fn a_red_i420_frame_becomes_bgra_red() {
        let mut buffer = I420Buffer::new(4, 4);
        let (y_plane, u_plane, v_plane) = buffer.data_mut();
        y_plane.fill(82);
        u_plane.fill(90);
        v_plane.fill(240);
        let picture = bgra_from_i420(buffer);
        let [blue, green, red, alpha] = [picture.bgra[0], picture.bgra[1], picture.bgra[2], picture.bgra[3]];
        assert!(red > 200 && green < 60 && blue < 60 && alpha == 255, "{blue} {green} {red} {alpha}");
    }
}
