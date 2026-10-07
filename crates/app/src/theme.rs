use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

const FONT_FAMILY: &str = "Google Sans Flex";
const MONO_FAMILIES: [&str; 2] = ["Cascadia Code", "Consolas"];
const BASE_FONT_SIZE: f32 = 14.;
const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/GoogleSansFlex-Regular.ttf"),
    include_bytes!("../assets/fonts/GoogleSansFlex-SemiBold.ttf"),
    include_bytes!("../assets/fonts/GoogleSansFlex-Bold.ttf"),
    include_bytes!("../assets/fonts/GoogleSansFlex-Italic.ttf"),
];

const BACKGROUND: u32 = 0x0f0f10;
const SURFACE: u32 = 0x161617;
const SURFACE_RAISED: u32 = 0x232325;
const ROW_HOVER: u32 = 0x1c1c1e;
const BORDER: u32 = 0x262628;
const BORDER_STRONG: u32 = 0x3f3f42;
const FOREGROUND: u32 = 0xf4f4f5;
const TEXT_STRONG: u32 = 0xe4e4e7;
const TEXT_SOFT: u32 = 0xd4d4d8;
const TEXT_MUTED: u32 = 0xa1a1a6;
const TEXT_FAINT: u32 = 0x8e8e93;
const ACCENT: u32 = 0xce6a3b;
const ACCENT_TEXT: u32 = 0xe08a5c;
const ACCENT_SOFT: u32 = 0xf5c4a5;
const ACCENT_TINT: u32 = 0xf5c4a5;
const ACCENT_STRONG: u32 = 0xa84f27;
const BUBBLE_OWN: u32 = 0x4f2a1b;
const BUBBLE_OTHER: u32 = 0x232325;
const MENTION_BACKGROUND: u32 = 0x3a2a23;
const MENTION_BACKGROUND_OWN: u32 = 0x3b1d11;
const OWN_META: u32 = 0xc9a28c;
const OWN_READ: u32 = 0xf5c4a5;
const DROP_BACKGROUND: u32 = 0xce6a3b14;
const CODE_BACKGROUND: u32 = 0x0b0b0c;
const BADGE_MUTED: u32 = 0x3f3f42;
const TOAST_MENTION: u32 = 0x2e1d15;
const GREEN: u32 = 0x22c55e;
const RED: u32 = 0xef4444;
const CLOSE_BUTTON_HOVER: u32 = 0xc42b1c;
const CLOSE_BUTTON_PRESSED: u32 = 0xb5271a;
const CLOSE_BUTTON_ICON: u32 = 0xffffff;
const RED_SOFT: u32 = 0xf87171;
const RED_TINT: u32 = 0xfca5a5;
const AMBER: u32 = 0xfbbf24;
const AVATAR_PALETTE: [u32; 6] = [0x9a4a22, 0x44606e, 0x8a3434, 0x4f6a3c, 0x6a4f7a, 0x2f6b63];

pub fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn color_with_alpha(hex: u32) -> Hsla {
    rgba(hex).into()
}

pub fn load_fonts(cx: &mut App) {
    let fonts = FONT_FILES
        .iter()
        .map(|bytes| Cow::Borrowed(*bytes))
        .collect();
    let _ = cx.text_system().add_fonts(fonts);
}

pub fn apply(cx: &mut App) {
    let installed = cx.text_system().all_font_names();
    let mono = MONO_FAMILIES
        .iter()
        .find(|family| installed.iter().any(|name| name == *family))
        .map(|family| SharedString::from(*family));
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = FONT_FAMILY.into();
        theme.font_size = px(BASE_FONT_SIZE);
        if let Some(mono) = mono {
            theme.mono_font_family = mono;
        }
        let colors = &mut theme.colors;
        colors.background = color(BACKGROUND);
        colors.foreground = color(FOREGROUND);
        colors.border = color(BORDER);
        colors.muted = color(BORDER);
        colors.muted_foreground = color(TEXT_MUTED);
        colors.sidebar = color(SURFACE);
        colors.sidebar_foreground = color(FOREGROUND);
        colors.sidebar_border = color(BORDER);
        colors.sidebar_accent = color(SURFACE_RAISED);
        colors.sidebar_accent_foreground = color(FOREGROUND);
        colors.sidebar_primary = color(ACCENT);
        colors.popover = color(SURFACE);
        colors.popover_foreground = color(FOREGROUND);
        colors.accent = color(SURFACE_RAISED);
        colors.accent_foreground = color(FOREGROUND);
        colors.secondary = color(SURFACE_RAISED);
        colors.secondary_hover = color(BORDER_STRONG);
        colors.secondary_active = color(BORDER_STRONG);
        colors.secondary_foreground = color(FOREGROUND);
        colors.primary = color(ACCENT);
        colors.primary_hover = color(ACCENT_TEXT);
        colors.primary_active = color(ACCENT_STRONG);
        colors.primary_foreground = color(BACKGROUND);
        colors.link = color(ACCENT_TEXT);
        colors.link_hover = color(ACCENT_SOFT);
        colors.link_active = color(ACCENT_STRONG);
        colors.ring = color(ACCENT);
        colors.caret = color(ACCENT);
        colors.input = color(BORDER_STRONG);
        colors.selection = color_with_alpha(MENTION_BACKGROUND);
        colors.title_bar = color(BACKGROUND);
        colors.title_bar_border = color(BORDER);
        colors.status_bar = color(BACKGROUND);
        colors.status_bar_border = color(BORDER);
        colors.success = color(GREEN);
        colors.warning = color(AMBER);
        colors.danger = color(CLOSE_BUTTON_HOVER);
        colors.danger_active = color(CLOSE_BUTTON_PRESSED);
        colors.danger_foreground = color(CLOSE_BUTTON_ICON);
        colors.drop_target = color_with_alpha(DROP_BACKGROUND);
        colors.window_border = color(BORDER);
        colors.scrollbar_thumb = color(BORDER_STRONG);
        colors.scrollbar_thumb_hover = color(TEXT_FAINT);
        colors.list = color(BACKGROUND);
        colors.list_hover = color(ROW_HOVER);
        colors.list_active = color(SURFACE_RAISED);
    });
}

pub fn background() -> Hsla {
    color(BACKGROUND)
}

pub fn toast_mention_fill() -> Hsla {
    color(TOAST_MENTION)
}

pub fn surface() -> Hsla {
    color(SURFACE)
}

pub fn surface_raised() -> Hsla {
    color(SURFACE_RAISED)
}

pub fn row_hover() -> Hsla {
    color(ROW_HOVER)
}

pub fn border() -> Hsla {
    color(BORDER)
}

pub fn border_strong() -> Hsla {
    color(BORDER_STRONG)
}

pub fn text() -> Hsla {
    color(FOREGROUND)
}

pub fn text_strong() -> Hsla {
    color(TEXT_STRONG)
}

pub fn text_soft() -> Hsla {
    color(TEXT_SOFT)
}

pub fn text_muted() -> Hsla {
    color(TEXT_MUTED)
}

pub fn text_faint() -> Hsla {
    color(TEXT_FAINT)
}

pub fn accent() -> Hsla {
    color(ACCENT)
}

pub fn accent_text() -> Hsla {
    color(ACCENT_TEXT)
}

pub fn accent_soft() -> Hsla {
    color(ACCENT_SOFT)
}

pub fn accent_tint() -> Hsla {
    color(ACCENT_TINT)
}

pub fn on_accent() -> Hsla {
    color(BACKGROUND)
}

pub fn bubble_own() -> Hsla {
    color(BUBBLE_OWN)
}

pub fn bubble_other() -> Hsla {
    color(BUBBLE_OTHER)
}

pub fn mention_background(own: bool) -> Hsla {
    color(if own {
        MENTION_BACKGROUND_OWN
    } else {
        MENTION_BACKGROUND
    })
}

pub fn own_meta() -> Hsla {
    color(OWN_META)
}

pub fn own_read() -> Hsla {
    color(OWN_READ)
}

pub fn mention_text() -> Hsla {
    color(ACCENT_SOFT)
}

pub fn drop_background() -> Hsla {
    color_with_alpha(DROP_BACKGROUND)
}

pub fn code_background() -> Hsla {
    color(CODE_BACKGROUND)
}

pub fn badge_muted() -> Hsla {
    color(BADGE_MUTED)
}

pub fn green() -> Hsla {
    color(GREEN)
}

pub fn red() -> Hsla {
    color(RED)
}

pub fn red_soft() -> Hsla {
    color(RED_SOFT)
}

pub fn red_tint() -> Hsla {
    color(RED_TINT)
}

pub fn amber() -> Hsla {
    color(AMBER)
}

pub fn avatar_color(index: usize) -> Hsla {
    color(AVATAR_PALETTE[index % AVATAR_PALETTE.len()])
}

pub fn avatar_palette_len() -> usize {
    AVATAR_PALETTE.len()
}

#[cfg(test)]
mod tests {
    use super::{avatar_color, avatar_palette_len};

    #[test]
    fn avatar_colors_wrap_around_the_palette() {
        assert_eq!(avatar_color(0), avatar_color(avatar_palette_len()));
        assert_ne!(avatar_color(0), avatar_color(1));
    }
}
