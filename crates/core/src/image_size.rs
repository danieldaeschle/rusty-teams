pub struct ImageInfo {
    pub content_type: &'static str,
    pub width: u32,
    pub height: u32,
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub fn sniff(bytes: &[u8]) -> Option<ImageInfo> {
    png(bytes).or_else(|| gif(bytes)).or_else(|| jpeg(bytes))
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn png(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 24 || !bytes.starts_with(PNG_SIGNATURE) {
        return None;
    }
    Some(ImageInfo {
        content_type: "image/png",
        width: be32(&bytes[16..20]),
        height: be32(&bytes[20..24]),
    })
}

fn gif(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 10 || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return None;
    }
    Some(ImageInfo {
        content_type: "image/gif",
        width: u32::from(u16::from_le_bytes([bytes[6], bytes[7]])),
        height: u32::from(u16::from_le_bytes([bytes[8], bytes[9]])),
    })
}

fn jpeg(bytes: &[u8]) -> Option<ImageInfo> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut position = 2;
    while position + 4 <= bytes.len() {
        if bytes[position] != 0xff {
            position += 1;
            continue;
        }
        let marker = bytes[position + 1];
        if marker == 0xff {
            position += 1;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            bytes[position + 2],
            bytes[position + 3],
        ]));
        let is_frame = matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if is_frame && position + 9 <= bytes.len() {
            return Some(ImageInfo {
                content_type: "image/jpeg",
                height: u32::from(u16::from_be_bytes([
                    bytes[position + 5],
                    bytes[position + 6],
                ])),
                width: u32::from(u16::from_be_bytes([
                    bytes[position + 7],
                    bytes[position + 8],
                ])),
            });
        }
        position += 2 + length;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_size() {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend([0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 1, 0, 0, 0, 0, 50]);
        let info = sniff(&bytes).unwrap();
        assert_eq!(
            (info.content_type, info.width, info.height),
            ("image/png", 256, 50)
        );
    }

    #[test]
    fn reads_gif_size() {
        let info = sniff(b"GIF89a\x10\x00\x20\x00\x00").unwrap();
        assert_eq!(
            (info.content_type, info.width, info.height),
            ("image/gif", 16, 32)
        );
    }

    #[test]
    fn reads_jpeg_size_after_other_segments() {
        let bytes = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0, 0, 0xff, 0xc0, 0x00, 0x0b, 8, 0x00, 0x40, 0x00,
            0x80, 3, 0,
        ];
        let info = sniff(&bytes).unwrap();
        assert_eq!(
            (info.content_type, info.width, info.height),
            ("image/jpeg", 128, 64)
        );
    }

    #[test]
    fn unknown_data_has_no_size() {
        assert!(sniff(b"not an image at all, really").is_none());
        assert!(sniff(&[]).is_none());
    }
}
