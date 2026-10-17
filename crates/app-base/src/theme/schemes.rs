use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, PartialEq, Eq)]
pub struct ColorScheme {
    pub id: &'static str,
    pub name: &'static str,
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
}

pub const RUSTY_TEAMS: ColorScheme = ColorScheme {
    id: "rusty-teams",
    name: "Rusty Teams",
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
    out_of_office: 0xc239b3,
    close_button_hover: 0xc42b1c,
    close_button_pressed: 0xb5271a,
    meeting_avatar: 0x4f52b2,
    bot_avatar: 0x3a96dd,
    avatar_palette: [0x9a4a22, 0x44606e, 0x8a3434, 0x4f6a3c, 0x6a4f7a, 0x2f6b63],
};

pub const TEAMS: ColorScheme = ColorScheme {
    id: "teams",
    name: "Teams",
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
    out_of_office: 0xc239b3,
    close_button_hover: 0xc4314b,
    close_button_pressed: 0xa72037,
    meeting_avatar: 0x4f52b2,
    bot_avatar: 0x3a96dd,
    avatar_palette: [0x9a4a22, 0x44606e, 0x8a3434, 0x4f6a3c, 0x6a4f7a, 0x2f6b63],
};

pub const SCHEMES: [&ColorScheme; 2] = [&RUSTY_TEAMS, &TEAMS];

static ACTIVE_SCHEME: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static THREAD_SCHEME: Cell<Option<usize>> = const { Cell::new(None) };
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

pub fn set_active_scheme(id: &str) {
    ACTIVE_SCHEME.store(position(id), Ordering::Relaxed);
}

pub fn set_thread_scheme(id: Option<&str>) {
    THREAD_SCHEME.with(|cell| cell.set(id.map(position)));
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

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
    fn teams_scheme_uses_the_teams_dark_brand_ramp() {
        assert_eq!(TEAMS.accent_text, 0x7f85f5);
        assert_eq!(TEAMS.bubble_own, 0x2f2f4a);
        assert_eq!(TEAMS.surface, 0x1f1f1f);
    }

    #[test]
    fn every_scheme_layers_its_surfaces_from_dark_to_light() {
        for scheme in SCHEMES {
            let layers = [
                scheme.background,
                scheme.surface,
                scheme.reply_surface,
                scheme.row_hover,
                scheme.surface_raised,
                scheme.popup,
                scheme.popup_hover,
            ];
            assert!(
                layers.windows(2).all(|pair| pair[0] < pair[1]),
                "{} surfaces are not ordered dark to light",
                scheme.id
            );
            assert!(scheme.popup_nested > scheme.popup, "{}", scheme.id);
            assert!(
                scheme.popup_nested_hover > scheme.popup_nested,
                "{}",
                scheme.id
            );
        }
    }

    #[test]
    fn a_thread_override_wins_over_the_process_scheme() {
        set_thread_scheme(Some("teams"));
        assert_eq!(active_scheme().id, "teams");
        set_thread_scheme(Some("unknown"));
        assert_eq!(active_scheme().id, "rusty-teams");
        set_thread_scheme(None);
    }
}
