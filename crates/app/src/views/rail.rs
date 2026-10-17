use gpui_kit::assets::IconName;
use gpui_kit::component::{tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::shortcuts::keys_for_label;
use super::sidebar::{SidebarTab, SidebarView};
use super::widgets::{RAIL_BADGE, badge_pill, icon};
use crate::activity::ActivityCenter;
use crate::agents::{AgentRuns, runs_handle};
use crate::app_state::AppState;
use crate::sidebar_model::{any_unread_channel, unread_chat_count};
use crate::theme;
use app_ui_kit::layout::{RAIL_ITEM_WIDTH, RAIL_WIDTH};

const ITEM_HEIGHT: f32 = 52.;
const ICON_SIZE: f32 = 20.;
const LABEL_INSET: f32 = 2.;
const LABEL_HEIGHT: f32 = 12.;

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

fn rail_items(
    activity_unread: usize,
    unread_chats: usize,
    channel_dot: bool,
    agents_running: Option<usize>,
) -> Vec<RailItem> {
    let mut items = vec![
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
    ];
    if let Some(running) = agents_running {
        items.push(RailItem {
            tab: SidebarTab::Agents,
            label: "Agents",
            shortcut_label: "Open Agents",
            icon: IconName::Bot,
            badge: count_badge_for(running),
        });
    }
    items
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
    runs: Option<Entity<AgentRuns>>,
    _observations: Vec<Subscription>,
}

fn item_element(item: &RailItem, active: bool) -> Stateful<Div> {
    let tooltip = tooltip_text(item);
    let color = if active {
        theme::accent_text()
    } else {
        theme::text_muted()
    };
    div()
        .id(SharedString::from(format!("rail-{}", item.label)))
        .relative()
        .w(px(RAIL_ITEM_WIDTH))
        .h(px(ITEM_HEIGHT))
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .justify_center()
        .pt(px(2.))
        .gap(px(4.))
        .rounded(px(10.))
        .cursor_pointer()
        .when(active, |button| button.bg(theme::surface_raised()))
        .when(!active, |button| {
            button.hover(|button| button.bg(theme::row_hover()))
        })
        .child(icon(item.icon, ICON_SIZE, color))
        .child(
            div()
                .max_w(px(RAIL_ITEM_WIDTH - 2. * LABEL_INSET))
                .truncate()
                .h(px(LABEL_HEIGHT))
                .text_size(px(9.))
                .line_height(px(LABEL_HEIGHT))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(item.label),
        )
        .children(badge(item.badge))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
}

impl RailView {
    pub fn new(
        state: Entity<AppState>,
        activity: Entity<ActivityCenter>,
        sidebar: Entity<SidebarView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut observations = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.observe(&activity, |_, _, cx| cx.notify()),
            cx.observe(&sidebar, |_, _, cx| cx.notify()),
        ];
        let runs = runs_handle(cx);
        if let Some(runs) = &runs {
            observations.push(cx.observe(runs, |_, _, cx| cx.notify()));
        }
        RailView {
            state,
            activity,
            sidebar,
            runs,
            _observations: observations,
        }
    }

    fn item(&self, item: &RailItem, active: bool) -> AnyElement {
        let tab = item.tab;
        let sidebar = self.sidebar.clone();
        item_element(item, active)
            .on_click(move |_, _, cx| {
                sidebar.update(cx, |sidebar, cx| sidebar.open_tab(tab, cx));
            })
            .into_any_element()
    }

    fn agents_running(&self, cx: &App) -> Option<usize> {
        let runs = self.runs.as_ref()?;
        crate::agents::agents_enabled(cx).then(|| runs.read(cx).running_count())
    }
}

fn badge(badge: RailBadge) -> Option<Div> {
    Some(match badge {
        RailBadge::None => return None,
        RailBadge::Dot => div()
            .absolute()
            .top(px(8.))
            .left(px(31.))
            .size(px(7.))
            .rounded_full()
            .bg(theme::accent()),
        RailBadge::Count(count) => {
            badge_pill(count, RAIL_BADGE, theme::accent(), theme::on_accent())
                .absolute()
                .top(px(5.))
                .left(px(28.))
        }
    })
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
        let items = rail_items(
            activity_unread,
            unread_chats as usize,
            channel_dot,
            self.agents_running(cx),
        );
        v_flex()
            .debug_selector(|| "shell/islands/rail".into())
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .pt(px(0.))
            .children(
                items
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
        let tabs: Vec<SidebarTab> = rail_items(0, 0, false, None)
            .iter()
            .map(|item| item.tab)
            .collect();
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
    fn agents_comes_last_and_only_when_actions_exist() {
        assert_eq!(rail_items(0, 0, false, None).len(), 5);
        let with_agents = rail_items(0, 0, false, Some(2));
        assert_eq!(with_agents.len(), 6);
        assert_eq!(with_agents[5].tab, SidebarTab::Agents);
        assert_eq!(with_agents[5].label, "Agents");
        assert_eq!(with_agents[5].badge, RailBadge::Count(2));
        assert_eq!(rail_items(0, 0, false, Some(0))[5].badge, RailBadge::None);
    }

    #[test]
    fn badges_follow_the_unread_inputs() {
        let items = rail_items(3, 5, true, None);
        assert_eq!(items[0].badge, RailBadge::Count(3));
        assert_eq!(items[1].badge, RailBadge::Count(5));
        assert_eq!(items[2].badge, RailBadge::Dot);
        assert_eq!(items[3].badge, RailBadge::None);
        assert_eq!(items[4].badge, RailBadge::None);
        let quiet = rail_items(0, 0, false, None);
        assert!(quiet.iter().all(|item| item.badge == RailBadge::None));
        assert_eq!(count_badge_for(usize::MAX), RailBadge::Count(u32::MAX));
    }

    #[test]
    fn every_item_shows_its_shortcut_in_the_tooltip() {
        let items = rail_items(0, 0, false, None);
        assert_eq!(tooltip_text(&items[0]), "Activity (Ctrl+1)");
        assert!(items.iter().all(|item| tooltip_text(item).ends_with(')')));
    }
}
