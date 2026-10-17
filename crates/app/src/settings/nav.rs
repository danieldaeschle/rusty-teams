use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings_section::SettingsSection;
use crate::theme;
use crate::views::widgets::icon;

pub const TEAMS_GROUP: [SettingsSection; 5] = [
    SettingsSection::General,
    SettingsSection::Notifications,
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
    v_flex()
        .debug_selector(|| "settings/nav".into())
        .w(px(layout.width))
        .h_full()
        .flex_none()
        .px(px(NAV_PADDING_X))
        .py(px(layout.padding_y))
        .gap(px(ITEM_GAP))
        .bg(theme::surface_raised())
        .child(
            div()
                .mx(px(12.))
                .mb(px(18.))
                .text_size(px(20.))
                .line_height(px(26.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child("Settings"),
        )
        .children(TEAMS_GROUP.map(item))
        .child(
            div()
                .mt(px(10.))
                .px(px(12.))
                .pt(px(12.))
                .pb(px(6.))
                .border_t_1()
                .border_color(theme::border_strong())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .child("RUSTY TEAMS"),
        )
        .children(rusty)
        .child(div().flex_1().min_h(px(0.)))
        .child(item(SettingsSection::About))
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
