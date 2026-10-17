use std::borrow::Cow;

use gpui_kit::component::elevation::{self, Elevation, ElevationTokens};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

mod schemes;

pub use schemes::{
    ColorScheme, SCHEMES, active_scheme as scheme, default_scheme, scheme_by_id, set_active_scheme,
    set_thread_scheme,
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
const CODE_SURFACE: u32 = 0x0000003d;
const CODE_HEADER_BORDER: u32 = 0xffffff0f;
const INLINE_CODE_FILL: u32 = 0xffffff14;
const INLINE_CODE_FILL_OWN: u32 = 0x0000003d;
const QUOTE_FILL: u32 = 0x00000052;
const INLINE_CODE_BORDER: u32 = 0xffffff1a;
const TABLE_HEADER: u32 = 0xffffff0d;
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
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = FONT_FAMILY.into();
        theme.font_size = px(BASE_FONT_SIZE);
        if let Some(mono) = mono {
            theme.mono_font_family = mono;
        }
        theme.radius_lg = px(10.);
        let colors = &mut theme.colors;
        colors.background = color(scheme().background);
        colors.foreground = color(scheme().foreground);
        colors.border = color(scheme().border);
        colors.muted = color(scheme().border);
        colors.muted_foreground = color(scheme().text_muted);
        colors.sidebar = color(scheme().surface);
        colors.sidebar_foreground = color(scheme().foreground);
        colors.sidebar_border = color(scheme().border);
        colors.sidebar_accent = color(scheme().popup_hover);
        colors.sidebar_accent_foreground = color(scheme().foreground);
        colors.sidebar_primary = color(scheme().accent);
        colors.popover = color(scheme().popup);
        colors.popover_foreground = color(scheme().foreground);
        colors.accent = color(scheme().surface_raised);
        colors.accent_foreground = color(scheme().foreground);
        colors.secondary = color(scheme().surface_raised);
        colors.secondary_hover = color(scheme().border_strong);
        colors.secondary_active = color(scheme().border_strong);
        colors.secondary_foreground = color(scheme().foreground);
        colors.primary = color(scheme().accent);
        colors.primary_hover = color(scheme().accent_text);
        colors.primary_active = color(scheme().accent_strong);
        colors.primary_foreground = color(scheme().on_accent);
        colors.link = color(scheme().accent_text);
        colors.link_hover = color(scheme().accent_soft);
        colors.link_active = color(scheme().accent_strong);
        colors.ring = color(scheme().accent);
        colors.caret = color(scheme().accent);
        colors.input = color(scheme().border_strong);
        colors.selection = color_with_alpha(scheme().selection);
        colors.title_bar = color(scheme().background);
        colors.title_bar_border = color(scheme().border);
        colors.status_bar = color(scheme().background);
        colors.status_bar_border = color(scheme().border);
        colors.success = color(scheme().green);
        colors.warning = color(scheme().amber);
        colors.danger = color(scheme().close_button_hover);
        colors.danger_active = color(scheme().close_button_pressed);
        colors.danger_foreground = color(CLOSE_BUTTON_ICON);
        colors.drop_target = color_with_alpha(scheme().drop_background);
        colors.window_border = color(scheme().border);
        colors.scrollbar_thumb = color(scheme().border_strong);
        colors.scrollbar_thumb_hover = color(scheme().text_faint);
        colors.list = color(scheme().background);
        colors.list_hover = color(scheme().row_hover);
        colors.list_active = color(scheme().surface_raised);
    });
    cx.set_global(elevation_tokens());
}

fn elevation_tokens() -> ElevationTokens {
    let flyout = Elevation {
        fill: color(scheme().popup),
        hover: color(scheme().popup_hover),
        shadow: elevation::shadow16(),
    };
    let nested_flyout = Elevation {
        fill: color(scheme().popup_nested),
        hover: color(scheme().popup_nested_hover),
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
    color(scheme().popup_nested)
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
    color(scheme().background)
}

pub fn background_hex() -> u32 {
    scheme().background
}

pub fn toast_mention_fill() -> Hsla {
    color(scheme().toast_mention)
}

pub fn surface() -> Hsla {
    color(scheme().surface)
}

pub fn surface_raised() -> Hsla {
    color(scheme().surface_raised)
}

pub fn row_hover() -> Hsla {
    color(scheme().row_hover)
}

pub fn popup() -> Hsla {
    color(scheme().popup)
}

pub fn popup_hover() -> Hsla {
    color(scheme().popup_hover)
}

pub fn reply_surface() -> Hsla {
    color(scheme().reply_surface)
}

pub fn border() -> Hsla {
    color(scheme().border)
}

pub fn selection() -> Hsla {
    color_with_alpha(scheme().selection)
}

pub fn border_strong() -> Hsla {
    color(scheme().border_strong)
}

pub fn text() -> Hsla {
    color(scheme().foreground)
}

pub fn text_strong() -> Hsla {
    color(scheme().text_strong)
}

pub fn text_soft() -> Hsla {
    color(scheme().text_soft)
}

pub fn text_muted() -> Hsla {
    color(scheme().text_muted)
}

pub fn text_faint() -> Hsla {
    color(scheme().text_faint)
}

pub fn accent() -> Hsla {
    color(scheme().accent)
}

pub fn accent_text() -> Hsla {
    color(scheme().accent_text)
}

pub fn accent_soft() -> Hsla {
    color(scheme().accent_soft)
}

pub fn on_accent() -> Hsla {
    color(scheme().on_accent)
}

pub fn bubble_own() -> Hsla {
    color(scheme().bubble_own)
}

pub fn bubble_other() -> Hsla {
    color(scheme().bubble_other)
}

pub fn reaction_on_own() -> Hsla {
    color(scheme().reaction_on_own)
}

pub fn white() -> Hsla {
    color(WHITE)
}

pub fn mention_background(own: bool) -> Hsla {
    color(if own {
        scheme().mention_background_own
    } else {
        scheme().mention_background
    })
}

pub fn own_meta() -> Hsla {
    color(scheme().own_meta)
}

pub fn own_read() -> Hsla {
    color(scheme().own_read)
}

pub fn mention_text() -> Hsla {
    color(scheme().accent_soft)
}

pub fn drop_background() -> Hsla {
    color_with_alpha(scheme().drop_background)
}

pub fn code_surface() -> Hsla {
    color_with_alpha(CODE_SURFACE)
}

pub fn code_header_border() -> Hsla {
    color_with_alpha(CODE_HEADER_BORDER)
}

pub fn inline_code_fill(own: bool) -> Hsla {
    color_with_alpha(if own {
        INLINE_CODE_FILL_OWN
    } else {
        INLINE_CODE_FILL
    })
}

pub fn quote_fill() -> Hsla {
    color_with_alpha(QUOTE_FILL)
}

pub fn inline_code_border() -> Hsla {
    color_with_alpha(INLINE_CODE_BORDER)
}

pub fn table_header() -> Hsla {
    color_with_alpha(TABLE_HEADER)
}

pub fn rule(own: bool) -> Hsla {
    if own {
        color(scheme().rule_own)
    } else {
        border_strong()
    }
}

pub fn badge_muted() -> Hsla {
    color(scheme().badge_muted)
}

pub fn green() -> Hsla {
    color(scheme().green)
}

pub fn red() -> Hsla {
    color(scheme().red)
}

pub fn red_soft() -> Hsla {
    color(scheme().red_soft)
}

pub fn red_tint() -> Hsla {
    color(scheme().red_tint)
}

pub fn amber() -> Hsla {
    color(scheme().amber)
}

pub fn out_of_office() -> Hsla {
    color(scheme().out_of_office)
}

pub fn meeting_avatar() -> Hsla {
    color(scheme().meeting_avatar)
}

pub fn bot_avatar() -> Hsla {
    color(scheme().bot_avatar)
}

pub fn avatar_color(index: usize) -> Hsla {
    let palette = scheme().avatar_palette;
    color(palette[index % palette.len()])
}

pub fn avatar_palette_len() -> usize {
    scheme().avatar_palette.len()
}

#[cfg(test)]
mod tests {
    use super::{
        SCHEMES, avatar_color, avatar_palette_len, color, elevation_tokens, set_thread_scheme,
    };

    #[test]
    fn avatar_colors_wrap_around_the_palette() {
        assert_eq!(avatar_color(0), avatar_color(avatar_palette_len()));
        assert_ne!(avatar_color(0), avatar_color(1));
    }

    #[test]
    fn flyout_over_a_dialog_takes_a_lighter_fill_than_the_dialog() {
        for scheme in SCHEMES {
            set_thread_scheme(Some(scheme.id));
            let tokens = elevation_tokens();
            assert_eq!(tokens.dialog.fill, color(scheme.popup));
            assert_eq!(tokens.flyout_at(0).fill, color(scheme.popup));
            let nested = tokens.flyout_at(1);
            assert_eq!(nested.fill, color(scheme.popup_nested));
            assert!(nested.fill.l > tokens.dialog.fill.l);
            assert!(nested.hover.l > nested.fill.l);
            assert_ne!(nested.fill, color(scheme.popup_hover));
        }
        set_thread_scheme(None);
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
