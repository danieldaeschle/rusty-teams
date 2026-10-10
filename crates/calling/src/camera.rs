use std::sync::Arc;
use std::time::Duration;

use libwebrtc::native::yuv_helper::abgr_to_i420;
use libwebrtc::video_frame::I420Buffer;
use nokhwa::pixel_format::RgbAFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::{Camera, query};
use std::sync::atomic::Ordering;

use crate::blur::{BlurSettings, BlurStage};
use crate::capture::Capture;
use crate::devices::{DeviceChoice, DeviceEntry};
use crate::error::{Error, Result};

const RETRY_PAUSE: Duration = Duration::from_millis(20);
const BYTES_PER_PIXEL: u32 = 4;

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

pub(crate) fn i420_from_rgba(rgba: &[u8], width: u32, height: u32) -> I420Buffer {
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

pub fn start_camera(
    choice: &DeviceChoice,
    blur: Arc<BlurSettings>,
    mut deliver: impl FnMut(I420Buffer) + Send + 'static,
    failed: impl FnOnce(String) + Send + 'static,
) -> Result<Capture> {
    let index = camera_index(choice);
    Capture::spawn_thread("camera", move |stop| {
        let format = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
        let mut camera = match Camera::new(index, format).and_then(|mut camera| camera.open_stream().map(|()| camera)) {
            Ok(camera) => camera,
            Err(error) => return failed(error.to_string()),
        };
        let mut blur_stage = BlurStage::new(blur);
        while !stop.load(Ordering::SeqCst) {
            let Ok(frame) = camera.frame() else {
                std::thread::sleep(RETRY_PAUSE);
                continue;
            };
            let Ok(decoded) = frame.decode_image::<RgbAFormat>() else { continue };
            let (width, height) = (decoded.width(), decoded.height());
            if width >= 2 && height >= 2 && width % 2 == 0 && height % 2 == 0 {
                let mut rgba = decoded.into_raw();
                blur_stage.apply_rgba(&mut rgba, width as usize, height as usize);
                deliver(i420_from_rgba(&rgba, width, height));
            }
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
    fn camera_choices_map_to_indices_or_names() {
        assert!(matches!(camera_index(&DeviceChoice::SystemDefault), CameraIndex::Index(0)));
        assert!(matches!(camera_index(&DeviceChoice::Device("2".into())), CameraIndex::Index(2)));
        assert!(matches!(camera_index(&DeviceChoice::Device("video0".into())), CameraIndex::String(_)));
    }
}
