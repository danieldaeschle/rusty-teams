use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::tr;
use crate::settings_section::SettingsSection;
use crate::theme;
use crate::views::widgets::icon;

pub const TEAMS_GROUP: [SettingsSection; 7] = [
    SettingsSection::General,
    SettingsSection::Notifications,
    SettingsSection::Appearance,
    SettingsSection::Chats,
    SettingsSection::Calls,
    SettingsSection::Calendar,
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

fn search_terms(section: SettingsSection) -> Vec<String> {
    match section {
        SettingsSection::General => vec![
            tr("settings.general.auto_start"),
            tr("settings.general.open_in_background"),
            tr("settings.general.close_to_tray"),
            tr("settings.general.up_arrow"),
            tr("settings.general.translate_into"),
            tr("settings.general.behavior_title"),
            tr("settings.general.never_translate"),
        ],
        SettingsSection::Notifications => vec![
            tr("settings.notifications.chat_messages"),
            tr("settings.notifications.meeting_chats"),
            tr("settings.notifications.likes_reactions"),
            tr("settings.notifications.teams_channels"),
            tr("settings.notifications.show_preview"),
            tr("settings.notifications.notification_sound"),
            tr("settings.notifications.urgent_sound"),
            tr("settings.notifications.position"),
            tr("settings.notifications.flash_taskbar"),
            tr("settings.notifications.hide_in_calls"),
            tr("settings.notifications.send_test"),
        ],
        SettingsSection::Appearance => vec![
            tr("settings.appearance.theme"),
            tr("settings.appearance.color_scheme"),
            tr("settings.appearance.app_icon"),
        ],
        SettingsSection::Chats => vec![
            tr("settings.chats.density"),
            tr("settings.chats.read_receipts"),
            tr("settings.chats.message_actions"),
            tr("settings.chats.muted_section"),
            tr("settings.chats.enter_to"),
            tr("settings.chats.markdown"),
        ],
        SettingsSection::Calls => vec![tr("settings.calls.ringtones")],
        SettingsSection::Calendar => vec![
            "Google Calendar".to_owned(),
            "Connect Google".to_owned(),
            "Home calendar".to_owned(),
            "Also write to".to_owned(),
            "Outlook".to_owned(),
        ],
        SettingsSection::Devices => vec![
            "Speaker".to_owned(),
            "Microphone".to_owned(),
            "Noise suppression".to_owned(),
            "Music mode".to_owned(),
            "Camera".to_owned(),
            "Background".to_owned(),
            "Soft focus".to_owned(),
            "Adjust brightness".to_owned(),
            "Mirror my video".to_owned(),
            "Make a test call".to_owned(),
            "Save a test report after each call".to_owned(),
        ],
        SettingsSection::Agents => vec!["Claude".to_owned(), "Agent runs".to_owned()],
        SettingsSection::Mcp => vec![
            "Model Context Protocol".to_owned(),
            "Conversation access".to_owned(),
        ],
        SettingsSection::About => vec!["Version".to_owned(), "Build".to_owned()],
    }
}

pub fn matches(section: SettingsSection, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || std::iter::once(label(section))
            .chain(search_terms(section))
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

pub fn label(section: SettingsSection) -> String {
    match section {
        SettingsSection::General => tr("settings.nav.general"),
        SettingsSection::Notifications => tr("settings.nav.notifications"),
        SettingsSection::Appearance => tr("settings.nav.appearance"),
        SettingsSection::Chats => tr("settings.nav.chats"),
        SettingsSection::Calls => tr("settings.nav.calls"),
        SettingsSection::Calendar => tr("settings.nav.calendar"),
        SettingsSection::Devices => tr("settings.nav.devices"),
        SettingsSection::Agents => tr("settings.nav.agents"),
        SettingsSection::Mcp => tr("settings.nav.mcp"),
        SettingsSection::About => tr("settings.nav.about"),
    }
}

fn icon_name(section: SettingsSection) -> IconName {
    match section {
        SettingsSection::General => IconName::SlidersHorizontal,
        SettingsSection::Notifications => IconName::Bell,
        SettingsSection::Appearance => IconName::Palette,
        SettingsSection::Chats => IconName::MessageSquare,
        SettingsSection::Calls => IconName::Phone,
        SettingsSection::Calendar => IconName::Calendar,
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
        SettingsSection::Calendar => "settings-nav-calendar",
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
                .child(tr("common.settings")),
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
                            .child(tr("settings.search.no_results")),
                    )
                })
                .when(!rusty.is_empty(), |groups| {
                    groups
                        .child(
                            // No debug_selector: scroll content bounds are unclipped, so a
                            // tagged divider under settings/nav false-overlaps sticky About.
                            div()
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
                div()
                    .w_full()
                    .flex_none()
                    .px(px(NAV_PADDING_X))
                    .pt(px(6.))
                    .child(
                        item(SettingsSection::About)
                            .debug_selector(|| "settings/nav/about".into()),
                    ),
            )
        })
}

fn nav_item(
    section: SettingsSection,
    active: bool,
    on_select: impl Fn(SettingsSection, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label_text = label(section);
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
                .child(label_text),
        )
        .on_click(move |_, window, cx| on_select(section, window, cx))
}

#[cfg(test)]
mod tests {
    use app_base::ui_locale::apply_locale;

    use super::{SettingsSection, first_match, label, matches, search_terms};

    #[test]
    fn search_finds_sections_by_name_and_by_setting() {
        apply_locale("en");
        assert!(matches(SettingsSection::Chats, "read receipts"));
        assert!(matches(SettingsSection::Devices, "MICRO"));
        assert!(matches(SettingsSection::Calls, "calls"));
        assert!(!matches(SettingsSection::Appearance, "receipts"));
        assert!(matches(SettingsSection::Appearance, "  "));
        assert_eq!(first_match("noise", true), Some(SettingsSection::Devices));
        assert_eq!(first_match("claude", false), None);
        assert_eq!(first_match("xyz", true), None);
    }

    fn search_term_keys(section: SettingsSection) -> &'static [&'static str] {
        match section {
            SettingsSection::General => &[
                "settings.general.auto_start",
                "settings.general.open_in_background",
                "settings.general.close_to_tray",
                "settings.general.up_arrow",
                "settings.general.translate_into",
                "settings.general.behavior_title",
                "settings.general.never_translate",
            ],
            SettingsSection::Notifications => &[
                "settings.notifications.chat_messages",
                "settings.notifications.meeting_chats",
                "settings.notifications.likes_reactions",
                "settings.notifications.teams_channels",
                "settings.notifications.show_preview",
                "settings.notifications.notification_sound",
                "settings.notifications.urgent_sound",
                "settings.notifications.position",
                "settings.notifications.flash_taskbar",
                "settings.notifications.hide_in_calls",
                "settings.notifications.send_test",
            ],
            SettingsSection::Appearance => &[
                "settings.appearance.theme",
                "settings.appearance.color_scheme",
                "settings.appearance.app_icon",
            ],
            SettingsSection::Chats => &[
                "settings.chats.density",
                "settings.chats.read_receipts",
                "settings.chats.message_actions",
                "settings.chats.muted_section",
                "settings.chats.enter_to",
                "settings.chats.markdown",
            ],
            _ => &[],
        }
    }

    #[test]
    fn every_search_term_names_a_setting_on_its_page() {
        apply_locale("en");
        let pages = [
            (SettingsSection::General, include_str!("general.rs")),
            (
                SettingsSection::Notifications,
                include_str!("notifications.rs"),
            ),
            (SettingsSection::Appearance, include_str!("appearance.rs")),
            (SettingsSection::Devices, include_str!("devices.rs")),
        ];
        for (section, source) in pages {
            for key in search_term_keys(section) {
                assert!(
                    source.contains(key),
                    "{key} is not wired on {}",
                    label(section)
                );
            }
            if search_term_keys(section).is_empty() {
                for term in search_terms(section) {
                    assert!(
                        source.contains(&term),
                        "{term} is not on {}",
                        label(section)
                    );
                }
            }
        }
    }
}
