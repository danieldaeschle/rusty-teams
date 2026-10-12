use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable;
use gpui_kit::component::{popover::Popover, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::shell::ISLAND_GAP;
use super::shortcuts::keys_for_label;
use super::sidebar::{SidebarTab, SidebarView};
use super::widgets::icon;
use crate::activity::ActivityCenter;
use crate::agents::{AgentRuns, RunsPopover, runs_handle};
use crate::app_state::AppState;
use crate::format;
use crate::sidebar_model::{any_unread_channel, unread_chat_count};
use crate::theme;

pub const RAIL_WIDTH: f32 = ITEM_WIDTH + 2. * ISLAND_GAP;
const ITEM_WIDTH: f32 = 48.;
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
enum RailTarget {
    Tab(SidebarTab),
    Agents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RailItem {
    target: RailTarget,
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
            target: RailTarget::Tab(SidebarTab::Activity),
            label: "Activity",
            shortcut_label: "Open Activity",
            icon: IconName::Bell,
            badge: count_badge_for(activity_unread),
        },
        RailItem {
            target: RailTarget::Tab(SidebarTab::Chats),
            label: "Chats",
            shortcut_label: "Show Chats",
            icon: IconName::MessageSquare,
            badge: count_badge_for(unread_chats),
        },
        RailItem {
            target: RailTarget::Tab(SidebarTab::Channels),
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
            target: RailTarget::Tab(SidebarTab::Calls),
            label: "Calls",
            shortcut_label: "Show Calls",
            icon: IconName::Phone,
            badge: RailBadge::None,
        },
        RailItem {
            target: RailTarget::Tab(SidebarTab::Saved),
            label: "Saved",
            shortcut_label: "Open Saved",
            icon: IconName::Bookmark,
            badge: RailBadge::None,
        },
    ];
    if let Some(running) = agents_running {
        items.push(RailItem {
            target: RailTarget::Agents,
            label: "Agents",
            shortcut_label: "Agents",
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
    runs_popover: Option<Entity<RunsPopover>>,
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
        .w(px(ITEM_WIDTH))
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
                .max_w(px(ITEM_WIDTH - 2. * LABEL_INSET))
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

#[derive(IntoElement)]
struct AgentsTrigger {
    item: RailItem,
    open: bool,
}

impl Selectable for AgentsTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.open = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.open
    }
}

impl RenderOnce for AgentsTrigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        item_element(&self.item, self.open)
    }
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
        let runs_popover = runs.clone().map(|runs| {
            let app = state.clone();
            cx.new(|cx| RunsPopover::new(app, runs, cx))
        });
        RailView {
            state,
            activity,
            sidebar,
            runs,
            runs_popover,
            _observations: observations,
        }
    }

    fn item(&self, item: &RailItem, active: bool) -> AnyElement {
        match item.target {
            RailTarget::Tab(tab) => {
                let sidebar = self.sidebar.clone();
                item_element(item, active)
                    .on_click(move |_, _, cx| {
                        sidebar.update(cx, |sidebar, cx| sidebar.open_tab(tab, cx));
                    })
                    .into_any_element()
            }
            RailTarget::Agents => self.agents_item(item),
        }
    }

    fn agents_item(&self, item: &RailItem) -> AnyElement {
        let Some(content) = self.runs_popover.clone() else {
            return div().into_any_element();
        };
        Popover::new("rail-agents-popover")
            .anchor(Anchor::LeftCenter)
            .offset(px(6.))
            .p(px(4.))
            .trigger(AgentsTrigger {
                item: *item,
                open: false,
            })
            .content(move |_, _, cx| {
                let popover = cx.entity().downgrade();
                content.update(cx, |content, _| content.set_popover(popover));
                content.clone()
            })
            .into_any_element()
    }

    fn agents_running(&self, cx: &App) -> Option<usize> {
        let runs = self.runs.as_ref()?.read(cx);
        (!runs.settings().actions.is_empty()).then(|| runs.running_count())
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
                    .map(|item| self.item(item, item.target == RailTarget::Tab(active))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{RailBadge, RailTarget, count_badge_for, rail_items, tooltip_text};
    use crate::views::sidebar::SidebarTab;

    #[test]
    fn items_follow_the_spec_order() {
        let targets: Vec<RailTarget> = rail_items(0, 0, false, None)
            .iter()
            .map(|item| item.target)
            .collect();
        assert_eq!(
            targets,
            [
                RailTarget::Tab(SidebarTab::Activity),
                RailTarget::Tab(SidebarTab::Chats),
                RailTarget::Tab(SidebarTab::Channels),
                RailTarget::Tab(SidebarTab::Calls),
                RailTarget::Tab(SidebarTab::Saved)
            ]
        );
    }

    #[test]
    fn agents_comes_last_and_only_when_actions_exist() {
        assert_eq!(rail_items(0, 0, false, None).len(), 5);
        let with_agents = rail_items(0, 0, false, Some(2));
        assert_eq!(with_agents.len(), 6);
        assert_eq!(with_agents[5].target, RailTarget::Agents);
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
