use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings_section::SettingsSection;
use crate::theme;
use crate::views::widgets::icon;

pub const TEAMS_GROUP: [SettingsSection; 6] = [
    SettingsSection::General,
    SettingsSection::Notifications,
    SettingsSection::Appearance,
    SettingsSection::Chats,
    SettingsSection::Calls,
    SettingsSection::Devices,
];
pub const RUSTY_GROUP: [SettingsSection; 2] = [SettingsSection::Agents, SettingsSection::Mcp];
const ITEM_MIN_HEIGHT: f32 = 32.;
const ITEM_PADDING_Y: f32 = 6.;
const ITEM_LINE_HEIGHT: f32 = 20.;
const ITEM_GAP: f32 = 2.;
const ITEM_RADIUS: f32 = 6.;
const ITEM_ICON: f32 = 16.;
const NAV_PADDING_X: f32 = 12.;
const NO_RESULTS: &str = "No settings found";

fn search_terms(section: SettingsSection) -> &'static [&'static str] {
    match section {
        SettingsSection::General => &[
            "Auto-start Teams",
            "Open application in background",
            "On close, keep the application running",
            "Up arrow key behaviour in compose box",
            "Translate messages into this language",
            "How to handle messages in other languages",
            "Never translate messages in these languages",
        ],
        SettingsSection::Notifications => &[
            "Chat message notifications",
            "Meeting chat notifications",
            "Likes and reactions",
            "Teams and channels",
            "Show message preview",
            "Notification sound",
            "Urgent sound",
            "Position on the screen",
            "Flash taskbar when notifications are received",
            "Hide notifications during calls and meetings",
            "Send test notification",
        ],
        SettingsSection::Appearance => &["Theme", "Color scheme", "App icon"],
        SettingsSection::Chats => &[
            "Message density",
            "Read receipts",
            "Show message actions on right click",
            "Show muted items in their own section",
            "When writing a message, press Enter to",
            "Format messages using mark-down",
        ],
        SettingsSection::Calls => &["Ringtones"],
        SettingsSection::Devices => &[
            "Speaker",
            "Microphone",
            "Noise suppression",
            "Music mode",
            "Camera",
            "Background",
            "Soft focus",
            "Adjust brightness",
            "Mirror my video",
            "Make a test call",
            "Save a test report after each call",
        ],
        SettingsSection::Agents => &["Claude", "Agent runs"],
        SettingsSection::Mcp => &["Model Context Protocol", "Conversation access"],
        SettingsSection::About => &["Version", "Build"],
    }
}

pub fn matches(section: SettingsSection, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || std::iter::once(label(section))
            .chain(search_terms(section).iter().copied())
            .any(|text| text.to_lowercase().contains(&query))
}

pub fn first_match(query: &str, agents_available: bool) -> Option<SettingsSection> {
    TEAMS_GROUP
        .into_iter()
        .chain(RUSTY_GROUP)
        .chain([SettingsSection::About])
        .filter(|section| agents_available || *section != SettingsSection::Agents)
        .find(|section| matches(*section, query))
}

pub fn label(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::General => "General",
        SettingsSection::Notifications => "Notifications and activity",
        SettingsSection::Appearance => "Appearance and accessibility",
        SettingsSection::Chats => "Chats and channels",
        SettingsSection::Calls => "Calls",
        SettingsSection::Devices => "Devices",
        SettingsSection::Agents => "Agents",
        SettingsSection::Mcp => "MCP",
        SettingsSection::About => "About",
    }
}

fn icon_name(section: SettingsSection) -> IconName {
    match section {
        SettingsSection::General => IconName::SlidersHorizontal,
        SettingsSection::Notifications => IconName::Bell,
        SettingsSection::Appearance => IconName::Palette,
        SettingsSection::Chats => IconName::MessageSquare,
        SettingsSection::Calls => IconName::Phone,
        SettingsSection::Devices => IconName::Headphones,
        SettingsSection::Agents => IconName::Bot,
        SettingsSection::Mcp => IconName::Plug,
        SettingsSection::About => IconName::Info,
    }
}

pub fn item_selector(section: SettingsSection) -> String {
    format!("settings/nav/item/{}", element_id(section))
}

pub fn label_selector(section: SettingsSection) -> String {
    format!("{}/label", item_selector(section))
}

fn element_id(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::General => "settings-nav-general",
        SettingsSection::Notifications => "settings-nav-notifications",
        SettingsSection::Appearance => "settings-nav-appearance",
        SettingsSection::Chats => "settings-nav-chats",
        SettingsSection::Calls => "settings-nav-calls",
        SettingsSection::Devices => "settings-nav-devices",
        SettingsSection::Agents => "settings-nav-agents",
        SettingsSection::Mcp => "settings-nav-mcp",
        SettingsSection::About => "settings-nav-about",
    }
}

pub struct NavLayout {
    pub width: f32,
    pub padding_y: f32,
}

pub struct NavSearch<'a> {
    pub input: AnyElement,
    pub query: &'a str,
}

pub fn render_nav(
    active: SettingsSection,
    agents_available: bool,
    layout: NavLayout,
    search: NavSearch<'_>,
    on_select: impl Fn(SettingsSection, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    let item = |section: SettingsSection| nav_item(section, section == active, on_select.clone());
    let query = search.query;
    let teams: Vec<Stateful<Div>> = TEAMS_GROUP
        .into_iter()
        .filter(|section| matches(*section, query))
        .map(item)
        .collect();
    let rusty: Vec<Stateful<Div>> = RUSTY_GROUP
        .into_iter()
        .filter(|section| agents_available || *section != SettingsSection::Agents)
        .filter(|section| matches(*section, query))
        .map(item)
        .collect();
    let about_shown = matches(SettingsSection::About, query);
    let nothing_found = teams.is_empty() && rusty.is_empty() && !about_shown;
    let group = |items: Vec<Stateful<Div>>| {
        v_flex()
            .w_full()
            .px(px(NAV_PADDING_X))
            .gap(px(ITEM_GAP))
            .children(items)
    };
    v_flex()
        .debug_selector(|| "settings/nav".into())
        .w(px(layout.width))
        .h_full()
        .flex_none()
        .pt(px(layout.padding_y))
        .pb(px(NAV_PADDING_X))
        .gap(px(ITEM_GAP))
        .rounded_l(px(super::dialog::PANEL_RADIUS))
        .bg(theme::surface_raised())
        .child(
            div()
                .mx(px(2. * NAV_PADDING_X))
                .mb(px(18.))
                .text_size(px(20.))
                .line_height(px(26.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child("Settings"),
        )
        .child(
            div()
                .debug_selector(|| "settings/nav/search".into())
                .w_full()
                .px(px(NAV_PADDING_X))
                .pb(px(10.))
                .child(search.input),
        )
        .child(
            v_flex()
                .id("settings-nav-groups")
                .w_full()
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scrollbar()
                .gap(px(ITEM_GAP))
                .child(group(teams))
                .when(nothing_found, |groups| {
                    groups.child(
                        div()
                            .px(px(2. * NAV_PADDING_X))
                            .py(px(ITEM_PADDING_Y))
                            .text_size(px(13.))
                            .text_color(theme::text_muted())
                            .child(NO_RESULTS),
                    )
                })
                .when(!rusty.is_empty(), |groups| {
                    groups
                        .child(
                            div()
                                .debug_selector(|| "settings/nav/divider".into())
                                .w_full()
                                .h(px(1.))
                                .flex_none()
                                .mt(px(10.))
                                .bg(theme::border_strong()),
                        )
                        .child(
                            div()
                                .px(px(2. * NAV_PADDING_X))
                                .pt(px(12.))
                                .pb(px(6.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text_muted())
                                .child("RUSTY TEAMS"),
                        )
                        .child(group(rusty))
                }),
        )
        .when(about_shown, |nav| {
            nav.child(
                div().w_full().px(px(NAV_PADDING_X)).child(
                    item(SettingsSection::About).debug_selector(|| "settings/nav/about".into()),
                ),
            )
        })
}

fn nav_item(
    section: SettingsSection,
    active: bool,
    on_select: impl Fn(SettingsSection, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(element_id(section))
        .debug_selector(move || item_selector(section))
        .w_full()
        .min_h(px(ITEM_MIN_HEIGHT))
        .flex_none()
        .px(px(12.))
        .py(px(ITEM_PADDING_Y))
        .gap(px(12.))
        .items_center()
        .rounded(px(ITEM_RADIUS))
        .cursor_pointer()
        .text_size(px(14.))
        .line_height(px(ITEM_LINE_HEIGHT))
        .when(active, |item| {
            item.bg(theme::popup_hover()).text_color(theme::text())
        })
        .when(!active, |item| {
            item.text_color(theme::text_strong())
                .hover(|item| item.bg(theme::popup()))
        })
        .child(icon(
            icon_name(section),
            ITEM_ICON,
            if active {
                theme::accent_text()
            } else {
                theme::text_muted()
            },
        ))
        .child(
            div()
                .debug_selector(move || label_selector(section))
                .flex_1()
                .min_w_0()
                .child(label(section)),
        )
        .on_click(move |_, window, cx| on_select(section, window, cx))
}

#[cfg(test)]
mod tests {
    use super::{SettingsSection, first_match, label, matches, search_terms};

    #[test]
    fn search_finds_sections_by_name_and_by_setting() {
        assert!(matches(SettingsSection::Chats, "read receipts"));
        assert!(matches(SettingsSection::Devices, "MICRO"));
        assert!(matches(SettingsSection::Calls, "calls"));
        assert!(!matches(SettingsSection::Appearance, "receipts"));
        assert!(matches(SettingsSection::Appearance, "  "));
        assert_eq!(first_match("noise", true), Some(SettingsSection::Devices));
        assert_eq!(first_match("claude", false), None);
        assert_eq!(first_match("xyz", true), None);
    }

    #[test]
    fn every_search_term_names_a_setting_on_its_page() {
        let pages = [
            (SettingsSection::General, include_str!("general.rs")),
            (
                SettingsSection::Notifications,
                include_str!("notifications.rs"),
            ),
            (SettingsSection::Appearance, include_str!("appearance.rs")),
            (SettingsSection::Chats, include_str!("chats.rs")),
            (SettingsSection::Devices, include_str!("devices.rs")),
        ];
        for (section, source) in pages {
            for term in search_terms(section) {
                assert!(source.contains(term), "{term} is not on {}", label(section));
            }
        }
    }
}
