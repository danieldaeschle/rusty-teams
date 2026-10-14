use std::sync::Arc;

use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::blur::{MASK_HEIGHT, MASK_WIDTH, axis};
use crate::matte::{Planes, refine};
use crate::blur::BlurSettings;

const CUTOUT_WIDTH: u32 = 480;
const WIDTH_SHARE: f32 = 0.22;
const MARGIN_SHARE: f32 = 0.03;
const CORNER_SHARE: f32 = 0.1;
const MIN_TARGET_WIDTH: usize = 64;

pub struct Cutout {
    width: usize,
    height: usize,
    luma: Vec<u8>,
    chroma_blue: Vec<u8>,
    chroma_red: Vec<u8>,
    alpha: Vec<u8>,
}

fn corner_coverage(column: usize, row: usize, width: usize, height: usize, radius: f32) -> f32 {
    let x = column as f32 + 0.5;
    let y = row as f32 + 0.5;
    let inner_x = x.clamp(radius, width as f32 - radius);
    let inner_y = y.clamp(radius, height as f32 - radius);
    let distance = ((x - inner_x).powi(2) + (y - inner_y).powi(2)).sqrt();
    (radius - distance + 0.5).clamp(0., 1.)
}

impl Cutout {
    pub fn from_camera(camera: &mut I420Buffer, mask: &[f32]) -> Option<Cutout> {
        if mask.len() != MASK_WIDTH * MASK_HEIGHT || camera.width() < 2 || camera.height() < 2 {
            return None;
        }
        let width = CUTOUT_WIDTH.min(camera.width()) & !1;
        let height = (u64::from(camera.height()) * u64::from(width) / u64::from(camera.width())) as u32 & !1;
        let scaled = camera.scale(width as i32, height as i32);
        let (width, height) = (width as usize, height as usize);
        let (stride_y, stride_u, stride_v) = scaled.strides();
        let (plane_y, plane_u, plane_v) = scaled.data();
        let matte = refine(mask, Planes::of(&scaled));
        let radius = width as f32 * CORNER_SHARE;
        let mut cutout = Cutout {
            width,
            height,
            luma: Vec::with_capacity(width * height),
            chroma_blue: Vec::with_capacity(width * height),
            chroma_red: Vec::with_capacity(width * height),
            alpha: Vec::with_capacity(width * height),
        };
        for row in 0..height {
            for column in 0..width {
                let person = f32::from(matte.alpha[row * width + column]) / 255.;
                let coverage = person * corner_coverage(column, row, width, height, radius);
                cutout.luma.push(plane_y[row * stride_y as usize + column]);
                cutout.chroma_blue.push(plane_u[(row / 2) * stride_u as usize + column / 2]);
                cutout.chroma_red.push(plane_v[(row / 2) * stride_v as usize + column / 2]);
                cutout.alpha.push((coverage * 255. + 0.5) as u8);
            }
        }
        Some(cutout)
    }

    fn scaled_to(&self, width: usize, height: usize) -> Cutout {
        let across = axis(width, self.width);
        let down = axis(height, self.height);
        let resample = |plane: &[u8]| -> Vec<u8> {
            let mut output = Vec::with_capacity(width * height);
            for row in 0..height {
                let (top, bottom, vertical) = (down.first[row] * self.width, down.second[row] * self.width, down.weight[row]);
                for column in 0..width {
                    let (left, right, horizontal) = (across.first[column], across.second[column], across.weight[column]);
                    let sample = |index: usize| f32::from(plane[index]);
                    let upper = sample(top + left) * (1. - horizontal) + sample(top + right) * horizontal;
                    let lower = sample(bottom + left) * (1. - horizontal) + sample(bottom + right) * horizontal;
                    output.push((upper * (1. - vertical) + lower * vertical + 0.5) as u8);
                }
            }
            output
        };
        Cutout {
            width,
            height,
            luma: resample(&self.luma),
            chroma_blue: resample(&self.chroma_blue),
            chroma_red: resample(&self.chroma_red),
            alpha: resample(&self.alpha),
        }
    }

    fn blend_onto(&self, target: &mut I420Buffer, left: usize, top: usize) {
        let (stride_y, stride_u, stride_v) = target.strides();
        let (plane_y, plane_u, plane_v) = target.data_mut();
        let mix = |below: u8, above: u8, alpha: u8| {
            let weight = u32::from(alpha);
            ((u32::from(below) * (255 - weight) + u32::from(above) * weight + 127) / 255) as u8
        };
        for row in 0..self.height {
            for column in 0..self.width {
                let alpha = self.alpha[row * self.width + column];
                if alpha == 0 {
                    continue;
                }
                let (x, y) = (left + column, top + row);
                let source = row * self.width + column;
                let luma = &mut plane_y[y * stride_y as usize + x];
                *luma = mix(*luma, self.luma[source], alpha);
                if x % 2 == 0 && y % 2 == 0 {
                    let chroma_index = (y / 2) * stride_u as usize + x / 2;
                    plane_u[chroma_index] = mix(plane_u[chroma_index], self.chroma_blue[source], alpha);
                    let chroma_index = (y / 2) * stride_v as usize + x / 2;
                    plane_v[chroma_index] = mix(plane_v[chroma_index], self.chroma_red[source], alpha);
                }
            }
        }
    }
}

pub struct StandoutOverlay {
    settings: Arc<BlurSettings>,
    scaled: Option<ScaledCutout>,
}

struct ScaledCutout {
    source: Arc<Cutout>,
    frame: (usize, usize),
    cutout: Cutout,
}

impl StandoutOverlay {
    pub fn new(settings: Arc<BlurSettings>) -> Self {
        StandoutOverlay { settings, scaled: None }
    }

    pub fn apply(&mut self, mut frame: I420Buffer) -> I420Buffer {
        if !self.settings.standout() {
            return frame;
        }
        let Some(source) = self.settings.cutout() else { return frame };
        let (frame_width, frame_height) = (frame.width() as usize, frame.height() as usize);
        let width = (frame_width as f32 * WIDTH_SHARE) as usize & !1;
        if width < MIN_TARGET_WIDTH {
            return frame;
        }
        let height = (width * source.height / source.width) & !1;
        let margin = (frame_width as f32 * MARGIN_SHARE) as usize;
        if height + margin >= frame_height || width + margin >= frame_width {
            return frame;
        }
        let fresh = self.scaled.as_ref().is_some_and(|scaled| Arc::ptr_eq(&scaled.source, &source) && scaled.frame == (frame_width, frame_height));
        if !fresh {
            self.scaled = Some(ScaledCutout { cutout: source.scaled_to(width, height), source, frame: (frame_width, frame_height) });
        }
        if let Some(scaled) = &self.scaled {
            let left = (frame_width - width - margin) & !1;
            let top = (frame_height - height - margin) & !1;
            scaled.cutout.blend_onto(&mut frame, left, top);
        }
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_frame(width: u32, height: u32, luma: u8) -> I420Buffer {
        let mut buffer = I420Buffer::new(width, height);
        let (stride_y, stride_u, stride_v) = buffer.strides();
        let (plane_y, plane_u, plane_v) = buffer.data_mut();
        for row in 0..height as usize {
            plane_y[row * stride_y as usize..][..width as usize].fill(luma);
        }
        for row in 0..height as usize / 2 {
            plane_u[row * stride_u as usize..][..width as usize / 2].fill(128);
            plane_v[row * stride_v as usize..][..width as usize / 2].fill(128);
        }
        buffer
    }

    fn full_mask(value: f32) -> Vec<f32> {
        vec![value; MASK_WIDTH * MASK_HEIGHT]
    }

    fn luma_at(buffer: &I420Buffer, x: usize, y: usize) -> u8 {
        buffer.data().0[y * buffer.strides().0 as usize + x]
    }

    fn overlay_with(person: f32) -> (StandoutOverlay, Arc<BlurSettings>) {
        let settings = Arc::new(BlurSettings::default());
        settings.set_standout(true);
        let mut camera = gray_frame(640, 360, 235);
        settings.set_cutout(Cutout::from_camera(&mut camera, &full_mask(person)).map(Arc::new));
        (StandoutOverlay::new(settings.clone()), settings)
    }

    #[test]
    fn the_person_lands_bottom_right_and_the_rest_of_the_screen_stays() {
        let (mut overlay, _) = overlay_with(1.);
        let frame = overlay.apply(gray_frame(1280, 720, 16));
        assert_eq!(luma_at(&frame, 100, 100), 16);
        assert!(luma_at(&frame, 1280 - 100, 720 - 100) > 200);
    }

    #[test]
    fn corners_are_rounded() {
        let (mut overlay, _) = overlay_with(1.);
        let frame = overlay.apply(gray_frame(1280, 720, 16));
        let width = (1280. * WIDTH_SHARE) as usize & !1;
        let left = (1280 - width - (1280. * MARGIN_SHARE) as usize) & !1;
        let top = (720 - ((width * 9 / 16) & !1) - (1280. * MARGIN_SHARE) as usize) & !1;
        assert_eq!(luma_at(&frame, left, top), 16);
        assert!(luma_at(&frame, left + width / 2, top + 20) > 200);
    }

    #[test]
    fn no_person_in_the_mask_changes_nothing() {
        let (mut overlay, _) = overlay_with(0.);
        let frame = overlay.apply(gray_frame(1280, 720, 16));
        assert_eq!(luma_at(&frame, 1280 - 100, 720 - 100), 16);
    }

    #[test]
    fn switched_off_or_without_a_cutout_the_frame_passes_through() {
        let (mut overlay, settings) = overlay_with(1.);
        settings.set_standout(false);
        assert_eq!(luma_at(&overlay.apply(gray_frame(1280, 720, 16)), 1280 - 100, 720 - 100), 16);
        settings.set_standout(true);
        settings.set_cutout(None);
        assert_eq!(luma_at(&overlay.apply(gray_frame(1280, 720, 16)), 1280 - 100, 720 - 100), 16);
    }

    #[test]
    fn a_missing_mask_gives_no_cutout() {
        let mut camera = gray_frame(640, 360, 100);
        assert!(Cutout::from_camera(&mut camera, &[]).is_none());
    }
}
