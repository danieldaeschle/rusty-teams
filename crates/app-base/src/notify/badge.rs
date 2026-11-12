use std::sync::{Arc, OnceLock};

use resvg::{tiny_skia, usvg};

const FILL: &str = "#C42B1C";
const INK: &str = "#FFFFFF";
const FONT: &[u8] = include_bytes!("../../../app/assets/fonts/GoogleSansFlex-Bold.ttf");
const FONT_FAMILY: &str = "Google Sans Flex";
const SINGLE_GLYPH_SCALE: f32 = 0.68;
const WIDE_TEXT_SCALE: f32 = 0.58;
const BASELINE_SHIFT: f32 = 0.36;

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

fn options() -> &'static usvg::Options<'static> {
    static OPTIONS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_font_data(FONT.to_vec());
        usvg::Options {
            font_family: FONT_FAMILY.to_owned(),
            fontdb: Arc::new(fonts),
            ..usvg::Options::default()
        }
    })
}

fn badge_svg(text: &str, size: u32) -> String {
    let size = size as f32;
    let radius = size / 2.;
    let font_size = size
        * if text.chars().count() > 1 {
            WIDE_TEXT_SCALE
        } else {
            SINGLE_GLYPH_SCALE
        };
    let baseline = radius + font_size * BASELINE_SHIFT;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}"><circle cx="{radius}" cy="{radius}" r="{radius}" fill="{FILL}"/><text x="{radius}" y="{baseline}" text-anchor="middle" font-family="{FONT_FAMILY}" font-weight="700" font-size="{font_size}" fill="{INK}">{text}</text></svg>"#
    )
}

/// BGRA, premultiplied, top-down, `size` square.
pub fn render_badge(badge: &Badge, size: u32) -> Vec<u8> {
    let size = size.max(1);
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("badge size is not zero");
    if let Ok(tree) = usvg::Tree::from_str(&badge_svg(&badge.text, size), options()) {
        resvg::render(
            &tree,
            tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
    }
    let mut pixels = pixmap.take();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    pixels
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

    fn rendered(text: &str, size: u32) -> Vec<u8> {
        render_badge(
            &Badge {
                text: text.to_owned(),
            },
            size,
        )
    }

    #[test]
    fn rendered_badge_has_fill_ink_and_transparent_corner() {
        let pixels = rendered("3", 32);
        assert_eq!(pixels.len(), 32 * 32 * 4);
        assert_eq!(pixels[3], 0);
        let has = |blue: u8, green: u8, red: u8| {
            pixels
                .chunks(4)
                .any(|pixel| pixel == [blue, green, red, 0xff])
        };
        assert!(has(0x1c, 0x2b, 0xc4));
        assert!(has(0xff, 0xff, 0xff));
    }

    #[test]
    fn nine_plus_stays_inside_the_circle() {
        let size = 32;
        let pixels = rendered("9+", size);
        let ink_columns: Vec<u32> = (0..size)
            .filter(|column| {
                (0..size).any(|row| {
                    let offset = ((row * size + column) * 4) as usize;
                    pixels[offset..offset + 3] == [0xff, 0xff, 0xff]
                })
            })
            .collect();
        assert!(ink_columns.len() > 8);
        assert!(ink_columns.first().is_some_and(|column| *column >= 4));
        assert!(ink_columns.last().is_some_and(|column| *column <= size - 5));
    }
}
