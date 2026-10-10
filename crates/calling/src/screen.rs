use std::sync::atomic::Ordering;
use std::time::Duration;

use libwebrtc::desktop_capturer::{CaptureSource, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions};
use libwebrtc::native::yuv_helper::argb_to_i420;
use libwebrtc::video_frame::I420Buffer;

use crate::capture::Capture;
use crate::error::{Error, Result};
use crate::video_pattern::PatternKind;


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

fn i420_from_bgra(bytes: &[u8], width: u32, height: u32, stride: u32) -> I420Buffer {
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    argb_to_i420(bytes, stride, plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, width as i32, height as i32);
    buffer
}

pub fn start_screen_capture(
    wanted: ShareSource,
    mut deliver: impl FnMut(I420Buffer) + Send + 'static,
    failed: impl FnOnce(String) + Send + 'static,
) -> Result<Capture> {
    let period = Duration::from_secs(1) / PatternKind::Screen.fps();
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
            capturer.capture_frame();
            std::thread::sleep(period);
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
