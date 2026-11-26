//! Accent presets for the Rusty Teams theme. A preset replaces the accent roles of the
//! hand-tuned palette and leaves its surfaces and text alone.

use std::sync::OnceLock;

use super::color_math::{contrast, oklch, readable};
use super::schemes::{Palette, RUSTY_TEAMS, Shade};

const NEAR_BLACK: u32 = 0x0f0f10;
const WHITE: u32 = 0xffffff;
const DROP_ALPHA: u32 = 0x14;
const DARK_SELECTION_ALPHA: u32 = 0x66;
const LIGHT_SELECTION_ALPHA: u32 = 0x40;
const AA: f64 = 4.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccentPreset {
    #[default]
    Rust,
    Amber,
    Emerald,
    Cyan,
    Blue,
    Violet,
    Pink,
    Red,
}

impl AccentPreset {
    pub const ALL: [AccentPreset; 8] = [
        AccentPreset::Rust,
        AccentPreset::Amber,
        AccentPreset::Emerald,
        AccentPreset::Cyan,
        AccentPreset::Blue,
        AccentPreset::Violet,
        AccentPreset::Pink,
        AccentPreset::Red,
    ];

    pub fn key(self) -> &'static str {
        match self {
            AccentPreset::Rust => "rust",
            AccentPreset::Amber => "amber",
            AccentPreset::Emerald => "emerald",
            AccentPreset::Cyan => "cyan",
            AccentPreset::Blue => "blue",
            AccentPreset::Violet => "violet",
            AccentPreset::Pink => "pink",
            AccentPreset::Red => "red",
        }
    }

    pub fn from_key(key: &str) -> Option<AccentPreset> {
        AccentPreset::ALL
            .into_iter()
            .find(|preset| preset.key() == key)
    }

    /// OKLCH chroma and hue of the preset. `Rust` keeps the hand-tuned palette.
    fn seed(self) -> Option<(f64, f64)> {
        match self {
            AccentPreset::Rust => None,
            AccentPreset::Amber => Some((0.16, 75.)),
            AccentPreset::Emerald => Some((0.15, 160.)),
            AccentPreset::Cyan => Some((0.12, 215.)),
            AccentPreset::Blue => Some((0.16, 255.)),
            AccentPreset::Violet => Some((0.17, 295.)),
            AccentPreset::Pink => Some((0.17, 350.)),
            AccentPreset::Red => Some((0.19, 25.)),
        }
    }

    pub(super) fn index(self) -> usize {
        AccentPreset::ALL
            .iter()
            .position(|preset| *preset == self)
            .unwrap_or(0)
    }

    pub(super) fn from_index(index: u8) -> AccentPreset {
        AccentPreset::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or_default()
    }

    /// The Rusty Teams palette of `shade` in this accent. High contrast never changes.
    pub fn palette(self, shade: Shade) -> &'static Palette {
        static DARK: [OnceLock<Palette>; 8] = [const { OnceLock::new() }; 8];
        static LIGHT: [OnceLock<Palette>; 8] = [const { OnceLock::new() }; 8];
        let (base, cache) = match shade {
            Shade::Dark => (&RUSTY_TEAMS.dark, &DARK),
            Shade::Light => (&RUSTY_TEAMS.light, &LIGHT),
            Shade::HighContrast => return RUSTY_TEAMS.palette(shade),
        };
        let Some((chroma, hue)) = self.seed() else {
            return base;
        };
        cache[self.index()].get_or_init(|| derive(base, chroma, hue))
    }
}

/// A solid accent and the label that stays readable on it, nudging the accent if neither
/// white nor near-black reaches AA.
fn accent_with_label(start: f64, chroma: f64, hue: f64, dark: bool) -> (u32, u32) {
    let step = if dark { 0.005 } else { -0.005 };
    let mut lightness = start;
    loop {
        let accent = oklch(lightness, chroma, hue);
        for label in [
            if dark { NEAR_BLACK } else { WHITE },
            if dark { WHITE } else { NEAR_BLACK },
        ] {
            if contrast(label, accent) >= AA {
                return (accent, label);
            }
        }
        lightness += step;
    }
}

fn derive(base: &Palette, chroma: f64, hue: f64) -> Palette {
    let dark = base.shade == Shade::Dark;
    let tone = |dark_value: f64, light_value: f64, share: f64| {
        oklch(
            if dark { dark_value } else { light_value },
            chroma * share,
            hue,
        )
    };
    let (accent, on_accent) = accent_with_label(if dark { 0.72 } else { 0.5 }, chroma, hue, dark);
    let bubble_own = tone(0.3, 0.93, if dark { 0.4 } else { 0.14 });
    let grounds = [
        base.background,
        base.surface,
        base.reply_surface,
        base.row_hover,
        base.surface_raised,
        base.popup,
        base.bubble_other,
        bubble_own,
    ];
    let readable_tone = |dark_value: f64, light_value: f64, share: f64, grounds: &[u32]| {
        let start = if dark { dark_value } else { light_value };
        readable(start, chroma * share, hue, grounds, AA, dark)
    };
    let accent_text = readable_tone(0.76, 0.48, 0.85, &grounds);
    let accent_soft = readable_tone(0.86, 0.42, 0.45, &grounds);
    let selection_alpha = if dark {
        DARK_SELECTION_ALPHA
    } else {
        LIGHT_SELECTION_ALPHA
    };
    Palette {
        accent,
        accent_text,
        accent_soft,
        accent_strong: tone(0.55, 0.44, 1.),
        on_accent,
        bubble_own,
        reaction_on_own: tone(0.37, 0.87, if dark { 0.4 } else { 0.2 }),
        mention_background: tone(0.27, 0.95, if dark { 0.25 } else { 0.1 }),
        mention_background_own: tone(0.24, 0.87, if dark { 0.4 } else { 0.2 }),
        own_meta: readable_tone(0.78, 0.42, 0.25, &[bubble_own]),
        own_read: readable_tone(0.86, 0.45, 0.6, &[bubble_own]),
        drop_background: accent << 8 | DROP_ALPHA,
        selection: accent << 8 | selection_alpha,
        rule_own: tone(0.4, 0.82, if dark { 0.4 } else { 0.25 }),
        toast_mention: tone(0.24, 0.95, if dark { 0.25 } else { 0.1 }),
        ..*base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_is_the_hand_tuned_palette() {
        assert_eq!(AccentPreset::Rust.palette(Shade::Dark), &RUSTY_TEAMS.dark);
        assert_eq!(AccentPreset::Rust.palette(Shade::Light), &RUSTY_TEAMS.light);
    }

    #[test]
    fn keys_round_trip() {
        for preset in AccentPreset::ALL {
            assert_eq!(AccentPreset::from_key(preset.key()), Some(preset));
        }
        assert_eq!(AccentPreset::from_key("mauve"), None);
    }

    #[test]
    fn every_preset_keeps_text_readable_in_both_modes() {
        for preset in AccentPreset::ALL {
            for shade in [Shade::Dark, Shade::Light] {
                let palette = preset.palette(shade);
                let chrome = [
                    palette.background,
                    palette.surface,
                    palette.reply_surface,
                    palette.row_hover,
                    palette.surface_raised,
                    palette.popup,
                    palette.bubble_other,
                    palette.bubble_own,
                ];
                for text in [palette.foreground, palette.text_soft, palette.accent_text] {
                    for ground in chrome {
                        assert!(
                            contrast(text, ground) >= AA,
                            "{preset:?} {shade:?}: #{text:06x} on #{ground:06x}"
                        );
                    }
                }
                assert!(
                    contrast(palette.on_accent, palette.accent) >= AA,
                    "{preset:?}"
                );
                for meta in [palette.own_meta, palette.own_read, palette.text_muted] {
                    assert!(
                        contrast(meta, palette.bubble_own) >= AA,
                        "{preset:?} {shade:?}: #{meta:06x} on the own bubble"
                    );
                }
            }
        }
    }

    #[test]
    fn presets_leave_surfaces_alone() {
        let palette = AccentPreset::Blue.palette(Shade::Dark);
        assert_eq!(palette.surface, RUSTY_TEAMS.dark.surface);
        assert_eq!(palette.foreground, RUSTY_TEAMS.dark.foreground);
        assert_ne!(palette.accent, RUSTY_TEAMS.dark.accent);
    }
}
