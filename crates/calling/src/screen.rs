use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use libwebrtc::desktop_capturer::{CaptureSource, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions};
use libwebrtc::native::yuv_helper::argb_to_i420;
use libwebrtc::video_frame::I420Buffer;

use crate::capture::Capture;
use crate::video_frame::VideoPicture;
use crate::error::{Error, Result};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareKind {
    Screen,
    Window,
}

impl ShareKind {
    fn source_type(self) -> DesktopCaptureSourceType {
        match self {
            ShareKind::Screen => DesktopCaptureSourceType::Screen,
            ShareKind::Window => DesktopCaptureSourceType::Window,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareSource {
    pub id: u64,
    pub kind: ShareKind,
    pub title: String,
}

impl ShareSource {
    pub fn label(&self) -> String {
        match (self.kind, self.title.trim()) {
            (ShareKind::Screen, "") => "Entire screen".to_owned(),
            (_, "") => "Window".to_owned(),
            (_, title) => title.to_owned(),
        }
    }
}

fn capturer(kind: ShareKind) -> Option<DesktopCapturer> {
    let mut options = DesktopCapturerOptions::new(kind.source_type());
    options.set_include_cursor(true);
    DesktopCapturer::new(options)
}

fn sources_of(kind: ShareKind) -> Vec<(ShareSource, CaptureSource)> {
    let Some(capturer) = capturer(kind) else { return Vec::new() };
    capturer
        .get_source_list()
        .into_iter()
        .map(|source| {
            (
                ShareSource {
                    id: source.id(),
                    kind,
                    title: source.title(),
                },
                source,
            )
        })
        .collect()
}

pub fn list_share_sources() -> Vec<ShareSource> {
    [ShareKind::Screen, ShareKind::Window]
        .into_iter()
        .flat_map(|kind| sources_of(kind).into_iter().map(|(source, _)| source))
        .collect()
}

const THUMBNAIL_ATTEMPTS: u32 = 5;
const THUMBNAIL_RETRY: Duration = Duration::from_millis(40);

pub fn share_thumbnail(wanted: &ShareSource, max_width: u32, max_height: u32) -> Option<VideoPicture> {
    let source = sources_of(wanted.kind).into_iter().find(|(source, _)| source.id == wanted.id).map(|(_, raw)| raw)?;
    let mut capturer = capturer(wanted.kind)?;
    let captured: Arc<Mutex<Option<VideoPicture>>> = Arc::default();
    let sink = captured.clone();
    capturer.start_capture(Some(source), move |result| {
        let Ok(frame) = result else { return };
        let (width, height) = (frame.width().max(0) as u32, frame.height().max(0) as u32);
        if let Some(picture) = scaled_bgra(frame.data(), width, height, frame.stride(), max_width, max_height)
            && let Ok(mut slot) = sink.lock()
        {
            *slot = Some(picture);
        }
    });
    for _ in 0..THUMBNAIL_ATTEMPTS {
        pump_messages();
        capturer.capture_frame();
        if let Some(picture) = captured.lock().ok().and_then(|mut slot| slot.take()) {
            return Some(picture);
        }
        std::thread::sleep(THUMBNAIL_RETRY);
    }
    None
}

// Windows Graphics Capture hands frames to a DispatcherQueue on this thread; without a pump none arrive.
#[cfg(windows)]
fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};
    let mut message = MSG::default();
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            let _ = DispatchMessageW(&message);
        }
    }
}

#[cfg(not(windows))]
fn pump_messages() {}

fn scaled_bgra(bytes: &[u8], width: u32, height: u32, stride: u32, max_width: u32, max_height: u32) -> Option<VideoPicture> {
    if width == 0 || height == 0 || (stride as usize) < width as usize * 4 || bytes.len() < stride as usize * height as usize {
        return None;
    }
    let scale = (max_width as f32 / width as f32).min(max_height as f32 / height as f32).min(1.);
    let (target_width, target_height) = (((width as f32 * scale) as u32).max(1), ((height as f32 * scale) as u32).max(1));
    let mut bgra = Vec::with_capacity(target_width as usize * target_height as usize * 4);
    for row in 0..target_height {
        let source_row = (row as u64 * height as u64 / target_height as u64) as usize;
        for column in 0..target_width {
            let source_column = (column as u64 * width as u64 / target_width as u64) as usize;
            let offset = source_row * stride as usize + source_column * 4;
            bgra.extend_from_slice(&[bytes[offset], bytes[offset + 1], bytes[offset + 2], u8::MAX]);
        }
    }
    Some(VideoPicture { width: target_width, height: target_height, bgra })
}

fn i420_from_bgra(bytes: &[u8], width: u32, height: u32, stride: u32) -> I420Buffer {
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    argb_to_i420(bytes, stride, plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, width as i32, height as i32);
    buffer
}

pub fn start_screen_capture(
    wanted: ShareSource,
    fps: Arc<AtomicU32>,
    mut deliver: impl FnMut(I420Buffer) + Send + 'static,
    failed: impl FnOnce(String) + Send + 'static,
) -> Result<Capture> {
    Capture::spawn_thread("screen-capture", move |stop| {
        let Some(source) = sources_of(wanted.kind).into_iter().find(|(source, _)| source.id == wanted.id).map(|(_, raw)| raw) else {
            return failed("the shared source is gone".to_owned());
        };
        let Some(mut capturer) = capturer(wanted.kind) else {
            return failed("screen capture is not available".to_owned());
        };
        capturer.start_capture(Some(source), move |result| {
            let Ok(frame) = result else { return };
            let (width, height) = (frame.width() as u32 & !1, frame.height() as u32 & !1);
            if width >= 2 && height >= 2 {
                deliver(i420_from_bgra(frame.data(), width, height, frame.stride()));
            }
        });
        while !stop.load(Ordering::SeqCst) {
            pump_messages();
            capturer.capture_frame();
            std::thread::sleep(Duration::from_secs(1) / fps.load(Ordering::Relaxed).max(1));
        }
    })
    .map_err(|error| Error::Webrtc(format!("screen thread: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_get_readable_labels() {
        let screen = ShareSource { id: 1, kind: ShareKind::Screen, title: String::new() };
        assert_eq!(screen.label(), "Entire screen");
        let window = ShareSource { id: 2, kind: ShareKind::Window, title: " Notes ".into() };
        assert_eq!(window.label(), "Notes");
    }
}

#[cfg(test)]
mod thumbnail_tests {
    use super::scaled_bgra;

    #[test]
    fn a_thumbnail_keeps_the_aspect_and_drops_the_row_padding() {
        let (width, height, stride) = (8u32, 4u32, 40u32);
        let mut bytes = vec![0u8; (stride * height) as usize];
        for row in 0..height as usize {
            for column in 0..width as usize {
                let offset = row * stride as usize + column * 4;
                bytes[offset..offset + 4].copy_from_slice(&[column as u8, row as u8, 7, 0]);
            }
        }
        let picture = scaled_bgra(&bytes, width, height, stride, 4, 4).unwrap();
        assert_eq!((picture.width, picture.height), (4, 2));
        assert_eq!(picture.bgra.len(), 4 * 2 * 4);
        assert_eq!(&picture.bgra[4..8], &[2, 0, 7, 255]);
        assert!(scaled_bgra(&bytes, width, height, 8, 4, 4).is_none());
    }
}
