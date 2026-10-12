use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};

const MAX_ENCODED_BYTES: usize = 3_750_000;
const MAX_SOURCE_EDGE: u32 = 16_384;
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;
const JPEG_QUALITY: u8 = 85;
const MIN_SHRUNK_EDGE: u32 = 64;
const LARGE_SOURCE_EDGE: u32 = 4096;

#[derive(Debug, PartialEq, Eq)]
pub struct Downscaled {
    pub bytes: Vec<u8>,
    pub mime_type: &'static str,
    pub width: u32,
    pub height: u32,
}

pub fn downscale(source: &[u8], max_edge: u32) -> Result<Downscaled, String> {
    let mut reader = ImageReader::new(Cursor::new(source))
        .with_guessed_format()
        .map_err(|error| format!("cannot read image: {error}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| format!("cannot decode image: {error}"))?;
    let transparent = has_transparent_pixel(&decoded);
    let filter = if decoded.width().max(decoded.height()) > LARGE_SOURCE_EDGE {
        FilterType::Triangle
    } else {
        FilterType::Lanczos3
    };
    let mut current = decoded;
    let mut edge = max_edge;
    loop {
        if current.width().max(current.height()) > edge {
            current = current.resize(edge, edge, filter);
        }
        let bytes = encode(&current, transparent)?;
        if bytes.len() <= MAX_ENCODED_BYTES {
            return Ok(Downscaled {
                bytes,
                mime_type: if transparent {
                    "image/png"
                } else {
                    "image/jpeg"
                },
                width: current.width(),
                height: current.height(),
            });
        }
        let longest = current.width().max(current.height());
        if longest <= MIN_SHRUNK_EDGE {
            return Err("image is too large even after scaling down".to_owned());
        }
        edge = (longest * 3 / 4).max(MIN_SHRUNK_EDGE);
    }
}

fn has_transparent_pixel(image: &DynamicImage) -> bool {
    match image {
        DynamicImage::ImageLumaA8(buffer) => buffer.pixels().any(|pixel| pixel.0[1] < u8::MAX),
        DynamicImage::ImageRgba8(buffer) => buffer.pixels().any(|pixel| pixel.0[3] < u8::MAX),
        DynamicImage::ImageLumaA16(buffer) => buffer.pixels().any(|pixel| pixel.0[1] < u16::MAX),
        DynamicImage::ImageRgba16(buffer) => buffer.pixels().any(|pixel| pixel.0[3] < u16::MAX),
        DynamicImage::ImageRgba32F(buffer) => buffer.pixels().any(|pixel| pixel.0[3] < 1.0),
        _ => false,
    }
}

fn encode(image: &DynamicImage, transparent: bool) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    if transparent {
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .map_err(|error| error.to_string())?;
    } else {
        JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
            .encode_image(&image.to_rgb8())
            .map_err(|error| error.to_string())?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;

    fn png(width: u32, height: u32, alpha: u8) -> Vec<u8> {
        let source = RgbaImage::from_pixel(width, height, Rgba([200, 30, 30, alpha]));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(source)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn gif(frames: &[[u8; 3]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in frames {
                let frame = image::Frame::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([color[0], color[1], color[2], 255]),
                ));
                encoder.encode_frame(frame).unwrap();
            }
        }
        bytes
    }

    #[test]
    fn a_large_opaque_image_becomes_a_jpeg_within_the_edge() {
        let scaled = downscale(&png(400, 200, 255), 100).unwrap();
        assert_eq!(scaled.mime_type, "image/jpeg");
        assert_eq!((scaled.width, scaled.height), (100, 50));
        assert_eq!(&scaled.bytes[..2], [0xFF, 0xD8]);
    }

    #[test]
    fn a_small_image_is_not_enlarged() {
        let scaled = downscale(&png(40, 20, 255), 1568).unwrap();
        assert_eq!((scaled.width, scaled.height), (40, 20));
    }

    #[test]
    fn transparent_pixels_keep_a_png() {
        let scaled = downscale(&png(40, 20, 100), 1568).unwrap();
        assert_eq!(scaled.mime_type, "image/png");
        assert_eq!(&scaled.bytes[1..4], b"PNG");
    }

    #[test]
    fn an_animated_gif_gives_its_first_frame() {
        let scaled = downscale(&gif(&[[250, 0, 0], [0, 0, 250]]), 1568).unwrap();
        let decoded = image::load_from_memory(&scaled.bytes).unwrap().to_rgb8();
        let pixel = decoded.get_pixel(4, 4).0;
        assert!(pixel[0] > 200 && pixel[2] < 60, "{pixel:?}");
    }

    #[test]
    fn bytes_that_are_not_an_image_are_an_error() {
        assert!(downscale(b"not an image", 100).is_err());
    }
}
