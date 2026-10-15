use std::io::Cursor;

use gpui_kit::ImageFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl InlineFormat {
    pub fn image_format(self) -> ImageFormat {
        match self {
            InlineFormat::Png => ImageFormat::Png,
            InlineFormat::Jpeg => ImageFormat::Jpeg,
            InlineFormat::Gif => ImageFormat::Gif,
            InlineFormat::Webp => ImageFormat::Webp,
        }
    }

    pub fn decoder_format(self) -> Option<image::ImageFormat> {
        match self {
            InlineFormat::Png => Some(image::ImageFormat::Png),
            InlineFormat::Jpeg => Some(image::ImageFormat::Jpeg),
            InlineFormat::Webp => Some(image::ImageFormat::WebP),
            InlineFormat::Gif => None,
        }
    }
}

pub fn sniff_inline_format(bytes: &[u8]) -> Option<InlineFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(InlineFormat::Png),
        [0xFF, 0xD8, 0xFF, ..] => Some(InlineFormat::Jpeg),
        [b'G', b'I', b'F', b'8', ..] => Some(InlineFormat::Gif),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(InlineFormat::Webp),
        _ => None,
    }
}

pub fn image_dimensions(format: InlineFormat, bytes: &[u8]) -> Option<(u32, u32)> {
    match format.decoder_format() {
        Some(decoder) => image::ImageReader::with_format(Cursor::new(bytes), decoder)
            .into_dimensions()
            .ok(),
        None => {
            let header = bytes.get(6..10)?;
            Some((
                u32::from(u16::from_le_bytes([header[0], header[1]])),
                u32::from(u16::from_le_bytes([header[2], header[3]])),
            ))
        }
    }
}

pub fn dimensions_of(bytes: &[u8]) -> Option<(u32, u32)> {
    image_dimensions(sniff_inline_format(bytes)?, bytes)
}
