pub const BADGE_SIZE: usize = 32;
const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
const GLYPH_SCALE: usize = 2;
const GLYPH_GAP: usize = 1;
const RED: [u8; 3] = [0xef, 0x44, 0x44];
const INK: [u8; 3] = [0x0a, 0x0a, 0x0a];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Badge {
    pub text: String,
}

pub fn badge_for(unread_chats: usize) -> Option<Badge> {
    if unread_chats == 0 {
        return None;
    }
    let text = if unread_chats >= 10 {
        "9+".to_owned()
    } else {
        unread_chats.to_string()
    };
    Some(Badge { text })
}

const GLYPHS: [[u8; GLYPH_HEIGHT]; 11] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
    [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
];

fn glyph_for(character: char) -> Option<&'static [u8; GLYPH_HEIGHT]> {
    match character {
        '0'..='9' => GLYPHS.get(character as usize - '0' as usize),
        '+' => GLYPHS.get(10),
        _ => None,
    }
}

pub fn text_width(text: &str) -> usize {
    let glyphs = text.chars().count();
    if glyphs == 0 {
        return 0;
    }
    glyphs * GLYPH_WIDTH * GLYPH_SCALE + (glyphs - 1) * GLYPH_GAP * GLYPH_SCALE
}

/// BGRA, premultiplied, top-down, `BADGE_SIZE` square.
pub fn render_badge(badge: &Badge) -> Vec<u8> {
    let mut pixels = vec![0u8; BADGE_SIZE * BADGE_SIZE * 4];
    let center = (BADGE_SIZE as f32 - 1.) / 2.;
    let radius = BADGE_SIZE as f32 / 2.;
    for row in 0..BADGE_SIZE {
        for column in 0..BADGE_SIZE {
            let distance = ((column as f32 - center).powi(2) + (row as f32 - center).powi(2)).sqrt();
            if distance <= radius - 0.5 {
                put(&mut pixels, column, row, RED);
            }
        }
    }
    let width = text_width(&badge.text);
    let left = (BADGE_SIZE - width.min(BADGE_SIZE)) / 2;
    let top = (BADGE_SIZE - GLYPH_HEIGHT * GLYPH_SCALE) / 2;
    for (index, character) in badge.text.chars().enumerate() {
        let Some(glyph) = glyph_for(character) else {
            continue;
        };
        let origin = left + index * (GLYPH_WIDTH + GLYPH_GAP) * GLYPH_SCALE;
        for (glyph_row, bits) in glyph.iter().enumerate() {
            for glyph_column in 0..GLYPH_WIDTH {
                if bits & (1 << (GLYPH_WIDTH - 1 - glyph_column)) == 0 {
                    continue;
                }
                for dy in 0..GLYPH_SCALE {
                    for dx in 0..GLYPH_SCALE {
                        put(
                            &mut pixels,
                            origin + glyph_column * GLYPH_SCALE + dx,
                            top + glyph_row * GLYPH_SCALE + dy,
                            INK,
                        );
                    }
                }
            }
        }
    }
    pixels
}

fn put(pixels: &mut [u8], column: usize, row: usize, rgb: [u8; 3]) {
    if column >= BADGE_SIZE || row >= BADGE_SIZE {
        return;
    }
    let offset = (row * BADGE_SIZE + column) * 4;
    pixels[offset..offset + 4].copy_from_slice(&[rgb[2], rgb[1], rgb[0], 0xff]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_unread_means_no_badge() {
        assert_eq!(badge_for(0), None);
    }

    #[test]
    fn counts_chats_and_caps_at_nine_plus() {
        assert_eq!(badge_for(3).unwrap().text, "3");
        assert_eq!(badge_for(9).unwrap().text, "9");
        assert_eq!(badge_for(10).unwrap().text, "9+");
        assert_eq!(badge_for(250).unwrap().text, "9+");
    }

    #[test]
    fn rendered_badge_has_fill_ink_and_transparent_corner() {
        let badge = badge_for(3).unwrap();
        let pixels = render_badge(&badge);
        assert_eq!(pixels.len(), BADGE_SIZE * BADGE_SIZE * 4);
        assert_eq!(pixels[3], 0);
        let has = |blue: u8, green: u8, red: u8| {
            pixels
                .chunks(4)
                .any(|pixel| pixel == [blue, green, red, 0xff])
        };
        assert!(has(0x44, 0x44, 0xef));
        assert!(has(0x0a, 0x0a, 0x0a));
    }

    #[test]
    fn two_glyphs_fit_the_badge() {
        assert!(text_width("9+") < BADGE_SIZE);
    }
}
