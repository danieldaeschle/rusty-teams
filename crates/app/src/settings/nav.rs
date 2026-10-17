use gpui_kit::assets::IconName;
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
const ITEM_HEIGHT: f32 = 34.;
const ITEM_GAP: f32 = 2.;
const ITEM_RADIUS: f32 = 6.;
const ITEM_ICON: f32 = 16.;
const NAV_PADDING_X: f32 = 12.;

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

pub fn render_nav(
    active: SettingsSection,
    agents_available: bool,
    layout: NavLayout,
    on_select: impl Fn(SettingsSection, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    let item = |section: SettingsSection| nav_item(section, section == active, on_select.clone());
    let rusty: Vec<Stateful<Div>> = RUSTY_GROUP
        .into_iter()
        .filter(|section| agents_available || *section != SettingsSection::Agents)
        .map(item)
        .collect();
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
            v_flex()
                .id("settings-nav-groups")
                .w_full()
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .gap(px(ITEM_GAP))
                .child(group(TEAMS_GROUP.into_iter().map(item).collect()))
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
                .child(group(rusty)),
        )
        .child(
            div()
                .w_full()
                .px(px(NAV_PADDING_X))
                .child(item(SettingsSection::About).debug_selector(|| "settings/nav/about".into())),
        )
}

fn nav_item(
    section: SettingsSection,
    active: bool,
    on_select: impl Fn(SettingsSection, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(element_id(section))
        .w_full()
        .h(px(ITEM_HEIGHT))
        .flex_none()
        .px(px(12.))
        .gap(px(12.))
        .items_center()
        .rounded(px(ITEM_RADIUS))
        .cursor_pointer()
        .text_size(px(14.))
        .when(active, |item| {
            item.bg(theme::popup_hover())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
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
        .child(div().flex_1().min_w_0().truncate().child(label(section)))
        .on_click(move |_, window, cx| on_select(section, window, cx))
}
