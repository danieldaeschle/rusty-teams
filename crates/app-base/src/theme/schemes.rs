use std::cell::Cell;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shade {
    #[default]
    Dark,
    Light,
    HighContrast,
}

impl Shade {
    pub const ALL: [Shade; 3] = [Shade::Dark, Shade::Light, Shade::HighContrast];

    pub fn is_dark(self) -> bool {
        !matches!(self, Shade::Light)
    }

    fn index(self) -> u8 {
        match self {
            Shade::Dark => 0,
            Shade::Light => 1,
            Shade::HighContrast => 2,
        }
    }

    fn from_index(index: u8) -> Shade {
        match index {
            1 => Shade::Light,
            2 => Shade::HighContrast,
            _ => Shade::Dark,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Palette {
    pub shade: Shade,
    pub background: u32,
    pub surface: u32,
    pub surface_raised: u32,
    pub row_hover: u32,
    pub popup: u32,
    pub popup_hover: u32,
    pub popup_nested: u32,
    pub popup_nested_hover: u32,
    pub reply_surface: u32,
    pub border: u32,
    pub border_strong: u32,
    pub foreground: u32,
    pub text_strong: u32,
    pub text_soft: u32,
    pub text_muted: u32,
    pub text_faint: u32,
    pub accent: u32,
    pub accent_text: u32,
    pub accent_soft: u32,
    pub accent_strong: u32,
    pub on_accent: u32,
    pub bubble_own: u32,
    pub bubble_other: u32,
    pub reaction_on_own: u32,
    pub mention_background: u32,
    pub mention_background_own: u32,
    pub own_meta: u32,
    pub own_read: u32,
    pub drop_background: u32,
    pub selection: u32,
    pub rule_own: u32,
    pub badge_muted: u32,
    pub toast_mention: u32,
    pub green: u32,
    pub red: u32,
    pub red_soft: u32,
    pub red_tint: u32,
    pub amber: u32,
    pub out_of_office: u32,
    pub close_button_hover: u32,
    pub close_button_pressed: u32,
    pub meeting_avatar: u32,
    pub bot_avatar: u32,
    pub avatar_palette: [u32; 6],
    pub code_surface: u32,
    pub code_header_border: u32,
    pub inline_code_fill: u32,
    pub inline_code_fill_own: u32,
    pub quote_fill: u32,
    pub inline_code_border: u32,
    pub code_frame: u32,
    pub table_header: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ColorScheme {
    pub id: &'static str,
    pub name: &'static str,
    pub dark: Palette,
    pub light: Palette,
}

impl ColorScheme {
    pub fn palette(&self, shade: Shade) -> &Palette {
        match shade {
            Shade::Dark => &self.dark,
            Shade::Light => &self.light,
            Shade::HighContrast => &HIGH_CONTRAST,
        }
    }
}

const AVATAR_PALETTE: [u32; 6] = [0x9a4a22, 0x44606e, 0x8a3434, 0x4f6a3c, 0x6a4f7a, 0x2f6b63];

const DARK_BASE: Palette = Palette {
    shade: Shade::Dark,
    background: 0,
    surface: 0,
    surface_raised: 0,
    row_hover: 0,
    popup: 0,
    popup_hover: 0,
    popup_nested: 0,
    popup_nested_hover: 0,
    reply_surface: 0,
    border: 0,
    border_strong: 0,
    foreground: 0,
    text_strong: 0,
    text_soft: 0,
    text_muted: 0,
    text_faint: 0,
    accent: 0,
    accent_text: 0,
    accent_soft: 0,
    accent_strong: 0,
    on_accent: 0,
    bubble_own: 0,
    bubble_other: 0,
    reaction_on_own: 0,
    mention_background: 0,
    mention_background_own: 0,
    own_meta: 0,
    own_read: 0,
    drop_background: 0,
    selection: 0,
    rule_own: 0,
    badge_muted: 0,
    toast_mention: 0,
    green: 0,
    red: 0,
    red_soft: 0,
    red_tint: 0,
    amber: 0,
    out_of_office: 0xc239b3,
    close_button_hover: 0,
    close_button_pressed: 0,
    meeting_avatar: 0x4f52b2,
    bot_avatar: 0x3a96dd,
    avatar_palette: AVATAR_PALETTE,
    code_surface: 0x0000003d,
    code_header_border: 0xffffff0f,
    inline_code_fill: 0xffffff14,
    inline_code_fill_own: 0x0000003d,
    quote_fill: 0x00000052,
    inline_code_border: 0xffffff1a,
    code_frame: 0xffffff3d,
    table_header: 0xffffff0d,
};

const LIGHT_BASE: Palette = Palette {
    shade: Shade::Light,
    code_surface: 0x0000000a,
    code_header_border: 0x0000000f,
    inline_code_fill: 0x0000000d,
    inline_code_fill_own: 0x0000000f,
    quote_fill: 0x0000000a,
    inline_code_border: 0x00000014,
    code_frame: 0x0000002e,
    table_header: 0x00000008,
    ..DARK_BASE
};

pub const RUSTY_TEAMS: ColorScheme = ColorScheme {
    id: "rusty-teams",
    name: "Rusty Teams",
    dark: Palette {
        background: 0x0f0f10,
        surface: 0x161617,
        surface_raised: 0x232325,
        row_hover: 0x1c1c1e,
        popup: 0x2c2c2f,
        popup_hover: 0x38383c,
        popup_nested: 0x3a3a3e,
        popup_nested_hover: 0x47474c,
        reply_surface: 0x19191b,
        border: 0x262628,
        border_strong: 0x3f3f42,
        foreground: 0xf4f4f5,
        text_strong: 0xe4e4e7,
        text_soft: 0xd4d4d8,
        text_muted: 0xa1a1a6,
        text_faint: 0x8e8e93,
        accent: 0xce6a3b,
        accent_text: 0xe08a5c,
        accent_soft: 0xf5c4a5,
        accent_strong: 0xa84f27,
        on_accent: 0x0f0f10,
        bubble_own: 0x4f2a1b,
        bubble_other: 0x232325,
        reaction_on_own: 0x6b3a26,
        mention_background: 0x3a2a23,
        mention_background_own: 0x3b1d11,
        own_meta: 0xc9a28c,
        own_read: 0xf5c4a5,
        drop_background: 0xce6a3b14,
        selection: 0xce6a3b66,
        rule_own: 0x6a3a25,
        badge_muted: 0x3f3f42,
        toast_mention: 0x2e1d15,
        green: 0x22c55e,
        red: 0xef4444,
        red_soft: 0xf87171,
        red_tint: 0xfca5a5,
        amber: 0xfbbf24,
        close_button_hover: 0xc42b1c,
        close_button_pressed: 0xb5271a,
        ..DARK_BASE
    },
    light: Palette {
        background: 0xe9e9eb,
        surface: 0xf4f4f5,
        surface_raised: 0xffffff,
        row_hover: 0xfafafa,
        popup: 0xffffff,
        popup_hover: 0xf1f1f2,
        popup_nested: 0xffffff,
        popup_nested_hover: 0xebebed,
        reply_surface: 0xf9f9fa,
        border: 0xe2e2e5,
        border_strong: 0xd0d0d4,
        foreground: 0x18181b,
        text_strong: 0x27272a,
        text_soft: 0x3f3f46,
        text_muted: 0x5f5f66,
        text_faint: 0x6e6e76,
        accent: 0xb4501f,
        accent_text: 0xa84a1d,
        accent_soft: 0x8f3d17,
        accent_strong: 0x93421d,
        on_accent: 0xffffff,
        bubble_own: 0xfae6da,
        bubble_other: 0xffffff,
        reaction_on_own: 0xf2cfb9,
        mention_background: 0xfbebe1,
        mention_background_own: 0xf2cfb9,
        own_meta: 0x7a5444,
        own_read: 0xa84a1d,
        drop_background: 0xb4501f14,
        selection: 0xb4501f40,
        rule_own: 0xecc3a8,
        badge_muted: 0xd0d0d4,
        toast_mention: 0xfbebe1,
        green: 0x16a34a,
        red: 0xc81e1e,
        red_soft: 0xb91c1c,
        red_tint: 0x991b1b,
        amber: 0xc27c0e,
        close_button_hover: 0xc42b1c,
        close_button_pressed: 0xb5271a,
        ..LIGHT_BASE
    },
};

pub const TEAMS: ColorScheme = ColorScheme {
    id: "teams",
    name: "Teams",
    dark: Palette {
        background: 0x0a0a0a,
        surface: 0x1f1f1f,
        surface_raised: 0x2e2e2e,
        row_hover: 0x292929,
        popup: 0x333333,
        popup_hover: 0x3d3d3d,
        popup_nested: 0x424242,
        popup_nested_hover: 0x4d4d4d,
        reply_surface: 0x242424,
        border: 0x333333,
        border_strong: 0x3d3d3d,
        foreground: 0xffffff,
        text_strong: 0xe0e0e0,
        text_soft: 0xd6d6d6,
        text_muted: 0xadadad,
        text_faint: 0x999999,
        accent: 0x5b5fc7,
        accent_text: 0x7f85f5,
        accent_soft: 0xaab1fa,
        accent_strong: 0x444791,
        on_accent: 0xffffff,
        bubble_own: 0x2f2f4a,
        bubble_other: 0x292929,
        reaction_on_own: 0x383966,
        mention_background: 0x2f2f4a,
        mention_background_own: 0x3d3e78,
        own_meta: 0xadadad,
        own_read: 0xaab1fa,
        drop_background: 0x5b5fc714,
        selection: 0x5b5fc766,
        rule_own: 0x444791,
        badge_muted: 0x3d3d3d,
        toast_mention: 0x2f2f4a,
        green: 0x92c353,
        red: 0xc4314b,
        red_soft: 0xe37d80,
        red_tint: 0xf1bbbc,
        amber: 0xffaa44,
        close_button_hover: 0xc4314b,
        close_button_pressed: 0xa72037,
        ..DARK_BASE
    },
    light: Palette {
        background: 0xebebeb,
        surface: 0xf5f5f5,
        surface_raised: 0xffffff,
        row_hover: 0xfafafa,
        popup: 0xffffff,
        popup_hover: 0xf5f5f5,
        popup_nested: 0xffffff,
        popup_nested_hover: 0xf0f0f0,
        reply_surface: 0xfafafa,
        border: 0xe0e0e0,
        border_strong: 0xd1d1d1,
        foreground: 0x242424,
        text_strong: 0x333333,
        text_soft: 0x424242,
        text_muted: 0x616161,
        text_faint: 0x707070,
        accent: 0x5b5fc7,
        accent_text: 0x5b5fc7,
        accent_soft: 0x4f52b2,
        accent_strong: 0x444791,
        on_accent: 0xffffff,
        bubble_own: 0xe8ebfa,
        bubble_other: 0xffffff,
        reaction_on_own: 0xdce0fa,
        mention_background: 0xe8ebfa,
        mention_background_own: 0xc5cbfa,
        own_meta: 0x616161,
        own_read: 0x4f52b2,
        drop_background: 0x5b5fc714,
        selection: 0x5b5fc740,
        rule_own: 0xc5cbfa,
        badge_muted: 0xd1d1d1,
        toast_mention: 0xe8ebfa,
        green: 0x13a10e,
        red: 0xc4314b,
        red_soft: 0xb10e1c,
        red_tint: 0x8e192e,
        amber: 0xc27c0e,
        close_button_hover: 0xc4314b,
        close_button_pressed: 0xa72037,
        ..LIGHT_BASE
    },
};

pub const HIGH_CONTRAST: Palette = Palette {
    shade: Shade::HighContrast,
    background: 0x000000,
    surface: 0x000000,
    surface_raised: 0x000000,
    row_hover: 0x003b40,
    popup: 0x000000,
    popup_hover: 0x003b40,
    popup_nested: 0x000000,
    popup_nested_hover: 0x003b40,
    reply_surface: 0x000000,
    border: 0xffffff,
    border_strong: 0xffffff,
    foreground: 0xffffff,
    text_strong: 0xffffff,
    text_soft: 0xffffff,
    text_muted: 0xffffff,
    text_faint: 0x3ff23f,
    accent: 0x1aebff,
    accent_text: 0xffff00,
    accent_soft: 0x1aebff,
    accent_strong: 0x1aebff,
    on_accent: 0x000000,
    bubble_own: 0x002e33,
    bubble_other: 0x000000,
    reaction_on_own: 0x003b40,
    mention_background: 0x000000,
    mention_background_own: 0x000000,
    own_meta: 0xffffff,
    own_read: 0x1aebff,
    drop_background: 0x1aebff26,
    selection: 0x1aebff80,
    rule_own: 0xffffff,
    badge_muted: 0xffffff,
    toast_mention: 0x000000,
    green: 0x3ff23f,
    red: 0xff4d4d,
    red_soft: 0xff4d4d,
    red_tint: 0xff4d4d,
    amber: 0xffff00,
    close_button_hover: 0xc4314b,
    close_button_pressed: 0xa72037,
    code_surface: 0x000000ff,
    code_header_border: 0xffffffff,
    inline_code_fill: 0x000000ff,
    inline_code_fill_own: 0x000000ff,
    quote_fill: 0x000000ff,
    inline_code_border: 0xffffffff,
    code_frame: 0xffffffff,
    table_header: 0x000000ff,
    ..DARK_BASE
};

pub const SCHEMES: [&ColorScheme; 2] = [&RUSTY_TEAMS, &TEAMS];

static ACTIVE_SCHEME: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_SHADE: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static THREAD_SCHEME: Cell<Option<usize>> = const { Cell::new(None) };
    static THREAD_SHADE: Cell<Option<Shade>> = const { Cell::new(None) };
}

pub fn default_scheme() -> &'static ColorScheme {
    SCHEMES[0]
}

pub fn scheme_by_id(id: &str) -> Option<&'static ColorScheme> {
    SCHEMES.into_iter().find(|scheme| scheme.id == id)
}

fn position(id: &str) -> usize {
    SCHEMES
        .iter()
        .position(|scheme| scheme.id == id)
        .unwrap_or(0)
}

pub fn active_scheme() -> &'static ColorScheme {
    let index = THREAD_SCHEME
        .with(Cell::get)
        .unwrap_or_else(|| ACTIVE_SCHEME.load(Ordering::Relaxed));
    SCHEMES[index.min(SCHEMES.len() - 1)]
}

pub fn active_shade() -> Shade {
    THREAD_SHADE
        .with(Cell::get)
        .unwrap_or_else(|| Shade::from_index(ACTIVE_SHADE.load(Ordering::Relaxed)))
}

pub fn active_palette() -> &'static Palette {
    active_scheme().palette(active_shade())
}

pub fn set_active_scheme(id: &str) {
    ACTIVE_SCHEME.store(position(id), Ordering::Relaxed);
}

pub fn set_active_shade(shade: Shade) {
    ACTIVE_SHADE.store(shade.index(), Ordering::Relaxed);
}

pub fn set_thread_scheme(id: Option<&str>) {
    THREAD_SCHEME.with(|cell| cell.set(id.map(position)));
}

pub fn set_thread_shade(shade: Option<Shade>) {
    THREAD_SHADE.with(|cell| cell.set(shade));
}

pub fn palettes() -> impl Iterator<Item = (&'static ColorScheme, &'static Palette)> {
    SCHEMES.into_iter().flat_map(|scheme| {
        Shade::ALL
            .into_iter()
            .map(move |shade| (scheme, scheme.palette(shade)))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn channel(hex: u32, shift: u32) -> f64 {
        let value = f64::from((hex >> shift) & 0xff) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(hex: u32) -> f64 {
        0.2126 * channel(hex, 16) + 0.7152 * channel(hex, 8) + 0.0722 * channel(hex, 0)
    }

    fn contrast(first: u32, second: u32) -> f64 {
        let (first, second) = (luminance(first), luminance(second));
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn scheme_ids_and_names_are_unique() {
        let ids: HashSet<_> = SCHEMES.iter().map(|scheme| scheme.id).collect();
        let names: HashSet<_> = SCHEMES.iter().map(|scheme| scheme.name).collect();
        assert_eq!(ids.len(), SCHEMES.len());
        assert_eq!(names.len(), SCHEMES.len());
    }

    #[test]
    fn rusty_teams_is_the_default() {
        assert_eq!(default_scheme().id, "rusty-teams");
        assert_eq!(scheme_by_id("teams"), Some(&TEAMS));
        assert_eq!(scheme_by_id("solarized"), None);
    }

    #[test]
    fn teams_scheme_uses_the_teams_brand_ramp_in_both_shades() {
        assert_eq!(TEAMS.dark.accent_text, 0x7f85f5);
        assert_eq!(TEAMS.dark.bubble_own, 0x2f2f4a);
        assert_eq!(TEAMS.dark.surface, 0x1f1f1f);
        assert_eq!(TEAMS.light.accent, 0x5b5fc7);
        assert_eq!(TEAMS.light.bubble_own, 0xe8ebfa);
        assert_eq!(TEAMS.light.foreground, 0x242424);
        assert_eq!(TEAMS.light.background, 0xebebeb);
    }

    #[test]
    fn every_palette_names_its_own_shade() {
        for scheme in SCHEMES {
            for shade in Shade::ALL {
                assert_eq!(scheme.palette(shade).shade, shade, "{}", scheme.id);
            }
        }
    }

    #[test]
    fn dark_palettes_layer_their_surfaces_from_dark_to_light() {
        for scheme in SCHEMES {
            let palette = &scheme.dark;
            let layers = [
                palette.background,
                palette.surface,
                palette.reply_surface,
                palette.row_hover,
                palette.surface_raised,
                palette.popup,
                palette.popup_hover,
            ];
            assert!(
                layers.windows(2).all(|pair| pair[0] < pair[1]),
                "{} surfaces are not ordered dark to light",
                scheme.id
            );
            assert!(palette.popup_nested > palette.popup, "{}", scheme.id);
            assert!(
                palette.popup_nested_hover > palette.popup_nested,
                "{}",
                scheme.id
            );
        }
    }

    #[test]
    fn light_palettes_lift_islands_and_flyouts_off_the_window_ground() {
        for scheme in SCHEMES {
            let palette = &scheme.light;
            assert!(palette.background < palette.surface, "{}", scheme.id);
            assert!(palette.surface < palette.reply_surface, "{}", scheme.id);
            assert!(palette.surface < palette.popup, "{}", scheme.id);
            assert_ne!(palette.popup_hover, palette.popup, "{}", scheme.id);
            assert_ne!(
                palette.popup_nested_hover, palette.popup_nested,
                "{}",
                scheme.id
            );
            assert_ne!(palette.row_hover, palette.surface, "{}", scheme.id);
        }
    }

    #[test]
    fn text_stays_readable_on_every_surface_of_every_palette() {
        for (scheme, palette) in palettes() {
            let grounds = [
                palette.background,
                palette.surface,
                palette.reply_surface,
                palette.row_hover,
                palette.surface_raised,
                palette.popup,
                palette.bubble_other,
                palette.bubble_own,
            ];
            let checks = [
                ("foreground", palette.foreground, 4.5),
                ("text_soft", palette.text_soft, 4.5),
                ("text_muted", palette.text_muted, 3.),
                ("text_faint", palette.text_faint, 2.5),
                ("accent_text", palette.accent_text, 3.),
            ];
            for (name, text, minimum) in checks {
                for ground in grounds {
                    let ratio = contrast(text, ground);
                    assert!(
                        ratio >= minimum,
                        "{} {:?}: {name} #{text:06x} on #{ground:06x} is {ratio:.2}",
                        scheme.id,
                        palette.shade
                    );
                }
            }
            assert!(
                contrast(palette.on_accent, palette.accent) >= 4.5,
                "{} {:?}: on_accent",
                scheme.id,
                palette.shade
            );
            assert!(
                contrast(palette.own_meta, palette.bubble_own) >= 3.,
                "{} {:?}: own_meta",
                scheme.id,
                palette.shade
            );
        }
    }

    #[test]
    fn a_thread_override_wins_over_the_process_scheme() {
        set_thread_scheme(Some("teams"));
        assert_eq!(active_scheme().id, "teams");
        set_thread_shade(Some(Shade::Light));
        assert_eq!(active_palette(), &TEAMS.light);
        set_thread_shade(Some(Shade::HighContrast));
        assert_eq!(active_palette(), &HIGH_CONTRAST);
        set_thread_scheme(Some("unknown"));
        assert_eq!(active_scheme().id, "rusty-teams");
        set_thread_scheme(None);
        set_thread_shade(None);
    }
}
