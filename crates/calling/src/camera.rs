use std::sync::Arc;
use std::time::Duration;

use libwebrtc::native::yuv_helper::{abgr_to_i420, nv12_to_i420};
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};
use nokhwa::pixel_format::RgbAFormat;
use nokhwa::utils::{ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution};
use nokhwa::{Camera, query};
use std::sync::atomic::Ordering;

use crate::blur::BlurSettings;
use crate::effects::EffectsStage;
use crate::capture::Capture;
use crate::devices::{DeviceChoice, DeviceEntry};
use crate::error::{Error, Result};

const RETRY_PAUSE: Duration = Duration::from_millis(20);
const BYTES_PER_PIXEL: u32 = 4;
const CAPTURE_WIDTH: u32 = 1280;
const CAPTURE_HEIGHT: u32 = 720;
const CAPTURE_FPS: u32 = 30;
const LARGEST_DOWNSCALED_HEIGHT: u32 = 1080;
const ASPECT_WIDTH: u32 = 16;
const ASPECT_HEIGHT: u32 = 9;
const DIRECT_FORMATS: [FrameFormat; 2] = [FrameFormat::NV12, FrameFormat::YUYV];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraDevice {
    pub key: String,
    pub name: String,
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
        DeviceChoice::Device(key) => key.parse().map_or_else(|_| CameraIndex::String(key.clone()), CameraIndex::Index),
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
        FrameFormat::NV12 => 1,
        _ => 0,
    }
}

fn is_widescreen(width: u32, height: u32) -> bool {
    width * ASPECT_HEIGHT == height * ASPECT_WIDTH
}

fn widescreen_tier(format: &CameraFormat) -> u8 {
    match (is_widescreen(format.width(), format.height()), format.height()) {
        (true, CAPTURE_HEIGHT) => 3,
        (true, height) if height > CAPTURE_HEIGHT => 2,
        (true, _) => 1,
        (false, _) => 0,
    }
}

fn pick_format(formats: &[CameraFormat]) -> Option<CameraFormat> {
    formats
        .iter()
        .copied()
        .filter(|format| DIRECT_FORMATS.contains(&format.format()))
        .filter(|format| format.frame_rate() >= CAPTURE_FPS)
        .filter(|format| {
            format.height() <= CAPTURE_HEIGHT
                || (is_widescreen(format.width(), format.height()) && format.height() <= LARGEST_DOWNSCALED_HEIGHT)
        })
        .max_by_key(|format| {
            (
                widescreen_tier(format),
                std::cmp::Reverse(format.height().abs_diff(CAPTURE_HEIGHT)),
                format_rank(format.format()),
                std::cmp::Reverse(format.frame_rate()),
            )
        })
}

fn even(value: u32) -> u32 {
    value & !1
}

fn crop_i420(buffer: &I420Buffer, left: u32, top: u32, width: u32, height: u32) -> I420Buffer {
    let mut cropped = I420Buffer::new(width, height);
    let (source_strides, source_planes) = (buffer.strides(), buffer.data());
    let (target_strides, target_planes) = (cropped.strides(), cropped.data_mut());
    let planes = [
        (source_planes.0, source_strides.0, target_planes.0, target_strides.0, 1),
        (source_planes.1, source_strides.1, target_planes.1, target_strides.1, 2),
        (source_planes.2, source_strides.2, target_planes.2, target_strides.2, 2),
    ];
    for (source, source_stride, target, target_stride, divisor) in planes {
        let (plane_left, plane_top) = ((left / divisor) as usize, (top / divisor) as usize);
        let (plane_width, plane_height) = (width.div_ceil(divisor) as usize, height.div_ceil(divisor) as usize);
        for row in 0..plane_height {
            let from = (plane_top + row) * source_stride as usize + plane_left;
            let to = row * target_stride as usize;
            target[to..to + plane_width].copy_from_slice(&source[from..from + plane_width]);
        }
    }
    cropped
}

pub(crate) fn fit_widescreen(mut buffer: I420Buffer) -> I420Buffer {
    let (width, height) = (buffer.width(), buffer.height());
    let (crop_width, crop_height) = if width * ASPECT_HEIGHT > height * ASPECT_WIDTH {
        (even(height * ASPECT_WIDTH / ASPECT_HEIGHT), height)
    } else {
        (width, even(width * ASPECT_HEIGHT / ASPECT_WIDTH))
    };
    if (crop_width, crop_height) != (width, height) && crop_width >= 2 && crop_height >= 2 {
        buffer = crop_i420(&buffer, even((width - crop_width) / 2), even((height - crop_height) / 2), crop_width, crop_height);
    }
    let (width, height) = (buffer.width(), buffer.height());
    if height > CAPTURE_HEIGHT {
        let scaled_width = even(width * CAPTURE_HEIGHT / height);
        return buffer.scale(scaled_width as i32, CAPTURE_HEIGHT as i32);
    }
    buffer
}

fn open_picked_format(index: CameraIndex) -> std::result::Result<Camera, nokhwa::NokhwaError> {
    let mut camera = Camera::new(index, RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate))?;
    let formats = camera.compatible_camera_formats()?;
    let format = pick_format(&formats).ok_or_else(|| nokhwa::NokhwaError::GeneralError("no raw camera format at 30 fps".to_owned()))?;
    camera.set_camera_requset(RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Exact(format)))?;
    camera.open_stream()?;
    Ok(camera)
}

fn open_camera(index: CameraIndex) -> std::result::Result<Camera, nokhwa::NokhwaError> {
    if let Ok(camera) = open_picked_format(index.clone()) {
        return Ok(camera);
    }
    let wanted = Resolution::new(CAPTURE_WIDTH, CAPTURE_HEIGHT);
    let mut requests: Vec<RequestedFormatType> = DIRECT_FORMATS
        .into_iter()
        .map(|frame_format| RequestedFormatType::Closest(CameraFormat::new(wanted, frame_format, CAPTURE_FPS)))
        .collect();
    requests.push(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut last_error = None;
    for request in requests {
        let format = RequestedFormat::new::<RgbAFormat>(request);
        match Camera::new(index.clone(), format).and_then(|mut camera| camera.open_stream().map(|()| camera)) {
            Ok(camera) => return Ok(camera),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("at least one request"))
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
    nv12_to_i420(source_y, width, source_uv, width, plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, width as i32, height as i32);
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

fn direct_i420(frame_format: FrameFormat, data: &[u8], width: u32, height: u32) -> Option<I420Buffer> {
    match frame_format {
        FrameFormat::NV12 => i420_from_nv12(data, width, height),
        FrameFormat::YUYV => i420_from_yuyv(data, width, height),
        _ => None,
    }
}

pub fn start_camera(
    choice: &DeviceChoice,
    blur: Arc<BlurSettings>,
    mut deliver: impl FnMut(I420Buffer) + Send + 'static,
    failed: impl FnOnce(String) + Send + 'static,
) -> Result<Capture> {
    let index = camera_index(choice);
    Capture::spawn_thread("camera", move |stop| {
        let mut camera = match open_camera(index) {
            Ok(camera) => camera,
            Err(error) => return failed(error.to_string()),
        };
        let mut stage = EffectsStage::new(blur);
        let mut warned_undecodable = false;
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
            let buffer = match direct_i420(frame.source_frame_format(), frame.buffer(), width, height) {
                Some(buffer) => buffer,
                None => {
                    let Ok(decoded) = frame.decode_image::<RgbAFormat>() else {
                        if !std::mem::replace(&mut warned_undecodable, true) {
                            eprintln!("calls: camera delivers {} and no decoder is available", frame.source_frame_format());
                        }
                        continue;
                    };
                    i420_from_rgba(&decoded.into_raw(), width, height)
                }
            };
            deliver(stage.apply_i420(fit_widescreen(buffer)));
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
            CameraDevice { key: "0".into(), name: "Integrated Camera".into() },
            CameraDevice { key: "1".into(), name: " ".into() },
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
    fn a_c920_like_list_picks_the_raw_format_that_runs_at_30_fps() {
        let formats = [
            format(1280, 720, FrameFormat::YUYV, 10),
            format(640, 480, FrameFormat::YUYV, 30),
            format(1280, 720, FrameFormat::MJPEG, 30),
        ];
        assert_eq!(pick_format(&formats), Some(format(640, 480, FrameFormat::YUYV, 30)));
    }

    #[test]
    fn the_largest_height_up_to_720_wins_then_nv12() {
        let formats = [
            format(1920, 1080, FrameFormat::NV12, 30),
            format(640, 480, FrameFormat::NV12, 30),
            format(1280, 720, FrameFormat::YUYV, 30),
            format(1280, 720, FrameFormat::NV12, 30),
        ];
        assert_eq!(pick_format(&formats), Some(format(1280, 720, FrameFormat::NV12, 30)));
    }

    #[test]
    fn a_widescreen_format_beats_a_4_3_one_of_the_same_height() {
        let formats = [format(960, 720, FrameFormat::NV12, 30), format(1280, 720, FrameFormat::YUYV, 30), format(640, 480, FrameFormat::NV12, 30)];
        assert_eq!(pick_format(&formats), Some(format(1280, 720, FrameFormat::YUYV, 30)));
    }

    #[test]
    fn without_720p_the_order_is_1080p_then_360p_then_4_3() {
        let full_hd = format(1920, 1080, FrameFormat::NV12, 30);
        let small = format(640, 360, FrameFormat::NV12, 30);
        let classic = format(960, 720, FrameFormat::NV12, 30);
        assert_eq!(pick_format(&[classic, small, full_hd]), Some(full_hd));
        assert_eq!(pick_format(&[classic, small]), Some(small));
        assert_eq!(pick_format(&[classic, format(2560, 1440, FrameFormat::NV12, 30)]), Some(classic));
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
        let cropped = fit_widescreen(gradient(960, 720));
        assert_eq!((cropped.width(), cropped.height()), (960, 540));
        let (stride_y, stride_u, _) = cropped.strides();
        let (plane_y, plane_u, _) = cropped.data();
        assert_eq!(plane_y[0], 90);
        assert_eq!(plane_y[539 * stride_y as usize], 629_u32 as u8);
        assert_eq!(plane_u[0], 45);
        assert_eq!(plane_u[269 * stride_u as usize], 314_u32 as u8);
    }

    #[test]
    fn a_widescreen_frame_passes_and_1080p_is_scaled_to_720p() {
        assert_eq!(fit_widescreen(gradient(1280, 720)).width(), 1280);
        let scaled = fit_widescreen(gradient(1920, 1080));
        assert_eq!((scaled.width(), scaled.height()), (1280, 720));
        let wide = fit_widescreen(gradient(2560, 1080));
        assert_eq!((wide.width(), wide.height()), (1280, 720));
        let classic = fit_widescreen(gradient(640, 480));
        assert_eq!((classic.width(), classic.height()), (640, 360));
    }

    #[test]
    fn no_usable_format_picks_nothing() {
        let formats = [format(1280, 720, FrameFormat::MJPEG, 30), format(1280, 720, FrameFormat::YUYV, 15)];
        assert_eq!(pick_format(&formats), None);
        assert_eq!(pick_format(&[]), None);
    }

    #[test]
    fn camera_choices_map_to_indices_or_names() {
        assert!(matches!(camera_index(&DeviceChoice::SystemDefault), CameraIndex::Index(0)));
        assert!(matches!(camera_index(&DeviceChoice::Device("2".into())), CameraIndex::Index(2)));
        assert!(matches!(camera_index(&DeviceChoice::Device("video0".into())), CameraIndex::String(_)));
    }
}
