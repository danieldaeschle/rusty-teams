use libwebrtc::native::yuv_helper::i420_to_abgr;
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::mask::Mask;

const CHANNELS: usize = 4;

pub(crate) const MEDIAPIPE_BYTES: &[u8] = include_bytes!("../../assets/selfie_segmentation_landscape.onnx");
pub(crate) const MODNET_BYTES: &[u8] = include_bytes!("../../assets/modnet_webcam_portrait_matting.onnx");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRange {
    Unit,
    Signed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    MediaPipe,
    Modnet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    pub name: &'static str,
    pub network: Network,
    pub width: usize,
    pub height: usize,
    pub range: InputRange,
}

pub const MEDIAPIPE: ModelSpec = ModelSpec { name: "mediapipe-256x144", network: Network::MediaPipe, width: 256, height: 144, range: InputRange::Unit };
pub const MODNET_SMALL: ModelSpec = ModelSpec { name: "modnet-320x192", network: Network::Modnet, width: 320, height: 192, range: InputRange::Signed };
pub const MODNET_LARGE: ModelSpec = ModelSpec { name: "modnet-512x288", network: Network::Modnet, width: 512, height: 288, range: InputRange::Signed };

impl ModelSpec {
    pub(crate) fn bytes(&self) -> &'static [u8] {
        match self.network {
            Network::MediaPipe => MEDIAPIPE_BYTES,
            Network::Modnet => MODNET_BYTES,
        }
    }

    pub fn input(&self, frame: &mut I420Buffer) -> Vec<f32> {
        let (width, height) = (self.width as u32, self.height as u32);
        let scaled = if (frame.width(), frame.height()) == (width, height) { None } else { Some(frame.scale(width as i32, height as i32)) };
        let source = scaled.as_ref().unwrap_or(&*frame);
        let (stride_y, stride_u, stride_v) = source.strides();
        let (plane_y, plane_u, plane_v) = source.data();
        let mut rgba = vec![0u8; self.width * self.height * CHANNELS];
        i420_to_abgr(plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, &mut rgba, width * CHANNELS as u32, width as i32, height as i32);
        self.input_from_rgba(&rgba)
    }

    pub(crate) fn input_from_rgba(&self, rgba: &[u8]) -> Vec<f32> {
        let plane = self.width * self.height;
        let (scale, offset) = match self.range {
            InputRange::Unit => (1. / 255., 0.),
            InputRange::Signed => (2. / 255., -1.),
        };
        let mut data = vec![0f32; 3 * plane];
        for (index, pixel) in rgba.chunks_exact(CHANNELS).enumerate() {
            for channel in 0..3 {
                data[channel * plane + index] = f32::from(pixel[channel]) * scale + offset;
            }
        }
        data
    }

    pub(crate) fn mask(&self, output: &[f32]) -> Option<Mask> {
        if output.len() != self.width * self.height {
            return None;
        }
        let mask = Mask::new(self.width, self.height, output.iter().map(|value| value.clamp(0., 1.)).collect());
        Some(if self.network == Network::Modnet { mask.into_matte() } else { mask })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_planar_and_scaled_to_the_model_range() {
        let tiny = ModelSpec { width: 2, height: 1, ..MODNET_SMALL };
        let rgba = [255, 0, 51, 255, 0, 255, 102, 255];
        let signed = tiny.input_from_rgba(&rgba);
        assert_eq!(signed.len(), 6);
        assert!((signed[0] - 1.).abs() < 1e-6 && (signed[1] + 1.).abs() < 1e-6 && (signed[2] + 1.).abs() < 1e-6);
        let unit = ModelSpec { range: InputRange::Unit, ..tiny }.input_from_rgba(&rgba);
        assert!((unit[4] - 0.2).abs() < 1e-6 && (unit[5] - 0.4).abs() < 1e-6);
        assert!(tiny.mask(&[0.5]).is_none());
        assert_eq!(tiny.mask(&[1.5, -0.2]).unwrap().values, vec![1., 0.]);
    }
}
