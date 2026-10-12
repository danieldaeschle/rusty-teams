use gpui_kit::assets::IconName;
use gpui_kit::component::{tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::shell::ISLAND_GAP;
use super::shortcuts::keys_for_label;
use super::sidebar::{SidebarTab, SidebarView};
use super::widgets::icon;
use crate::activity::ActivityCenter;
use crate::app_state::AppState;
use crate::format;
use crate::sidebar_model::{any_unread_channel, unread_chat_count};
use crate::theme;

pub const RAIL_WIDTH: f32 = ITEM_WIDTH + 2. * ISLAND_GAP;
const ITEM_WIDTH: f32 = 48.;
const ITEM_HEIGHT: f32 = 52.;
const ICON_SIZE: f32 = 20.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailBadge {
    None,
    Count(u32),
    Dot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RailItem {
    tab: SidebarTab,
    label: &'static str,
    shortcut_label: &'static str,
    icon: IconName,
    badge: RailBadge,
}

fn count_badge_for(count: usize) -> RailBadge {
    match count {
        0 => RailBadge::None,
        count => RailBadge::Count(count.min(u32::MAX as usize) as u32),
    }
}

fn rail_items(activity_unread: usize, unread_chats: usize, channel_dot: bool) -> [RailItem; 5] {
    [
        RailItem {
            tab: SidebarTab::Activity,
            label: "Activity",
            shortcut_label: "Open Activity",
            icon: IconName::Bell,
            badge: count_badge_for(activity_unread),
        },
        RailItem {
            tab: SidebarTab::Chats,
            label: "Chats",
            shortcut_label: "Show Chats",
            icon: IconName::MessageSquare,
            badge: count_badge_for(unread_chats),
        },
        RailItem {
            tab: SidebarTab::Channels,
            label: "Channels",
            shortcut_label: "Show Channels",
            icon: IconName::Users,
            badge: if channel_dot {
                RailBadge::Dot
            } else {
                RailBadge::None
            },
        },
        RailItem {
            tab: SidebarTab::Calls,
            label: "Calls",
            shortcut_label: "Show Calls",
            icon: IconName::Phone,
            badge: RailBadge::None,
        },
        RailItem {
            tab: SidebarTab::Saved,
            label: "Saved",
            shortcut_label: "Open Saved",
            icon: IconName::Bookmark,
            badge: RailBadge::None,
        },
    ]
}

fn tooltip_text(item: &RailItem) -> SharedString {
    match keys_for_label(item.shortcut_label) {
        Some(keys) => format!("{} ({keys})", item.label).into(),
        None => item.label.into(),
    }
}

pub struct RailView {
    state: Entity<AppState>,
    activity: Entity<ActivityCenter>,
    sidebar: Entity<SidebarView>,
    _observations: [Subscription; 3],
}

impl RailView {
    pub fn new(
        state: Entity<AppState>,
        activity: Entity<ActivityCenter>,
        sidebar: Entity<SidebarView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observations = [
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.observe(&activity, |_, _, cx| cx.notify()),
            cx.observe(&sidebar, |_, _, cx| cx.notify()),
        ];
        RailView {
            state,
            activity,
            sidebar,
            _observations: observations,
        }
    }

    fn item(&self, item: &RailItem, active: bool) -> impl IntoElement {
        let tab = item.tab;
        let sidebar = self.sidebar.clone();
        let tooltip = tooltip_text(item);
        let color = if active {
            theme::accent_text()
        } else {
            theme::text_muted()
        };
        div()
            .id(SharedString::from(format!("rail-{}", item.label)))
            .relative()
            .w(px(ITEM_WIDTH))
            .h(px(ITEM_HEIGHT))
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .rounded(px(10.))
            .cursor_pointer()
            .when(active, |button| button.bg(theme::surface_raised()))
            .when(!active, |button| button.hover(|button| button.bg(theme::row_hover())))
            .child(icon(item.icon, ICON_SIZE, color))
            .child(
                div()
                    .whitespace_nowrap()
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(item.label),
            )
            .child(badge(item.badge))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_click(move |_, _, cx| {
                sidebar.update(cx, |sidebar, cx| sidebar.open_tab(tab, cx));
            })
    }
}

fn badge(badge: RailBadge) -> Div {
    match badge {
        RailBadge::None => div(),
        RailBadge::Dot => div()
            .absolute()
            .top(px(8.))
            .left(px(31.))
            .size(px(7.))
            .rounded_full()
            .bg(theme::accent()),
        RailBadge::Count(count) => div()
            .absolute()
            .top(px(5.))
            .left(px(28.))
            .min_w(px(16.))
            .h(px(16.))
            .px(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .bg(theme::accent())
            .text_color(theme::on_accent())
            .text_size(px(10.))
            .line_height(px(16.))
            .font_weight(FontWeight::BOLD)
            .child(format::badge_text(count)),
    }
}

impl Render for RailView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.sidebar.read(cx).tab();
        let activity_unread = self.activity.read(cx).feed().unread_count();
        let (unread_chats, channel_dot) = {
            let state = self.state.read(cx);
            (
                unread_chat_count(&state.sidebar.chats),
                any_unread_channel(&state.sidebar),
            )
        };
        v_flex()
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .pt(px(0.))
            .children(
                rail_items(activity_unread, unread_chats as usize, channel_dot)
                    .iter()
                    .map(|item| self.item(item, item.tab == active)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{RailBadge, count_badge_for, rail_items, tooltip_text};
    use crate::views::sidebar::SidebarTab;

    #[test]
    fn items_follow_the_spec_order() {
        let tabs: Vec<SidebarTab> = rail_items(0, 0, false).iter().map(|item| item.tab).collect();
        assert_eq!(
            tabs,
            [
                SidebarTab::Activity,
                SidebarTab::Chats,
                SidebarTab::Channels,
                SidebarTab::Calls,
                SidebarTab::Saved
            ]
        );
    }

    #[test]
    fn badges_follow_the_unread_inputs() {
        let items = rail_items(3, 5, true);
        assert_eq!(items[0].badge, RailBadge::Count(3));
        assert_eq!(items[1].badge, RailBadge::Count(5));
        assert_eq!(items[2].badge, RailBadge::Dot);
        assert_eq!(items[3].badge, RailBadge::None);
        assert_eq!(items[4].badge, RailBadge::None);
        let quiet = rail_items(0, 0, false);
        assert!(quiet.iter().all(|item| item.badge == RailBadge::None));
        assert_eq!(count_badge_for(usize::MAX), RailBadge::Count(u32::MAX));
    }

    #[test]
    fn every_item_shows_its_shortcut_in_the_tooltip() {
        let items = rail_items(0, 0, false);
        assert_eq!(tooltip_text(&items[0]), "Activity (Ctrl+1)");
        assert!(items.iter().all(|item| tooltip_text(item).ends_with(')')));
    }
}
