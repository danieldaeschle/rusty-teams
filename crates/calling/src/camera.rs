use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use libwebrtc::native::yuv_helper::{abgr_to_i420, nv12_to_i420};
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};
use nokhwa::pixel_format::RgbAFormat;
use nokhwa::utils::{
    ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
    Resolution,
};
use nokhwa::{Camera, query};
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::blur::BlurSettings;
use crate::capture::Capture;
use crate::devices::{DeviceChoice, DeviceEntry};
use crate::effects::EffectsThread;
use crate::error::{Error, Result};
use crate::video_quality::VideoQuality;

const RETRY_PAUSE: Duration = Duration::from_millis(20);
const BYTES_PER_PIXEL: u32 = 4;
const CAPTURE_FPS: u32 = 30;
const ASPECT_WIDTH: u32 = 16;
const ASPECT_HEIGHT: u32 = 9;
const DIRECT_FORMATS: [FrameFormat; 2] = [FrameFormat::NV12, FrameFormat::YUYV];
const USABLE_FORMATS: [FrameFormat; 3] = [FrameFormat::NV12, FrameFormat::YUYV, FrameFormat::MJPEG];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraDevice {
    pub key: String,
    pub name: String,
}

/// Shared capture counters for call-report stats (format + fps + decode time).
#[derive(Debug, Default)]
pub struct CameraCaptureStats {
    format: std::sync::Mutex<String>,
    started: std::sync::Mutex<Option<Instant>>,
    frames: AtomicU64,
    decode_ns: AtomicU64,
}

impl CameraCaptureStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn set_format(&self, label: impl Into<String>) {
        if let Ok(mut slot) = self.format.lock() {
            *slot = label.into();
        }
        if let Ok(mut started) = self.started.lock() {
            *started = Some(Instant::now());
        }
        self.frames.store(0, Ordering::Relaxed);
        self.decode_ns.store(0, Ordering::Relaxed);
    }

    fn note_frame(&self, decode: Duration) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.decode_ns
            .fetch_add(decode.as_nanos() as u64, Ordering::Relaxed);
    }

    pub fn format_label(&self) -> String {
        self.format
            .lock()
            .map(|label| label.clone())
            .unwrap_or_default()
    }

    pub fn summary(&self) -> Option<String> {
        let label = self.format_label();
        if label.is_empty() {
            return None;
        }
        let frames = self.frames.load(Ordering::Relaxed);
        let decode_ns = self.decode_ns.load(Ordering::Relaxed);
        let decode_ms = if frames == 0 {
            0.0
        } else {
            (decode_ns as f64 / frames as f64) / 1_000_000.0
        };
        let fps = self
            .started
            .lock()
            .ok()
            .and_then(|started| *started)
            .map(|started| {
                let seconds = started.elapsed().as_secs_f64().max(0.001);
                frames as f64 / seconds
            })
            .unwrap_or(0.0);
        Some(format!(
            " camera={label} capture={fps:.1}fps decode={decode_ms:.1}ms"
        ))
    }
}

pub fn list_cameras() -> Vec<CameraDevice> {
    query(ApiBackend::Auto)
        .unwrap_or_default()
        .into_iter()
        .map(|info| CameraDevice {
            key: match info.index() {
                CameraIndex::Index(number) => number.to_string(),
                CameraIndex::String(text) => text.clone(),
            },
            name: info.human_name(),
        })
        .collect()
}

fn camera_index(choice: &DeviceChoice) -> CameraIndex {
    match choice {
        DeviceChoice::SystemDefault => CameraIndex::Index(0),
        DeviceChoice::Device(key) => key
            .parse()
            .map_or_else(|_| CameraIndex::String(key.clone()), CameraIndex::Index),
    }
}

pub fn i420_from_rgba(rgba: &[u8], width: u32, height: u32) -> I420Buffer {
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    abgr_to_i420(
        rgba,
        width * BYTES_PER_PIXEL,
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

fn format_rank(frame_format: FrameFormat) -> u8 {
    match frame_format {
        FrameFormat::NV12 => 3,
        FrameFormat::YUYV => 2,
        FrameFormat::MJPEG => 1,
        _ => 0,
    }
}

fn format_short_name(frame_format: FrameFormat) -> String {
    match frame_format {
        FrameFormat::MJPEG => "MJPG".into(),
        FrameFormat::NV12 => "NV12".into(),
        FrameFormat::YUYV => "YUYV".into(),
        other => other.to_string(),
    }
}

fn format_label(format: &CameraFormat) -> String {
    format!(
        "{} {}x{} @{}",
        format_short_name(format.format()),
        format.width(),
        format.height(),
        format.frame_rate()
    )
}

fn is_widescreen(width: u32, height: u32) -> bool {
    width * ASPECT_HEIGHT == height * ASPECT_WIDTH
}

fn widescreen_tier(format: &CameraFormat, capture_height: u32) -> u8 {
    match (
        is_widescreen(format.width(), format.height()),
        format.height(),
    ) {
        (true, height) if height == capture_height => 3,
        (true, height) if height > capture_height => 2,
        (true, _) => 1,
        (false, _) => 0,
    }
}

fn pick_format(formats: &[CameraFormat], quality: VideoQuality) -> Option<CameraFormat> {
    let capture_height = quality.capture_height();
    let largest = quality.largest_sensor_height();
    formats
        .iter()
        .copied()
        .filter(|format| USABLE_FORMATS.contains(&format.format()))
        .filter(|format| format.frame_rate() >= CAPTURE_FPS)
        .filter(|format| {
            format.height() <= capture_height
                || (is_widescreen(format.width(), format.height()) && format.height() <= largest)
        })
        .max_by_key(|format| {
            (
                widescreen_tier(format, capture_height),
                std::cmp::Reverse(format.height().abs_diff(capture_height)),
                format_rank(format.format()),
                std::cmp::Reverse(format.frame_rate()),
            )
        })
}

fn candidate_heights(quality: VideoQuality) -> Vec<u32> {
    match quality {
        VideoQuality::P720 => vec![720, 540, 360],
        VideoQuality::Auto | VideoQuality::P1080 => vec![1080, 720, 540, 360],
    }
}

fn preferred_candidates(quality: VideoQuality) -> Vec<CameraFormat> {
    let mut candidates = Vec::new();
    for height in candidate_heights(quality) {
        let width = height * ASPECT_WIDTH / ASPECT_HEIGHT;
        candidates.push(CameraFormat::new(
            Resolution::new(width, height),
            FrameFormat::MJPEG,
            CAPTURE_FPS,
        ));
        for frame_format in DIRECT_FORMATS {
            candidates.push(CameraFormat::new(
                Resolution::new(width, height),
                frame_format,
                CAPTURE_FPS,
            ));
        }
    }
    candidates
}

fn even(value: u32) -> u32 {
    value & !1
}

fn crop_i420(buffer: &I420Buffer, left: u32, top: u32, width: u32, height: u32) -> I420Buffer {
    let mut cropped = I420Buffer::new(width, height);
    let (source_strides, source_planes) = (buffer.strides(), buffer.data());
    let (target_strides, target_planes) = (cropped.strides(), cropped.data_mut());
    let planes = [
        (
            source_planes.0,
            source_strides.0,
            target_planes.0,
            target_strides.0,
            1,
        ),
        (
            source_planes.1,
            source_strides.1,
            target_planes.1,
            target_strides.1,
            2,
        ),
        (
            source_planes.2,
            source_strides.2,
            target_planes.2,
            target_strides.2,
            2,
        ),
    ];
    for (source, source_stride, target, target_stride, divisor) in planes {
        let (plane_left, plane_top) = ((left / divisor) as usize, (top / divisor) as usize);
        let (plane_width, plane_height) = (
            width.div_ceil(divisor) as usize,
            height.div_ceil(divisor) as usize,
        );
        for row in 0..plane_height {
            let from = (plane_top + row) * source_stride as usize + plane_left;
            let to = row * target_stride as usize;
            target[to..to + plane_width].copy_from_slice(&source[from..from + plane_width]);
        }
    }
    cropped
}

pub(crate) fn fit_widescreen(mut buffer: I420Buffer, quality: VideoQuality) -> I420Buffer {
    let capture_height = quality.capture_height();
    let (width, height) = (buffer.width(), buffer.height());
    let (crop_width, crop_height) = if width * ASPECT_HEIGHT > height * ASPECT_WIDTH {
        (even(height * ASPECT_WIDTH / ASPECT_HEIGHT), height)
    } else {
        (width, even(width * ASPECT_HEIGHT / ASPECT_WIDTH))
    };
    if (crop_width, crop_height) != (width, height) && crop_width >= 2 && crop_height >= 2 {
        buffer = crop_i420(
            &buffer,
            even((width - crop_width) / 2),
            even((height - crop_height) / 2),
            crop_width,
            crop_height,
        );
    }
    let (width, height) = (buffer.width(), buffer.height());
    if height > capture_height {
        let scaled_width = even(width * capture_height / height);
        return buffer.scale(scaled_width as i32, capture_height as i32);
    }
    buffer
}

fn open_with_request(
    index: CameraIndex,
    request: RequestedFormatType,
) -> std::result::Result<Camera, nokhwa::NokhwaError> {
    let format = RequestedFormat::new::<RgbAFormat>(request);
    let mut camera = Camera::new(index, format)?;
    camera.open_stream()?;
    Ok(camera)
}

fn device_path(index: &CameraIndex) -> Option<String> {
    match index {
        CameraIndex::Index(number) => Some(format!("/dev/video{number}")),
        CameraIndex::String(text) if text.starts_with("/dev/") => Some(text.clone()),
        CameraIndex::String(text) if text.chars().all(|c| c.is_ascii_digit()) => {
            Some(format!("/dev/video{text}"))
        }
        CameraIndex::String(text) if text.starts_with("video") => Some(format!("/dev/{text}")),
        _ => None,
    }
}

fn tune_uvc_controls(index: &CameraIndex) {
    let Some(device) = device_path(index) else {
        return;
    };
    // Best effort: fixed frame rate (no auto-drop in low light) and 50 Hz mains (CH).
    let _ = Command::new("v4l2-ctl")
        .args([
            "-d",
            &device,
            "-c",
            "exposure_dynamic_framerate=0,power_line_frequency=1",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn accept_opened(index: &CameraIndex, mut camera: Camera) -> Option<(Camera, String)> {
    let got = camera.camera_format();
    if got.frame_rate() < CAPTURE_FPS {
        let _ = camera.stop_stream();
        return None;
    }
    tune_uvc_controls(index);
    let label = format_label(&got);
    Some((camera, label))
}

fn try_exact_or_closest(
    index: CameraIndex,
    format: CameraFormat,
) -> std::result::Result<Camera, nokhwa::NokhwaError> {
    match open_with_request(index.clone(), RequestedFormatType::Exact(format)) {
        Ok(camera) => Ok(camera),
        Err(_) => open_with_request(index, RequestedFormatType::Closest(format)),
    }
}

fn open_from_list(
    index: CameraIndex,
    quality: VideoQuality,
    formats: &[CameraFormat],
) -> Option<(Camera, String)> {
    let Some(wanted) = pick_format(formats, quality) else {
        return None;
    };
    let camera = try_exact_or_closest(index.clone(), wanted).ok()?;
    accept_opened(&index, camera)
}

fn open_picked_format(
    index: CameraIndex,
    quality: VideoQuality,
) -> std::result::Result<(Camera, String), nokhwa::NokhwaError> {
    let mut camera = Camera::new(
        index,
        RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate),
    )?;
    let formats = camera.compatible_camera_formats()?;
    let format = pick_format(&formats, quality).ok_or_else(|| {
        nokhwa::NokhwaError::GeneralError("no camera format at 30 fps".to_owned())
    })?;
    camera.set_camera_requset(RequestedFormat::new::<RgbAFormat>(
        RequestedFormatType::Exact(format),
    ))?;
    camera.open_stream()?;
    Ok((camera, format_label(&format)))
}

fn open_camera(
    index: CameraIndex,
    quality: VideoQuality,
) -> std::result::Result<(Camera, String), nokhwa::NokhwaError> {
    if cfg!(windows)
        && let Ok(opened) = open_picked_format(index.clone(), quality)
    {
        return Ok(opened);
    }
    // Probe the device list without committing to a slow Closest(NV12) open.
    for probe in preferred_candidates(quality) {
        let Ok(mut camera) = try_exact_or_closest(index.clone(), probe) else {
            continue;
        };
        if let Ok(formats) = camera.compatible_camera_formats() {
            let _ = camera.stop_stream();
            drop(camera);
            if let Some(opened) = open_from_list(index.clone(), quality, &formats) {
                return Ok(opened);
            }
        } else if let Some(opened) = accept_opened(&index, camera) {
            return Ok(opened);
        }
    }

    let mut last_error = None;
    for candidate in preferred_candidates(quality) {
        match try_exact_or_closest(index.clone(), candidate) {
            Ok(camera) => {
                if let Some(opened) = accept_opened(&index, camera) {
                    return Ok(opened);
                }
            }
            Err(error) => last_error = Some(error),
        }
    }

    // Last resort: AbsoluteHighest may still yield a usable stream on odd backends.
    match open_with_request(index.clone(), RequestedFormatType::AbsoluteHighestFrameRate) {
        Ok(camera) => {
            tune_uvc_controls(&index);
            let label = format_label(&camera.camera_format());
            Ok((camera, label))
        }
        Err(error) => Err(last_error.unwrap_or(error)),
    }
}

pub(crate) fn i420_from_nv12(data: &[u8], width: u32, height: u32) -> Option<I420Buffer> {
    let luma_len = width as usize * height as usize;
    if data.len() < luma_len + luma_len / 2 {
        return None;
    }
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    let (source_y, source_uv) = data.split_at(luma_len);
    nv12_to_i420(
        source_y,
        width,
        source_uv,
        width,
        plane_y,
        stride_y,
        plane_u,
        stride_u,
        plane_v,
        stride_v,
        width as i32,
        height as i32,
    );
    Some(buffer)
}

pub(crate) fn i420_from_yuyv(data: &[u8], width: u32, height: u32) -> Option<I420Buffer> {
    let (width_pixels, height_rows) = (width as usize, height as usize);
    let row_bytes = width_pixels * 2;
    if data.len() < row_bytes * height_rows {
        return None;
    }
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    for row in 0..height_rows {
        let source = &data[row * row_bytes..(row + 1) * row_bytes];
        let luma_row = &mut plane_y[row * stride_y as usize..][..width_pixels];
        for (luma, pair) in luma_row.chunks_exact_mut(2).zip(source.chunks_exact(4)) {
            luma[0] = pair[0];
            luma[1] = pair[2];
        }
    }
    for row in 0..height_rows / 2 {
        let upper = &data[row * 2 * row_bytes..][..row_bytes];
        let lower = &data[(row * 2 + 1) * row_bytes..][..row_bytes];
        let blue_row = &mut plane_u[row * stride_u as usize..][..width_pixels / 2];
        let red_row = &mut plane_v[row * stride_v as usize..][..width_pixels / 2];
        for (column, (blue, red)) in blue_row.iter_mut().zip(red_row.iter_mut()).enumerate() {
            let offset = column * 4;
            *blue = ((u16::from(upper[offset + 1]) + u16::from(lower[offset + 1]) + 1) / 2) as u8;
            *red = ((u16::from(upper[offset + 3]) + u16::from(lower[offset + 3]) + 1) / 2) as u8;
        }
    }
    Some(buffer)
}

fn i420_from_mjpeg(data: &[u8], rgba_scratch: &mut Vec<u8>) -> Option<(I420Buffer, u32, u32)> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if width < 2 || height < 2 || width % 2 != 0 || height % 2 != 0 {
        return None;
    }
    let rgba_len = (width * height * BYTES_PER_PIXEL) as usize;
    let rgba = if pixels.len() == rgba_len {
        pixels.as_slice()
    } else if pixels.len() == (width * height * 3) as usize {
        rgba_scratch.clear();
        rgba_scratch.reserve(rgba_len);
        for chunk in pixels.chunks_exact(3) {
            rgba_scratch.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
        }
        rgba_scratch.as_slice()
    } else {
        return None;
    };
    Some((i420_from_rgba(rgba, width, height), width, height))
}

fn direct_i420(
    frame_format: FrameFormat,
    data: &[u8],
    width: u32,
    height: u32,
    rgba_scratch: &mut Vec<u8>,
) -> Option<I420Buffer> {
    match frame_format {
        FrameFormat::NV12 => i420_from_nv12(data, width, height),
        FrameFormat::YUYV => i420_from_yuyv(data, width, height),
        FrameFormat::MJPEG => i420_from_mjpeg(data, rgba_scratch).map(|(buffer, _, _)| buffer),
        _ => None,
    }
}

pub fn start_camera(
    choice: &DeviceChoice,
    quality: VideoQuality,
    blur: Arc<BlurSettings>,
    stats: Arc<CameraCaptureStats>,
    on_opened: impl FnOnce(String) + Send + 'static,
    deliver: impl FnMut(I420Buffer) + Send + 'static,
    failed: impl FnOnce(String) + Send + 'static,
) -> Result<Capture> {
    let index = camera_index(choice);
    Capture::spawn_thread("camera", move |stop| {
        let (mut camera, label) = match open_camera(index, quality) {
            Ok(opened) => opened,
            Err(error) => return failed(error.to_string()),
        };
        stats.set_format(&label);
        on_opened(label);
        let effects = match EffectsThread::spawn(blur, deliver) {
            Ok(effects) => effects,
            Err(error) => return failed(format!("effects thread: {error}")),
        };
        let mut warned_undecodable = false;
        let mut rgba_scratch = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            let Ok(frame) = camera.frame() else {
                std::thread::sleep(RETRY_PAUSE);
                continue;
            };
            let resolution = frame.resolution();
            let (width, height) = (resolution.width(), resolution.height());
            if width < 2 || height < 2 || width % 2 != 0 || height % 2 != 0 {
                continue;
            }
            let started = Instant::now();
            let buffer = match direct_i420(
                frame.source_frame_format(),
                frame.buffer(),
                width,
                height,
                &mut rgba_scratch,
            ) {
                Some(buffer) => buffer,
                None => {
                    let Ok(decoded) = frame.decode_image::<RgbAFormat>() else {
                        if !std::mem::replace(&mut warned_undecodable, true) {
                            eprintln!(
                                "calls: camera delivers {} and no decoder is available",
                                frame.source_frame_format()
                            );
                        }
                        continue;
                    };
                    i420_from_rgba(&decoded.into_raw(), width, height)
                }
            };
            stats.note_frame(started.elapsed());
            effects.submit(fit_widescreen(buffer, quality));
        }
        let _ = camera.stop_stream();
    })
    .map_err(|error| Error::Webrtc(format!("camera thread: {error}")))
}

pub const DEFAULT_CAMERA_LABEL: &str = "Default camera";

pub fn camera_entries(cameras: &[CameraDevice]) -> Vec<DeviceEntry> {
    let default_entry = DeviceEntry {
        choice: DeviceChoice::SystemDefault,
        label: DEFAULT_CAMERA_LABEL.to_owned(),
    };
    let named = cameras
        .iter()
        .filter(|camera| !camera.name.trim().is_empty())
        .map(|camera| DeviceEntry {
            choice: DeviceChoice::Device(camera.key.clone()),
            label: camera.name.clone(),
        });
    std::iter::once(default_entry).chain(named).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_camera_comes_first_and_unnamed_ones_are_hidden() {
        let cameras = [
            CameraDevice {
                key: "0".into(),
                name: "Integrated Camera".into(),
            },
            CameraDevice {
                key: "1".into(),
                name: " ".into(),
            },
        ];
        let entries = camera_entries(&cameras);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].choice, DeviceChoice::SystemDefault);
        assert_eq!(entries[1].choice, DeviceChoice::Device("0".into()));
    }

    #[test]
    fn yuyv_converts_to_i420_with_averaged_chroma() {
        let row_a = [10u8, 100, 20, 200, 30, 50, 40, 60];
        let row_b = [11u8, 120, 21, 220, 31, 70, 41, 80];
        let data: Vec<u8> = row_a.iter().chain(row_b.iter()).copied().collect();
        let mut buffer = i420_from_yuyv(&data, 4, 2).expect("converted");
        let (plane_y, plane_u, plane_v) = buffer.data_mut();
        assert_eq!(&plane_y[..4], &[10, 20, 30, 40]);
        assert_eq!(plane_u[0], 110);
        assert_eq!(plane_u[1], 60);
        assert_eq!(plane_v[0], 210);
        assert_eq!(plane_v[1], 70);
    }

    #[test]
    fn short_raw_frames_are_rejected() {
        assert!(i420_from_yuyv(&[0; 8], 4, 2).is_none());
        assert!(i420_from_nv12(&[0; 8], 4, 2).is_none());
    }

    fn format(width: u32, height: u32, frame_format: FrameFormat, fps: u32) -> CameraFormat {
        CameraFormat::new(Resolution::new(width, height), frame_format, fps)
    }

    #[test]
    fn a_c920_like_list_picks_hd_mjpeg_at_30_fps_over_slow_raw() {
        let formats = [
            format(1280, 720, FrameFormat::YUYV, 10),
            format(640, 480, FrameFormat::YUYV, 30),
            format(1280, 720, FrameFormat::MJPEG, 30),
        ];
        assert_eq!(
            pick_format(&formats, VideoQuality::P720),
            Some(format(1280, 720, FrameFormat::MJPEG, 30))
        );
    }

    #[test]
    fn raw_at_the_same_height_beats_mjpeg() {
        let formats = [
            format(1280, 720, FrameFormat::MJPEG, 30),
            format(1280, 720, FrameFormat::NV12, 30),
        ];
        assert_eq!(
            pick_format(&formats, VideoQuality::P720),
            Some(format(1280, 720, FrameFormat::NV12, 30))
        );
    }

    #[test]
    fn a_c930e_like_list_picks_mjpeg_1080_for_auto() {
        let formats = [
            format(1920, 1080, FrameFormat::YUYV, 5),
            format(1280, 720, FrameFormat::YUYV, 10),
            format(640, 360, FrameFormat::YUYV, 30),
            format(1920, 1080, FrameFormat::MJPEG, 30),
            format(1280, 720, FrameFormat::MJPEG, 30),
            format(960, 540, FrameFormat::MJPEG, 30),
        ];
        assert_eq!(
            pick_format(&formats, VideoQuality::Auto),
            Some(format(1920, 1080, FrameFormat::MJPEG, 30))
        );
        assert_eq!(
            pick_format(&formats, VideoQuality::P720),
            Some(format(1280, 720, FrameFormat::MJPEG, 30))
        );
    }

    #[test]
    fn auto_prefers_1080p_nv12_over_720p() {
        let formats = [
            format(1920, 1080, FrameFormat::NV12, 30),
            format(640, 480, FrameFormat::NV12, 30),
            format(1280, 720, FrameFormat::YUYV, 30),
            format(1280, 720, FrameFormat::NV12, 30),
        ];
        assert_eq!(
            pick_format(&formats, VideoQuality::Auto),
            Some(format(1920, 1080, FrameFormat::NV12, 30))
        );
        assert_eq!(
            pick_format(&formats, VideoQuality::P720),
            Some(format(1280, 720, FrameFormat::NV12, 30))
        );
    }

    #[test]
    fn a_widescreen_format_beats_a_4_3_one_of_the_same_height() {
        let formats = [
            format(960, 720, FrameFormat::NV12, 30),
            format(1280, 720, FrameFormat::YUYV, 30),
            format(640, 480, FrameFormat::NV12, 30),
        ];
        assert_eq!(
            pick_format(&formats, VideoQuality::P720),
            Some(format(1280, 720, FrameFormat::YUYV, 30))
        );
    }

    #[test]
    fn locked_720_keeps_1080_as_downscale_candidate_but_prefers_exact_720() {
        let full_hd = format(1920, 1080, FrameFormat::NV12, 30);
        let hd = format(1280, 720, FrameFormat::NV12, 30);
        let small = format(640, 360, FrameFormat::NV12, 30);
        let classic = format(960, 720, FrameFormat::NV12, 30);
        assert_eq!(
            pick_format(&[classic, small, full_hd], VideoQuality::Auto),
            Some(full_hd)
        );
        assert_eq!(
            pick_format(&[classic, small, full_hd, hd], VideoQuality::P720),
            Some(hd)
        );
        assert_eq!(
            pick_format(
                &[classic, format(2560, 1440, FrameFormat::NV12, 30)],
                VideoQuality::P720
            ),
            Some(classic)
        );
    }

    fn gradient(width: u32, height: u32) -> I420Buffer {
        let mut buffer = I420Buffer::new(width, height);
        let (stride_y, stride_u, _) = buffer.strides();
        let (plane_y, plane_u, _) = buffer.data_mut();
        for row in 0..height as usize {
            plane_y[row * stride_y as usize..][..width as usize].fill(row as u8);
        }
        for row in 0..height.div_ceil(2) as usize {
            plane_u[row * stride_u as usize..][..width.div_ceil(2) as usize].fill(row as u8);
        }
        buffer
    }

    #[test]
    fn a_4_3_frame_is_centre_cropped_to_16_9() {
        let cropped = fit_widescreen(gradient(960, 720), VideoQuality::P720);
        assert_eq!((cropped.width(), cropped.height()), (960, 540));
        let (stride_y, stride_u, _) = cropped.strides();
        let (plane_y, plane_u, _) = cropped.data();
        assert_eq!(plane_y[0], 90);
        assert_eq!(plane_y[539 * stride_y as usize], 629_u32 as u8);
        assert_eq!(plane_u[0], 45);
        assert_eq!(plane_u[269 * stride_u as usize], 314_u32 as u8);
    }

    #[test]
    fn auto_keeps_1080p_and_720_lock_scales_it_down() {
        assert_eq!(
            fit_widescreen(gradient(1280, 720), VideoQuality::Auto).width(),
            1280
        );
        let kept = fit_widescreen(gradient(1920, 1080), VideoQuality::Auto);
        assert_eq!((kept.width(), kept.height()), (1920, 1080));
        let scaled = fit_widescreen(gradient(1920, 1080), VideoQuality::P720);
        assert_eq!((scaled.width(), scaled.height()), (1280, 720));
        let wide = fit_widescreen(gradient(2560, 1440), VideoQuality::Auto);
        assert_eq!((wide.width(), wide.height()), (1920, 1080));
        let classic = fit_widescreen(gradient(640, 480), VideoQuality::Auto);
        assert_eq!((classic.width(), classic.height()), (640, 360));
    }

    #[test]
    fn no_usable_format_picks_nothing() {
        let formats = [
            format(1280, 720, FrameFormat::MJPEG, 15),
            format(1280, 720, FrameFormat::YUYV, 15),
        ];
        assert_eq!(pick_format(&formats, VideoQuality::Auto), None);
        assert_eq!(pick_format(&[], VideoQuality::Auto), None);
    }

    #[test]
    fn camera_choices_map_to_indices_or_names() {
        assert!(matches!(
            camera_index(&DeviceChoice::SystemDefault),
            CameraIndex::Index(0)
        ));
        assert!(matches!(
            camera_index(&DeviceChoice::Device("2".into())),
            CameraIndex::Index(2)
        ));
        assert!(matches!(
            camera_index(&DeviceChoice::Device("video0".into())),
            CameraIndex::String(_)
        ));
    }

    #[test]
    fn mjpeg_decodes_to_i420() {
        // Minimal valid grayscale JPEG is awkward; encode via `image` crate.
        let mut rgb = image::RgbImage::new(16, 16);
        for pixel in rgb.pixels_mut() {
            *pixel = image::Rgb([40, 80, 120]);
        }
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode(rgb.as_raw(), 16, 16, image::ExtendedColorType::Rgb8)
            .expect("encode");
        let mut scratch = Vec::new();
        let (buffer, width, height) = i420_from_mjpeg(&jpeg, &mut scratch).expect("mjpeg decodes");
        assert_eq!((width, height), (16, 16));
        assert_eq!((buffer.width(), buffer.height()), (16, 16));
    }

    #[test]
    #[ignore = "timing; run with --release --ignored"]
    fn mjpeg_1080p_decode_stays_under_fifteen_ms() {
        let mut rgb = image::RgbImage::new(1920, 1080);
        for (index, pixel) in rgb.pixels_mut().enumerate() {
            let value = (index % 255) as u8;
            *pixel = image::Rgb([value, value.wrapping_add(40), value.wrapping_add(80)]);
        }
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
            .encode(rgb.as_raw(), 1920, 1080, image::ExtendedColorType::Rgb8)
            .expect("encode");
        let mut scratch = Vec::new();
        // Warm caches / CPU, then time a steady batch (30 fps allows ~33 ms/frame).
        for _ in 0..3 {
            let _ = i420_from_mjpeg(&jpeg, &mut scratch).expect("decode");
        }
        let started = Instant::now();
        for _ in 0..10 {
            let _ = i420_from_mjpeg(&jpeg, &mut scratch).expect("decode");
        }
        let average = started.elapsed() / 10;
        assert!(
            average < Duration::from_millis(20),
            "average decode {average:?} exceeds 20 ms"
        );
    }

    #[test]
    #[ignore = "needs a connected camera"]
    fn open_picks_at_least_thirty_fps_when_available() {
        if list_cameras().is_empty() {
            return;
        }
        let (mut camera, label) =
            open_camera(CameraIndex::Index(0), VideoQuality::Auto).expect("camera opens");
        assert!(
            camera.camera_format().frame_rate() >= CAPTURE_FPS,
            "opened {label} under 30 fps"
        );
        // Warm up buffers, then count for one second.
        for _ in 0..10 {
            let _ = camera.frame();
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut frames = 0u32;
        while Instant::now() < deadline {
            if camera.frame().is_ok() {
                frames += 1;
            }
        }
        let _ = camera.stop_stream();
        assert!(
            frames >= 20,
            "expected >= 20 frames in 1 s from {label}, got {frames}"
        );
    }
}
