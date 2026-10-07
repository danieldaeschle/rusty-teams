use std::io::Cursor;

use image::imageops::FilterType;
use image::{ImageFormat, Rgba, RgbaImage};

const TEXTURE_SIZE: u32 = 100;
const MARGIN: f32 = 2.;

pub fn circular_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let side = decoded.width().min(decoded.height());
    let left = (decoded.width() - side) / 2;
    let top = (decoded.height() - side) / 2;
    let square = image::imageops::crop_imm(&decoded, left, top, side, side).to_image();
    let mut resized =
        image::imageops::resize(&square, TEXTURE_SIZE, TEXTURE_SIZE, FilterType::Lanczos3);
    apply_circle_mask(&mut resized);
    let mut encoded = Vec::new();
    resized
        .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Png)
        .ok()?;
    Some(encoded)
}

fn apply_circle_mask(texture: &mut RgbaImage) {
    let center = TEXTURE_SIZE as f32 / 2.;
    let radius = center - MARGIN;
    for (x, y, pixel) in texture.enumerate_pixels_mut() {
        let distance =
            ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
        let coverage = (radius + 0.5 - distance).clamp(0., 1.);
        let Rgba([red, green, blue, alpha]) = *pixel;
        *pixel = Rgba([red, green, blue, (alpha as f32 * coverage).round() as u8]);
    }
}

pub fn visible_fraction() -> f32 {
    (TEXTURE_SIZE as f32 - 2. * MARGIN) / TEXTURE_SIZE as f32
}

#[cfg(test)]
mod tests {
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
    fn texture_edges_are_transparent_and_center_is_opaque() {
        let masked = image::load_from_memory(&circular_png(&solid_jpeg(48, 48)).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(masked.dimensions(), (TEXTURE_SIZE, TEXTURE_SIZE));
        let middle = TEXTURE_SIZE / 2;
        for (x, y) in [
            (middle, 0),
            (middle, TEXTURE_SIZE - 1),
            (0, middle),
            (TEXTURE_SIZE - 1, middle),
            (0, 0),
        ] {
            assert_eq!(
                masked.get_pixel(x, y)[3],
                0,
                "edge pixel {x},{y} must be transparent"
            );
        }
        assert_eq!(masked.get_pixel(middle, middle)[3], 255);
    }

    #[test]
    fn non_square_photos_are_center_cropped() {
        assert!(circular_png(&solid_jpeg(64, 40)).is_some());
    }

    #[test]
    fn garbage_bytes_fall_back() {
        assert!(circular_png(b"not an image").is_none());
    }
}
