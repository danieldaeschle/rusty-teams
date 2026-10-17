use std::borrow::Cow;

use gpui_kit::component::elevation::{self, Elevation, ElevationTokens};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

mod schemes;

pub use schemes::{
    ColorScheme, HIGH_CONTRAST, Palette, SCHEMES, Shade, active_palette as palette,
    active_scheme as scheme, active_shade as shade, default_scheme, palettes, scheme_by_id,
    set_active_scheme, set_active_shade, set_thread_scheme, set_thread_shade,
};

const FONT_FAMILY: &str = "Google Sans Flex";
const MONO_FAMILIES: [&str; 2] = ["Cascadia Code", "Consolas"];
const BASE_FONT_SIZE: f32 = 14.;
const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../../app/assets/fonts/GoogleSansFlex-Regular.ttf"),
    include_bytes!("../../app/assets/fonts/GoogleSansFlex-SemiBold.ttf"),
    include_bytes!("../../app/assets/fonts/GoogleSansFlex-Bold.ttf"),
    include_bytes!("../../app/assets/fonts/GoogleSansFlex-Italic.ttf"),
];

const WHITE: u32 = 0xffffff;
const CLOSE_BUTTON_ICON: u32 = 0xffffff;

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
    let mode = if palette().shade.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = FONT_FAMILY.into();
        theme.font_size = px(BASE_FONT_SIZE);
        if let Some(mono) = mono {
            theme.mono_font_family = mono;
        }
        theme.radius_lg = px(10.);
        let colors = &mut theme.colors;
        colors.background = color(palette().background);
        colors.foreground = color(palette().foreground);
        colors.border = color(palette().border);
        colors.muted = color(palette().border);
        colors.muted_foreground = color(palette().text_muted);
        colors.sidebar = color(palette().surface);
        colors.sidebar_foreground = color(palette().foreground);
        colors.sidebar_border = color(palette().border);
        colors.sidebar_accent = color(palette().popup_hover);
        colors.sidebar_accent_foreground = color(palette().foreground);
        colors.sidebar_primary = color(palette().accent);
        colors.popover = color(palette().popup);
        colors.popover_foreground = color(palette().foreground);
        colors.accent = color(palette().surface_raised);
        colors.accent_foreground = color(palette().foreground);
        colors.secondary = color(palette().surface_raised);
        colors.secondary_hover = color(palette().border_strong);
        colors.secondary_active = color(palette().border_strong);
        colors.secondary_foreground = color(palette().foreground);
        colors.primary = color(palette().accent);
        colors.primary_hover = color(palette().accent_text);
        colors.primary_active = color(palette().accent_strong);
        colors.primary_foreground = color(palette().on_accent);
        colors.link = color(palette().accent_text);
        colors.link_hover = color(palette().accent_soft);
        colors.link_active = color(palette().accent_strong);
        colors.ring = color(palette().accent);
        colors.caret = color(palette().accent);
        colors.input = color(palette().border_strong);
        colors.selection = color_with_alpha(palette().selection);
        colors.title_bar = color(palette().background);
        colors.title_bar_border = color(palette().border);
        colors.status_bar = color(palette().background);
        colors.status_bar_border = color(palette().border);
        colors.success = color(palette().green);
        colors.warning = color(palette().amber);
        colors.danger = color(palette().close_button_hover);
        colors.danger_active = color(palette().close_button_pressed);
        colors.danger_foreground = color(CLOSE_BUTTON_ICON);
        colors.drop_target = color_with_alpha(palette().drop_background);
        colors.window_border = color(palette().border);
        colors.scrollbar_thumb = color(palette().border_strong);
        colors.scrollbar_thumb_hover = color(palette().text_faint);
        colors.list = color(palette().background);
        colors.list_hover = color(palette().row_hover);
        colors.list_active = color(palette().surface_raised);
    });
    cx.set_global(elevation_tokens());
}

fn elevation_tokens() -> ElevationTokens {
    let flyout = Elevation {
        fill: color(palette().popup),
        hover: color(palette().popup_hover),
        shadow: elevation::shadow16(),
    };
    let nested_flyout = Elevation {
        fill: color(palette().popup_nested),
        hover: color(palette().popup_nested_hover),
        shadow: elevation::shadow16(),
    };
    ElevationTokens {
        flyouts: vec![flyout.clone(), nested_flyout],
        dialog: Elevation {
            shadow: elevation::shadow64(),
            ..flyout.clone()
        },
        toast: Elevation {
            shadow: elevation::shadow8(),
            ..flyout
        },
    }
}

pub fn flyout_shadow() -> Vec<BoxShadow> {
    elevation::shadow16()
}

pub fn dialog_shadow() -> Vec<BoxShadow> {
    elevation::shadow64()
}

pub fn toast_shadow() -> Vec<BoxShadow> {
    elevation::shadow8()
}

pub fn popup_nested() -> Hsla {
    color(palette().popup_nested)
}

pub trait ElevatedSurface: Styled + Sized {
    fn flyout_surface(self) -> Self {
        self.bg(popup()).shadow(flyout_shadow())
    }

    fn nested_flyout_surface(self) -> Self {
        self.bg(popup_nested()).shadow(flyout_shadow())
    }

    fn dialog_surface(self) -> Self {
        self.bg(popup()).shadow(dialog_shadow())
    }

    fn toast_surface(self) -> Self {
        self.bg(popup()).shadow(toast_shadow())
    }
}

impl<T: Styled + Sized> ElevatedSurface for T {}

pub fn font_family() -> SharedString {
    FONT_FAMILY.into()
}

pub fn background() -> Hsla {
    color(palette().background)
}

pub fn background_hex() -> u32 {
    palette().background
}

pub fn toast_mention_fill() -> Hsla {
    color(palette().toast_mention)
}

pub fn surface() -> Hsla {
    color(palette().surface)
}

pub fn surface_raised() -> Hsla {
    color(palette().surface_raised)
}

pub fn row_hover() -> Hsla {
    color(palette().row_hover)
}

pub fn popup() -> Hsla {
    color(palette().popup)
}

pub fn popup_hover() -> Hsla {
    color(palette().popup_hover)
}

pub fn reply_surface() -> Hsla {
    color(palette().reply_surface)
}

pub fn border() -> Hsla {
    color(palette().border)
}

pub fn selection() -> Hsla {
    color_with_alpha(palette().selection)
}

pub fn border_strong() -> Hsla {
    color(palette().border_strong)
}

pub fn text() -> Hsla {
    color(palette().foreground)
}

pub fn text_strong() -> Hsla {
    color(palette().text_strong)
}

pub fn text_soft() -> Hsla {
    color(palette().text_soft)
}

pub fn text_muted() -> Hsla {
    color(palette().text_muted)
}

pub fn text_faint() -> Hsla {
    color(palette().text_faint)
}

pub fn accent() -> Hsla {
    color(palette().accent)
}

pub fn accent_text() -> Hsla {
    color(palette().accent_text)
}

pub fn accent_soft() -> Hsla {
    color(palette().accent_soft)
}

pub fn on_accent() -> Hsla {
    color(palette().on_accent)
}

pub fn bubble_own() -> Hsla {
    color(palette().bubble_own)
}

pub fn bubble_other() -> Hsla {
    color(palette().bubble_other)
}

pub fn reaction_on_own() -> Hsla {
    color(palette().reaction_on_own)
}

pub fn white() -> Hsla {
    color(WHITE)
}

pub fn mention_background(own: bool) -> Hsla {
    color(if own {
        palette().mention_background_own
    } else {
        palette().mention_background
    })
}

pub fn own_meta() -> Hsla {
    color(palette().own_meta)
}

pub fn own_read() -> Hsla {
    color(palette().own_read)
}

pub fn mention_text() -> Hsla {
    color(palette().accent_soft)
}

pub fn drop_background() -> Hsla {
    color_with_alpha(palette().drop_background)
}

pub fn code_surface() -> Hsla {
    color_with_alpha(palette().code_surface)
}

pub fn code_header_border() -> Hsla {
    color_with_alpha(palette().code_header_border)
}

pub fn inline_code_fill(own: bool) -> Hsla {
    color_with_alpha(if own {
        palette().inline_code_fill_own
    } else {
        palette().inline_code_fill
    })
}

pub fn quote_fill() -> Hsla {
    color_with_alpha(palette().quote_fill)
}

pub fn inline_code_border() -> Hsla {
    color_with_alpha(palette().inline_code_border)
}

pub fn table_header() -> Hsla {
    color_with_alpha(palette().table_header)
}

pub fn rule(own: bool) -> Hsla {
    if own {
        color(palette().rule_own)
    } else {
        border_strong()
    }
}

pub fn badge_muted() -> Hsla {
    color(palette().badge_muted)
}

pub fn green() -> Hsla {
    color(palette().green)
}

pub fn red() -> Hsla {
    color(palette().red)
}

pub fn red_soft() -> Hsla {
    color(palette().red_soft)
}

pub fn red_tint() -> Hsla {
    color(palette().red_tint)
}

pub fn amber() -> Hsla {
    color(palette().amber)
}

pub fn out_of_office() -> Hsla {
    color(palette().out_of_office)
}

pub fn meeting_avatar() -> Hsla {
    color(palette().meeting_avatar)
}

pub fn bot_avatar() -> Hsla {
    color(palette().bot_avatar)
}

pub fn avatar_color(index: usize) -> Hsla {
    let palette = palette().avatar_palette;
    color(palette[index % palette.len()])
}

pub fn avatar_palette_len() -> usize {
    palette().avatar_palette.len()
}

#[cfg(test)]
mod tests {
    use super::{
        SCHEMES, Shade, avatar_color, avatar_palette_len, color, elevation_tokens,
        set_thread_scheme, set_thread_shade,
    };

    #[test]
    fn avatar_colors_wrap_around_the_palette() {
        assert_eq!(avatar_color(0), avatar_color(avatar_palette_len()));
        assert_ne!(avatar_color(0), avatar_color(1));
    }

    #[test]
    fn flyout_over_a_dialog_takes_a_lighter_fill_than_the_dialog() {
        for scheme in SCHEMES {
            for shade in Shade::ALL {
                set_thread_scheme(Some(scheme.id));
                set_thread_shade(Some(shade));
                let palette = scheme.palette(shade);
                let tokens = elevation_tokens();
                assert_eq!(tokens.dialog.fill, color(palette.popup));
                assert_eq!(tokens.flyout_at(0).fill, color(palette.popup));
                let nested = tokens.flyout_at(1);
                assert_eq!(nested.fill, color(palette.popup_nested));
                assert_ne!(nested.hover, nested.fill);
                if shade == Shade::Dark {
                    assert!(nested.fill.l > tokens.dialog.fill.l);
                    assert!(nested.hover.l > nested.fill.l);
                    assert_ne!(nested.fill, color(palette.popup_hover));
                }
            }
        }
        set_thread_scheme(None);
        set_thread_shade(None);
    }

    #[test]
    fn fluent_shadow_blur_is_halved_for_gpui() {
        let shadow = super::flyout_shadow();
        assert_eq!(shadow[0].blur_radius, gpui_kit::px(1.));
        assert_eq!(shadow[1].offset.y, gpui_kit::px(8.));
        assert_eq!(shadow[1].blur_radius, gpui_kit::px(8.));
        assert_eq!(super::dialog_shadow()[1].blur_radius, gpui_kit::px(32.));
    }
}
