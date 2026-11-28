use std::collections::HashSet;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use gpui_kit::{App, RenderImage};
use image::imageops::FilterType;
use image::{Frame, ImageFormat, ImageReader, RgbaImage};

const MAX_TEXTURES: usize = 8;
const SOURCE_SIDE: u32 = 240;
const SOURCE_LIMIT: u32 = 2 * SOURCE_SIDE;
pub const TEXTURE_MARGIN: u32 = 2;

pub struct AvatarPhoto {
    bytes: Vec<u8>,
    textures: Mutex<Vec<(u32, Arc<RenderImage>)>>,
    decoding: Mutex<HashSet<u32>>,
    failed: Mutex<HashSet<u32>>,
}

pub struct Texture {
    pub image: Arc<RenderImage>,
    pub evicted: Option<Arc<RenderImage>>,
}

impl AvatarPhoto {
    pub fn source_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        let (width, height) = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let bytes = if width.max(height) > SOURCE_LIMIT {
            shrunk_source(&bytes)?
        } else {
            bytes
        };
        Some(Self {
            bytes,
            textures: Mutex::new(Vec::new()),
            decoding: Mutex::new(HashSet::new()),
            failed: Mutex::new(HashSet::new()),
        })
    }

    pub fn cached_texture(&self, side: u32) -> Option<Texture> {
        let mut textures = self.textures.lock().ok()?;
        let position = textures.iter().position(|(cached, _)| *cached == side)?;
        let entry = textures.remove(position);
        let image = entry.1.clone();
        textures.push(entry);
        Some(Texture {
            image,
            evicted: None,
        })
    }

    pub fn schedule_texture(self: &Arc<Self>, side: u32, cx: &mut App) {
        if self
            .textures
            .lock()
            .ok()
            .is_some_and(|textures| textures.iter().any(|(cached, _)| *cached == side))
        {
            return;
        }
        if self
            .failed
            .lock()
            .ok()
            .is_some_and(|failed| failed.contains(&side))
        {
            return;
        }
        if self
            .decoding
            .lock()
            .ok()
            .is_none_or(|mut decoding| !decoding.insert(side))
        {
            return;
        }
        let photo = Arc::clone(self);
        let bytes = self.bytes.clone();
        cx.spawn(async move |cx| {
            let built = cx
                .background_executor()
                .spawn(async move {
                    square_texture(&bytes, side)
                        .map(|pixels| with_edge_margin(&pixels, TEXTURE_MARGIN))
                })
                .await;
            photo.store(side, built);
            let _ = cx.update(|cx| cx.refresh_windows());
        })
        .detach();
    }

    fn store(&self, side: u32, pixels: Option<image::RgbaImage>) {
        if let Some(mut decoding) = self.decoding.lock().ok() {
            decoding.remove(&side);
        }
        let Some(pixels) = pixels else {
            if let Some(mut failed) = self.failed.lock().ok() {
                failed.insert(side);
            }
            return;
        };
        let image = Arc::new(RenderImage::new(vec![Frame::new(pixels)]));
        let Some(mut textures) = self.textures.lock().ok() else {
            return;
        };
        if textures.iter().any(|(cached, _)| *cached == side) {
            return;
        }
        if textures.len() >= MAX_TEXTURES {
            textures.remove(0);
        }
        textures.push((side, image));
    }

    pub fn texture(&self, side: u32) -> Option<Texture> {
        let mut textures = self.textures.lock().ok()?;
        if let Some(position) = textures.iter().position(|(cached, _)| *cached == side) {
            let entry = textures.remove(position);
            let image = entry.1.clone();
            textures.push(entry);
            return Some(Texture {
                image,
                evicted: None,
            });
        }
        let pixels = with_edge_margin(&square_texture(&self.bytes, side)?, TEXTURE_MARGIN);
        let image = Arc::new(RenderImage::new(vec![Frame::new(pixels)]));
        let evicted = (textures.len() >= MAX_TEXTURES).then(|| textures.remove(0).1);
        textures.push((side, image.clone()));
        Some(Texture { image, evicted })
    }
}

fn shrunk_source(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut source = square_texture(bytes, SOURCE_SIDE)?;
    for pixel in source.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let mut encoded = Vec::new();
    source
        .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Png)
        .ok()?;
    Some(encoded)
}

pub fn device_span(start: f32, end: f32, scale_factor: f32) -> u32 {
    let snapped =
        round_half_toward_zero(end * scale_factor) - round_half_toward_zero(start * scale_factor);
    snapped.max(1.) as u32
}

fn round_half_toward_zero(value: f32) -> f32 {
    value.signum() * (value.abs() - 0.5).ceil()
}

/// The sprite atlas packs images without gaps and samples linearly, so the outer texels
/// of a photo blend with its neighbour. Repeated edge texels keep that blend inside the photo.
pub fn with_edge_margin(texture: &RgbaImage, margin: u32) -> RgbaImage {
    let (width, height) = texture.dimensions();
    RgbaImage::from_fn(width + 2 * margin, height + 2 * margin, |x, y| {
        let source_x = x.saturating_sub(margin).min(width - 1);
        let source_y = y.saturating_sub(margin).min(height - 1);
        *texture.get_pixel(source_x, source_y)
    })
}

/// Microsoft's small profile photos often carry a light outer row or column; the disc touches it at the edge midpoints.
const EDGE_TRIM_PER: u32 = 48;

pub fn square_texture(bytes: &[u8], side: u32) -> Option<RgbaImage> {
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let full = decoded.width().min(decoded.height());
    let trim = if full > 2 {
        (full / EDGE_TRIM_PER).max(1)
    } else {
        0
    };
    let crop = full - 2 * trim;
    let left = (decoded.width() - full) / 2 + trim;
    let top = (decoded.height() - full) / 2 + trim;
    let square = image::imageops::crop_imm(&decoded, left, top, crop, crop).to_image();
    let mut texture = if crop == side {
        square
    } else {
        image::imageops::resize(&square, side, side, FilterType::Lanczos3)
    };
    for pixel in texture.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(texture)
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn solid_jpeg(width: u32, height: u32) -> Vec<u8> {
        let source = RgbaImage::from_pixel(width, height, Rgba([200, 100, 50, 255]));
        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgba8(source)
            .to_rgb8()
            .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Jpeg)
            .unwrap();
        encoded
    }

    #[test]
    fn texture_matches_the_device_pixels_and_is_bgra() {
        let texture = square_texture(&solid_jpeg(240, 240), 39).unwrap();
        assert_eq!(texture.dimensions(), (39, 39));
        let Rgba([blue, _, red, alpha]) = *texture.get_pixel(19, 19);
        assert!(
            blue.abs_diff(50) <= 3 && red.abs_diff(200) <= 3,
            "{blue} {red}"
        );
        assert_eq!(alpha, 255);
    }

    #[test]
    fn the_margin_repeats_the_edge_texels() {
        let mut texture = RgbaImage::from_pixel(3, 3, Rgba([1, 2, 3, 255]));
        texture.put_pixel(0, 0, Rgba([9, 9, 9, 255]));
        let padded = with_edge_margin(&texture, 2);
        assert_eq!(padded.dimensions(), (7, 7));
        assert_eq!(*padded.get_pixel(0, 0), Rgba([9, 9, 9, 255]));
        assert_eq!(*padded.get_pixel(1, 2), Rgba([9, 9, 9, 255]));
        assert_eq!(*padded.get_pixel(6, 6), Rgba([1, 2, 3, 255]));
    }

    #[test]
    fn a_light_outer_edge_of_the_photo_is_trimmed() {
        let mut source = RgbaImage::from_pixel(48, 48, Rgba([40, 40, 40, 255]));
        for index in 0..48 {
            source.put_pixel(index, 0, Rgba([255, 255, 255, 255]));
            source.put_pixel(47, index, Rgba([255, 255, 255, 255]));
        }
        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Png)
            .unwrap();
        let texture = square_texture(&encoded, 32).unwrap();
        let Rgba([top, ..]) = *texture.get_pixel(16, 0);
        let Rgba([right, ..]) = *texture.get_pixel(31, 16);
        assert!(top < 80 && right < 80, "top {top}, right {right}");
    }

    #[test]
    fn non_square_photos_are_center_cropped() {
        assert_eq!(
            square_texture(&solid_jpeg(64, 40), 60)
                .unwrap()
                .dimensions(),
            (60, 60)
        );
    }

    #[test]
    fn device_span_follows_the_snapped_bounds() {
        assert_eq!(device_span(0., 26., 1.), 26);
        assert_eq!(device_span(0., 28., 1.5), 42);
        assert_eq!(device_span(10.5, 36.5, 1.5), 39);
        assert_eq!(device_span(10.1, 36.1, 1.5), 39);
        assert_eq!(device_span(0., 32., 1.25), 40);
        assert_eq!(device_span(0., 0.1, 1.), 1);
        assert_eq!(device_span(-3.5, 22.5, 1.), 25);
    }

    #[test]
    fn textures_are_cached_per_side_and_evict_the_oldest() {
        let photo = AvatarPhoto::from_bytes(solid_jpeg(48, 48)).unwrap();
        let first = photo.texture(32).unwrap();
        let again = photo.texture(32).unwrap();
        assert_eq!(first.image.id, again.image.id);
        assert!(again.evicted.is_none());
        for side in 33..33 + MAX_TEXTURES as u32 - 1 {
            assert!(photo.texture(side).unwrap().evicted.is_none());
        }
        let overflow = photo.texture(100).unwrap();
        assert_eq!(overflow.evicted.map(|image| image.id), Some(first.image.id));
    }

    #[test]
    fn large_originals_are_kept_at_the_source_side() {
        let photo = AvatarPhoto::from_bytes(solid_jpeg(1200, 900)).unwrap();
        let dimensions = image::load_from_memory(&photo.bytes)
            .unwrap()
            .to_rgba8()
            .dimensions();
        assert_eq!(dimensions, (SOURCE_SIDE, SOURCE_SIDE));
        let texture = photo.texture(20).unwrap().image;
        let raw = texture.as_bytes(0).unwrap().to_vec();
        let padded_side = 20 + 2 * TEXTURE_MARGIN;
        let Rgba([blue, _, red, _]) = *RgbaImage::from_raw(padded_side, padded_side, raw)
            .unwrap()
            .get_pixel(10, 10);
        assert!(
            blue.abs_diff(50) <= 3 && red.abs_diff(200) <= 3,
            "{blue} {red}"
        );
    }

    #[test]
    fn garbage_bytes_fall_back() {
        assert!(AvatarPhoto::from_bytes(b"not an image".to_vec()).is_none());
    }
}
