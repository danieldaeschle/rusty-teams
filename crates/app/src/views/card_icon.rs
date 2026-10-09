use gpui_kit::assets::IconName;
use gpui_kit::*;
use teams_core::{CardActionIcon, CardIcon, IconSize};

use super::adaptive_card::card_color;
use super::widgets::icon;
use crate::theme;

const ACTION_ICON_SIZE: f32 = 16.;
const UNKNOWN_GLYPH_RATIO: f32 = 0.45;
const UNKNOWN_GLYPH_RADIUS: f32 = 2.;
const GLYPH_RATIO: f32 = 0.85;
const STYLE_SUFFIXES: [&str; 2] = ["regular", "filled"];

const FLUENT_ICONS: &[(&str, IconName)] = &[
    ("add", IconName::Plus),
    ("alert", IconName::Bell),
    ("arrowdown", IconName::ArrowDown),
    ("arrowup", IconName::ArrowUp),
    ("arrowleft", IconName::ArrowLeft),
    ("arrowright", IconName::ArrowRight),
    ("arrowdownload", IconName::Download),
    ("arrowupload", IconName::Upload),
    ("arrowsync", IconName::RefreshCw),
    ("arrowclockwise", IconName::RotateCw),
    ("arrowrepeatall", IconName::RefreshCw),
    ("attach", IconName::Paperclip),
    ("bookmark", IconName::Bookmark),
    ("briefcase", IconName::Briefcase),
    ("building", IconName::Building),
    ("calendar", IconName::Calendar),
    ("calendarltr", IconName::Calendar),
    ("calendardate", IconName::Calendar),
    ("calendarclock", IconName::CalendarClock),
    ("camera", IconName::Camera),
    ("chat", IconName::MessageSquare),
    ("comment", IconName::MessageSquare),
    ("commentmultiple", IconName::MessageSquare),
    ("chartmultiple", IconName::ChartBar),
    ("checkmark", IconName::Check),
    ("checkmarkcircle", IconName::CircleCheck),
    ("chevrondown", IconName::ChevronDown),
    ("chevronup", IconName::ChevronUp),
    ("chevronleft", IconName::ChevronLeft),
    ("chevronright", IconName::ChevronRight),
    ("clipboard", IconName::Clipboard),
    ("clock", IconName::Clock),
    ("cloud", IconName::Cloud),
    ("code", IconName::Code),
    ("copy", IconName::Copy),
    ("database", IconName::Database),
    ("delete", IconName::Trash),
    ("dismiss", IconName::X),
    ("dismisscircle", IconName::CircleX),
    ("document", IconName::File),
    ("documenttext", IconName::FileText),
    ("edit", IconName::Pencil),
    ("errorcircle", IconName::CircleAlert),
    ("eye", IconName::Eye),
    ("eyeoff", IconName::EyeOff),
    ("flag", IconName::Flag),
    ("flash", IconName::Zap),
    ("folder", IconName::Folder),
    ("gift", IconName::Gift),
    ("globe", IconName::Globe),
    ("heart", IconName::Heart),
    ("home", IconName::House),
    ("image", IconName::Image),
    ("info", IconName::Info),
    ("key", IconName::Key),
    ("link", IconName::Link),
    ("list", IconName::List),
    ("location", IconName::MapPin),
    ("lock", IconName::Lock),
    ("lockclosed", IconName::Lock),
    ("mail", IconName::Mail),
    ("megaphone", IconName::Megaphone),
    ("mic", IconName::Mic),
    ("money", IconName::DollarSign),
    ("open", IconName::ExternalLink),
    ("people", IconName::Users),
    ("peoplecommunity", IconName::Users),
    ("person", IconName::User),
    ("personadd", IconName::UserPlus),
    ("personcircle", IconName::CircleUser),
    ("phone", IconName::Phone),
    ("pin", IconName::Pin),
    ("play", IconName::Play),
    ("question", IconName::CircleQuestionMark),
    ("questioncircle", IconName::CircleQuestionMark),
    ("rocket", IconName::Rocket),
    ("search", IconName::Search),
    ("send", IconName::Send),
    ("settings", IconName::Settings),
    ("share", IconName::Share),
    ("shield", IconName::Shield),
    ("shieldcheckmark", IconName::ShieldCheck),
    ("star", IconName::Star),
    ("table", IconName::Table),
    ("tag", IconName::Tag),
    ("target", IconName::Target),
    ("thumbdislike", IconName::ThumbsDown),
    ("thumblike", IconName::ThumbsUp),
    ("timer", IconName::Timer),
    ("trophy", IconName::Trophy),
    ("video", IconName::Video),
    ("warning", IconName::TriangleAlert),
    ("wrench", IconName::Wrench),
    ("task", IconName::ClipboardCheck),
];

pub fn fluent_icon(name: &str) -> Option<IconName> {
    let mut key = name.trim().to_ascii_lowercase();
    for suffix in STYLE_SUFFIXES {
        if let Some(stripped) = key.strip_suffix(suffix) {
            key = stripped.to_owned();
        }
    }
    let key = key.trim_end_matches(|character: char| character.is_ascii_digit());
    FLUENT_ICONS
        .iter()
        .find(|(known, _)| *known == key)
        .map(|(_, glyph)| *glyph)
}

pub fn icon_pixels(size: IconSize) -> f32 {
    match size {
        IconSize::ExtraExtraSmall => 12.,
        IconSize::ExtraSmall => 16.,
        IconSize::Small => 20.,
        IconSize::Standard => 24.,
        IconSize::Medium => 32.,
        IconSize::Large => 48.,
        IconSize::ExtraLarge => 64.,
        IconSize::ExtraExtraLarge => 72.,
    }
}

pub fn icon_view(card_icon: &CardIcon) -> AnyElement {
    glyph_view(
        &card_icon.name,
        icon_pixels(card_icon.size),
        card_color(card_icon.color, false),
    )
}

pub fn action_icon_view(action_icon: &CardActionIcon) -> AnyElement {
    match action_icon {
        CardActionIcon::Named(name) => glyph_view(name, ACTION_ICON_SIZE, theme::accent_text()),
        CardActionIcon::Url(url) => img(url.clone())
            .size(px(ACTION_ICON_SIZE))
            .flex_none()
            .object_fit(ObjectFit::Contain)
            .with_loading(|| div().into_any_element())
            .with_fallback(|| div().into_any_element())
            .into_any_element(),
    }
}

pub(super) fn glyph_view(name: &str, size: f32, color: Hsla) -> AnyElement {
    let frame = div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();
    match fluent_icon(name) {
        Some(glyph) => frame
            .child(icon(glyph, size * GLYPH_RATIO, color))
            .into_any_element(),
        None => frame
            .child(
                div()
                    .size(px(size * UNKNOWN_GLYPH_RATIO))
                    .rounded(px(UNKNOWN_GLYPH_RADIUS))
                    .bg(theme::text_muted()),
            )
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::assets::IconName;
    use teams_core::IconSize;

    use super::{fluent_icon, icon_pixels};

    #[test]
    fn fluent_names_map_ignoring_case_style_and_size_suffixes() {
        assert_eq!(fluent_icon("Calendar"), Some(IconName::Calendar));
        assert_eq!(fluent_icon("MailRegular"), Some(IconName::Mail));
        assert_eq!(fluent_icon("Alert24"), Some(IconName::Bell));
        assert_eq!(fluent_icon("NoSuchIcon"), None);
    }

    #[test]
    fn sizes_grow_with_the_scale() {
        let sizes = [
            IconSize::ExtraExtraSmall,
            IconSize::ExtraSmall,
            IconSize::Small,
            IconSize::Standard,
            IconSize::Medium,
            IconSize::Large,
            IconSize::ExtraLarge,
            IconSize::ExtraExtraLarge,
        ];
        let pixels: Vec<f32> = sizes.into_iter().map(icon_pixels).collect();
        assert!(pixels.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
