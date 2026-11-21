use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use libwebrtc::desktop_capturer::{
    CaptureError, CaptureSource, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
    PortalPickerEvent,
};
use libwebrtc::native::yuv_helper::{argb_scale, argb_to_i420};
use libwebrtc::video_frame::I420Buffer;

use crate::capture::Capture;
use crate::error::{Error, Result};
use crate::send_control::scale_for;
use crate::video_frame::VideoPicture;

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

/// How often and how large the capture threads deliver share frames. Both change while sharing.
#[derive(Debug)]
pub struct ShareRate {
    pub fps: AtomicU32,
    /// The mixer's frame budget in macroblocks. 0 keeps the captured size.
    pub max_fs: AtomicU32,
}

impl ShareRate {
    pub fn new(fps: u32) -> Arc<Self> {
        Arc::new(ShareRate {
            fps: AtomicU32::new(fps),
            max_fs: AtomicU32::new(0),
        })
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(1) / self.fps.load(Ordering::Relaxed).max(1)
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
    let Some(capturer) = capturer(kind) else {
        return Vec::new();
    };
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
const SOURCE_GONE_AFTER: Duration = Duration::from_secs(3);
const THUMBNAIL_RETRY: Duration = Duration::from_millis(40);

pub fn share_thumbnail(
    wanted: &ShareSource,
    max_width: u32,
    max_height: u32,
) -> Option<VideoPicture> {
    let source = sources_of(wanted.kind)
        .into_iter()
        .find(|(source, _)| source.id == wanted.id)
        .map(|(_, raw)| raw)?;
    let mut capturer = capturer(wanted.kind)?;
    let captured: Arc<Mutex<Option<VideoPicture>>> = Arc::default();
    let sink = captured.clone();
    capturer.start_capture(Some(source), move |result| {
        let Ok(frame) = result else { return };
        let (width, height) = (frame.width().max(0) as u32, frame.height().max(0) as u32);
        if let Some(picture) = scaled_bgra(
            frame.data(),
            width,
            height,
            frame.stride(),
            max_width,
            max_height,
        ) && let Ok(mut slot) = sink.lock()
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
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
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

fn scaled_bgra(
    bytes: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    max_width: u32,
    max_height: u32,
) -> Option<VideoPicture> {
    if width == 0
        || height == 0
        || (stride as usize) < width as usize * 4
        || bytes.len() < stride as usize * height as usize
    {
        return None;
    }
    let scale = (max_width as f32 / width as f32)
        .min(max_height as f32 / height as f32)
        .min(1.);
    let (target_width, target_height) = (
        ((width as f32 * scale) as u32).max(1),
        ((height as f32 * scale) as u32).max(1),
    );
    let mut bgra = Vec::with_capacity(target_width as usize * target_height as usize * 4);
    for row in 0..target_height {
        let source_row = (row as u64 * height as u64 / target_height as u64) as usize;
        for column in 0..target_width {
            let source_column = (column as u64 * width as u64 / target_width as u64) as usize;
            let offset = source_row * stride as usize + source_column * 4;
            bgra.extend_from_slice(&[bytes[offset], bytes[offset + 1], bytes[offset + 2], u8::MAX]);
        }
    }
    Some(VideoPicture {
        width: target_width,
        height: target_height,
        bgra,
    })
}

fn i420_from_bgra(bytes: &[u8], width: u32, height: u32, stride: u32) -> I420Buffer {
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    argb_to_i420(
        bytes,
        stride,
        plane_y,
        stride_y,
        plane_u,
        stride_u,
        plane_v,
        stride_v,
        width as i32,
        height as i32,
    );
    buffer
}

/// The even size within `max_fs` macroblocks that keeps the aspect ratio.
fn share_size(width: u32, height: u32, max_fs: u32) -> (u32, u32) {
    let scale = scale_for(width, height, max_fs);
    if scale <= 1.0 {
        return (width, height);
    }
    let fit = |side: u32| ((f64::from(side) / scale) as u32 & !1).max(2);
    (fit(width), fit(height))
}

fn scale_bgra(bytes: &[u8], stride: u32, from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
    let mut scaled = vec![0u8; to.0 as usize * to.1 as usize * 4];
    argb_scale(
        bytes,
        stride,
        from.0 as i32,
        from.1 as i32,
        &mut scaled,
        to.0 * 4,
        to.0 as i32,
        to.1 as i32,
    );
    scaled
}

/// Converts a captured frame to I420 within the mixer's frame budget. Scaling
/// here keeps WebRTC from converting and scaling the full-size frame on the CPU
/// with its slow path for uneven factors. Exact halvings average 2x2 blocks, so
/// text stays readable, and a bilinear step covers the rest.
fn share_frame(bytes: &[u8], width: u32, height: u32, stride: u32, max_fs: u32) -> I420Buffer {
    let target = share_size(width, height, max_fs);
    let (mut size, mut stride) = ((width, height), stride);
    let mut scaled: Option<Vec<u8>> = None;
    while size != target {
        let next = if size.0 >= target.0 * 2 && size.1 >= target.1 * 2 {
            (size.0 / 2, size.1 / 2)
        } else {
            target
        };
        scaled = Some(scale_bgra(
            scaled.as_deref().unwrap_or(bytes),
            stride,
            size,
            next,
        ));
        (size, stride) = (next, next.0 * 4);
    }
    i420_from_bgra(scaled.as_deref().unwrap_or(bytes), size.0, size.1, stride)
}

pub fn start_screen_capture(
    wanted: ShareSource,
    rate: Arc<ShareRate>,
    mut deliver: impl FnMut(I420Buffer) + Send + 'static,
    failed: impl FnOnce(String) + Send + 'static,
) -> Result<Capture> {
    if current_share_picker() == SharePicker::Portal {
        return start_portal_capture(wanted.kind, rate.clone(), deliver, move |follow| {
            if let PortalFollowUp::Notice(text) = follow {
                failed(text.to_owned());
            }
        });
    }
    Capture::spawn_thread("screen-capture", move |stop| {
        let Some(source) = sources_of(wanted.kind)
            .into_iter()
            .find(|(source, _)| source.id == wanted.id)
            .map(|(_, raw)| raw)
        else {
            return failed("the shared source is gone".to_owned());
        };
        let Some(mut capturer) = capturer(wanted.kind) else {
            return failed("screen capture is not available".to_owned());
        };
        let gone_since: Arc<Mutex<Option<Instant>>> = Arc::default();
        let gone_mark = gone_since.clone();
        let frame_rate = rate.clone();
        capturer.start_capture(Some(source), move |result| {
            let frame = match result {
                Ok(frame) => frame,
                Err(error) => {
                    if matches!(error, CaptureError::Permanent)
                        && let Ok(mut since) = gone_mark.lock()
                    {
                        since.get_or_insert_with(Instant::now);
                    }
                    return;
                }
            };
            if let Ok(mut since) = gone_mark.lock() {
                *since = None;
            }
            let (width, height) = (frame.width() as u32 & !1, frame.height() as u32 & !1);
            if width >= 2 && height >= 2 {
                let max_fs = frame_rate.max_fs.load(Ordering::Relaxed);
                deliver(share_frame(
                    frame.data(),
                    width,
                    height,
                    frame.stride(),
                    max_fs,
                ));
            }
        });
        while !stop.load(Ordering::SeqCst) {
            let gone = gone_since
                .lock()
                .is_ok_and(|since| since.is_some_and(|since| since.elapsed() >= SOURCE_GONE_AFTER));
            if gone {
                return failed("the shared source is gone".to_owned());
            }
            pump_messages();
            capturer.capture_frame();
            std::thread::sleep(rate.interval());
        }
    })
    .map_err(|error| Error::Webrtc(format!("screen thread: {error}")))
}

/// Where the share menu gets its sources. Wayland and macOS use a system dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePicker {
    List,
    Portal,
}

pub fn share_picker_for(wayland_display: Option<&str>) -> SharePicker {
    share_picker_for_host(cfg!(target_os = "macos"), wayland_display)
}

pub fn share_picker_for_host(macos: bool, wayland_display: Option<&str>) -> SharePicker {
    if macos {
        return SharePicker::Portal;
    }
    match wayland_display {
        Some(value) if !value.is_empty() => SharePicker::Portal,
        _ => SharePicker::List,
    }
}

pub fn current_share_picker() -> SharePicker {
    share_picker_for(std::env::var("WAYLAND_DISPLAY").ok().as_deref())
}

/// Portal menus must not enumerate or thumbnail sources. That would open the dialog early.
pub fn share_sources_for_menu(picker: SharePicker, sources: Vec<ShareSource>) -> Vec<ShareSource> {
    match picker {
        SharePicker::Portal => Vec::new(),
        SharePicker::List => sources,
    }
}

pub use crate::notices::PORTAL_UNAVAILABLE_NOTICE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalFollowUp {
    Share(ShareSource),
    Quiet,
    Notice(&'static str),
}

pub fn portal_follow_up(event: PortalPickerEvent, source: Option<ShareSource>) -> PortalFollowUp {
    match event {
        PortalPickerEvent::Selected => match source {
            Some(source) => PortalFollowUp::Share(source),
            None => PortalFollowUp::Notice(PORTAL_UNAVAILABLE_NOTICE),
        },
        PortalPickerEvent::Cancelled => PortalFollowUp::Quiet,
        PortalPickerEvent::Failed => PortalFollowUp::Notice(PORTAL_UNAVAILABLE_NOTICE),
    }
}

/// What a frame result says about a system picker without a delegated
/// controller (macOS). Frames start once a source is picked. A cancel, or a
/// picker that fails to start, turns into a permanent error.
fn picker_event_from_frame<T>(
    result: &std::result::Result<T, CaptureError>,
) -> Option<PortalPickerEvent> {
    match result {
        Ok(_) => Some(PortalPickerEvent::Selected),
        Err(CaptureError::Permanent) => Some(PortalPickerEvent::Cancelled),
        Err(CaptureError::Temporary) => None,
    }
}

pub(crate) fn start_portal_capture(
    kind: ShareKind,
    rate: Arc<ShareRate>,
    mut deliver: impl FnMut(I420Buffer) + Send + 'static,
    outcome: impl FnOnce(PortalFollowUp) + Send + 'static,
) -> Result<Capture> {
    Capture::spawn_thread("portal-share", move |stop| {
        libwebrtc::desktop_capturer::ensure_shared_glib_loop();
        let Some(mut capturer) = capturer(kind) else {
            outcome(PortalFollowUp::Notice(PORTAL_UNAVAILABLE_NOTICE));
            return;
        };
        // The Wayland portal reports the pick through a delegated picker. The
        // macOS system picker has none: it opens in `start_capture` and the
        // outcome shows in the frame results.
        let delegated = capturer.has_delegated_picker();
        if !delegated && !cfg!(target_os = "macos") {
            outcome(PortalFollowUp::Notice(PORTAL_UNAVAILABLE_NOTICE));
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let frame_events = (!delegated).then(|| sender.clone());
        let selected = Arc::new(AtomicBool::new(false));
        let gate = selected.clone();
        let frame_rate = rate.clone();
        capturer.start_capture(None, move |result| {
            if !gate.load(Ordering::SeqCst) {
                let Some(events) = &frame_events else { return };
                let Some(event) = picker_event_from_frame(&result) else {
                    return;
                };
                let _ = events.send(event);
                if event != PortalPickerEvent::Selected {
                    return;
                }
                gate.store(true, Ordering::SeqCst);
            }
            let Ok(frame) = result else { return };
            let (width, height) = (frame.width() as u32 & !1, frame.height() as u32 & !1);
            if width >= 2 && height >= 2 {
                let max_fs = frame_rate.max_fs.load(Ordering::Relaxed);
                deliver(share_frame(
                    frame.data(),
                    width,
                    height,
                    frame.stride(),
                    max_fs,
                ));
            }
        });
        if delegated {
            capturer.watch_picker(move |event| {
                let _ = sender.send(event);
            });
            capturer.ensure_picker_visible();
        }
        let event = loop {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if !delegated {
                capturer.capture_frame();
            }
            match receiver.recv_timeout(Duration::from_millis(40)) {
                Ok(event) => break event,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    outcome(PortalFollowUp::Notice(PORTAL_UNAVAILABLE_NOTICE));
                    return;
                }
            }
        };
        if stop.load(Ordering::SeqCst) {
            return;
        }
        // The portal already bound this stream. Looking the source up again, or
        // selecting that id, is what used to tear the stream down and report
        // "the shared source is gone".
        let source = if event == PortalPickerEvent::Selected {
            selected.store(true, Ordering::SeqCst);
            Some(ShareSource {
                id: 0,
                kind,
                title: String::new(),
            })
        } else {
            None
        };
        let follow = portal_follow_up(event, source);
        if !matches!(follow, PortalFollowUp::Share(_)) {
            outcome(follow);
            return;
        }
        capturer.capture_frame();
        outcome(follow);
        while !stop.load(Ordering::SeqCst) {
            capturer.capture_frame();
            std::thread::sleep(rate.interval());
        }
    })
    .map_err(|error| Error::Webrtc(format!("portal thread: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_get_readable_labels() {
        let screen = ShareSource {
            id: 1,
            kind: ShareKind::Screen,
            title: String::new(),
        };
        assert_eq!(screen.label(), "Entire screen");
        let window = ShareSource {
            id: 2,
            kind: ShareKind::Window,
            title: " Notes ".into(),
        };
        assert_eq!(window.label(), "Notes");
    }

    #[test]
    fn frames_report_the_macos_picker_outcome() {
        assert_eq!(
            picker_event_from_frame(&Ok(())),
            Some(PortalPickerEvent::Selected)
        );
        assert_eq!(
            picker_event_from_frame::<()>(&Err(CaptureError::Permanent)),
            Some(PortalPickerEvent::Cancelled)
        );
        assert_eq!(
            picker_event_from_frame::<()>(&Err(CaptureError::Temporary)),
            None
        );
    }
}

#[cfg(test)]
mod share_frame_tests {
    use libwebrtc::video_frame::VideoBuffer as _;

    use super::{share_frame, share_size};
    use crate::send_control::scale_for;

    #[test]
    fn a_share_frame_fits_the_mixer_budget_and_keeps_the_aspect() {
        assert_eq!(share_size(5120, 2880, 0), (5120, 2880));
        assert_eq!(share_size(1280, 720, 8160), (1280, 720));
        let (width, height) = share_size(5120, 2880, 8160);
        assert_eq!((width % 2, height % 2), (0, 0));
        assert_eq!(scale_for(width, height, 8160), 1.0);
        assert!((f64::from(width) / f64::from(height) - 16.0 / 9.0).abs() < 0.01);
        assert!(width >= 1900);
    }

    #[test]
    fn a_padded_frame_is_scaled_to_the_budget() {
        let (width, height, stride) = (1600u32, 900u32, 1600 * 4 + 64);
        let bytes = vec![128u8; (stride * height) as usize];
        let frame = share_frame(&bytes, width, height, stride, 920);
        assert_eq!(
            (frame.width(), frame.height()),
            share_size(width, height, 920)
        );
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

#[cfg(test)]
mod portal_mode_tests {
    use super::{
        ShareKind, SharePicker, ShareSource, share_picker_for_host, share_sources_for_menu,
    };

    #[test]
    fn portal_mode_follows_wayland_display_and_skips_the_menu_list() {
        assert_eq!(share_picker_for_host(false, None), SharePicker::List);
        assert_eq!(share_picker_for_host(false, Some("")), SharePicker::List);
        assert_eq!(
            share_picker_for_host(false, Some("wayland-1")),
            SharePicker::Portal
        );
        assert_eq!(share_picker_for_host(true, None), SharePicker::Portal);
        assert_eq!(share_picker_for_host(true, Some("")), SharePicker::Portal);
        let sources = vec![ShareSource {
            id: 1,
            kind: ShareKind::Screen,
            title: "Desk".into(),
        }];
        assert!(share_sources_for_menu(SharePicker::Portal, sources.clone()).is_empty());
        assert_eq!(
            share_sources_for_menu(SharePicker::List, sources.clone()),
            sources
        );
    }
}

#[cfg(test)]
mod portal_share_tests {
    use libwebrtc::desktop_capturer::PortalPickerEvent;

    use super::{
        PORTAL_UNAVAILABLE_NOTICE, PortalFollowUp, ShareKind, ShareSource, portal_follow_up,
    };

    #[test]
    fn portal_share_cancel_is_quiet_and_failure_is_a_notice() {
        let source = ShareSource {
            id: 7,
            kind: ShareKind::Screen,
            title: "Desk".into(),
        };
        assert!(matches!(
            portal_follow_up(PortalPickerEvent::Selected, Some(source.clone())),
            PortalFollowUp::Share(chosen) if chosen == source
        ));
        assert!(matches!(
            portal_follow_up(PortalPickerEvent::Selected, None),
            PortalFollowUp::Notice(PORTAL_UNAVAILABLE_NOTICE)
        ));
        assert!(matches!(
            portal_follow_up(PortalPickerEvent::Cancelled, Some(source.clone())),
            PortalFollowUp::Quiet
        ));
        assert!(matches!(
            portal_follow_up(PortalPickerEvent::Failed, Some(source)),
            PortalFollowUp::Notice(PORTAL_UNAVAILABLE_NOTICE)
        ));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod portal_loop_tests {
    #[test]
    fn portal_loop_stays_up_across_ensures() {
        libwebrtc::desktop_capturer::ensure_shared_glib_loop();
        let id = libwebrtc::desktop_capturer::shared_glib_loop_id();
        assert_ne!(id, 0);
        libwebrtc::desktop_capturer::ensure_shared_glib_loop();
        assert_eq!(id, libwebrtc::desktop_capturer::shared_glib_loop_id());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod portal_smoke {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::{PortalFollowUp, ShareKind, ShareRate, start_portal_capture};

    #[test]
    fn portal_smoke_captures_after_the_dialog() {
        if std::env::var("PORTAL_SMOKE").ok().as_deref() != Some("1") {
            return;
        }
        let frames = Arc::new(AtomicU32::new(0));
        let counter = frames.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let capture = start_portal_capture(
            ShareKind::Screen,
            ShareRate::new(5),
            move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            },
            move |follow| {
                eprintln!("PORTAL_FOLLOW={follow:?}");
                let _ = sender.send(follow);
            },
        )
        .expect("portal thread");
        let follow = receiver
            .recv_timeout(Duration::from_secs(40))
            .expect("no portal result in 40s");
        std::thread::sleep(Duration::from_millis(700));
        let count = frames.load(Ordering::Relaxed);
        eprintln!("PORTAL_FRAMES={count}");
        drop(capture);
        match follow {
            PortalFollowUp::Share(_) => {
                assert!(count > 0, "the dialog confirmed, but no frame arrived")
            }
            PortalFollowUp::Quiet => panic!("the dialog was cancelled"),
            PortalFollowUp::Notice(text) => panic!("portal notice: {text}"),
        }
    }
}
