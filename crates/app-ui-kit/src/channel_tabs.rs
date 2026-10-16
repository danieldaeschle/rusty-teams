use std::rc::Rc;

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::ChannelTabRecord;
use teams_core::channel_tab_link;

use app_base::theme;

const BAR_HEIGHT: f32 = 36.;
const TAB_HEIGHT: f32 = 28.;
const TAB_GAP: f32 = 4.;
const TAB_PADDING: f32 = 12.;
const TAB_TEXT_SIZE: f32 = 12.5;
const BAR_PADDING: f32 = 20.;
const CHAR_WIDTH: f32 = 6.5;
const ARROW_WIDTH: f32 = 14.;
const OVERFLOW_WIDTH: f32 = 44.;
const EXTERNAL_ARROW: &str = "\u{2197}";

pub const POSTS_LABEL: &str = "Posts";
pub const SHARED_LABEL: &str = "Shared";
pub const CHAT_LABEL: &str = "Chat";
pub const RECAP_LABEL: &str = "Recap";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPane {
    Posts,
    Shared,
    Recap,
}

pub type SelectPane = Rc<dyn Fn(ChannelPane, &mut App)>;
pub type OpenTab = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct TabBarActions {
    pub select: SelectPane,
    pub open: OpenTab,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabAction {
    Browser(String),
    ResolveLink,
}

pub fn tab_action(tab: &ChannelTabRecord, resolved_link: Option<&str>) -> TabAction {
    match (&tab.open_url, resolved_link) {
        (Some(url), _) => TabAction::Browser(url.clone()),
        (None, Some(link)) => TabAction::Browser(link.to_owned()),
        (None, None) => TabAction::ResolveLink,
    }
}

pub fn fallback_tab_link(tab: &ChannelTabRecord, channel_id: &str) -> String {
    channel_tab_link(channel_id, &tab.tab_id, &tab.name)
}

fn label_width(label: &str, external: bool) -> f32 {
    label.chars().count() as f32 * CHAR_WIDTH
        + 2. * TAB_PADDING
        + if external { ARROW_WIDTH } else { 0. }
}

pub fn visible_tab_count(tabs: &[ChannelTabRecord], available_width: f32) -> usize {
    let fixed = label_width(POSTS_LABEL, false) + TAB_GAP + label_width(SHARED_LABEL, false);
    let widths: Vec<f32> = tabs
        .iter()
        .map(|tab| TAB_GAP + label_width(&tab.name, true))
        .collect();
    if fixed + widths.iter().sum::<f32>() <= available_width {
        return tabs.len();
    }
    let room = available_width - fixed - TAB_GAP - OVERFLOW_WIDTH;
    let mut used = 0.;
    widths
        .iter()
        .take_while(|width| {
            used += **width;
            used <= room
        })
        .count()
}

fn tab_label(
    id: ElementId,
    text: &str,
    active: bool,
    external: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let color = if active {
        theme::text()
    } else {
        theme::text_muted()
    };
    h_flex()
        .id(id)
        .h(px(TAB_HEIGHT))
        .flex_none()
        .items_center()
        .gap(px(4.))
        .px(px(TAB_PADDING))
        .rounded(px(7.))
        .cursor_pointer()
        .when(active, |tab| tab.bg(theme::surface_raised()))
        .text_size(px(TAB_TEXT_SIZE))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(color)
        .hover(|tab| tab.text_color(theme::text()))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(text.to_owned())
        .when(external, |tab| {
            tab.child(div().text_color(theme::text_faint()).child(EXTERNAL_ARROW))
        })
}

fn overflow_button(hidden: &[(usize, String)], actions: &TabBarActions) -> impl IntoElement {
    let entries = hidden.to_vec();
    let open = actions.open.clone();
    Button::new("channel-tab-overflow")
        .ghost()
        .compact()
        .label(format!("+{}", hidden.len()))
        .dropdown_menu(move |menu, _, _| {
            entries.iter().fold(menu, |menu, (index, name)| {
                let (open, index) = (open.clone(), *index);
                menu.item(
                    PopupMenuItem::new(format!("{name} {EXTERNAL_ARROW}"))
                        .on_click(move |_, window, cx| open(index, window, cx)),
                )
            })
        })
}

pub fn render_tab_bar(
    tabs: &[ChannelTabRecord],
    pane: ChannelPane,
    available_width: f32,
    actions: &TabBarActions,
) -> AnyElement {
    let visible = visible_tab_count(tabs, available_width - 2. * BAR_PADDING);
    let select = actions.select.clone();
    let posts = tab_label(
        "channel-tab-posts".into(),
        POSTS_LABEL,
        pane == ChannelPane::Posts,
        false,
        {
            let select = select.clone();
            move |_, cx| select(ChannelPane::Posts, cx)
        },
    );
    let shared = tab_label(
        "channel-tab-shared".into(),
        SHARED_LABEL,
        pane == ChannelPane::Shared,
        false,
        move |_, cx| select(ChannelPane::Shared, cx),
    );
    let configured = tabs.iter().enumerate().take(visible).map(|(index, tab)| {
        let open = actions.open.clone();
        tab_label(
            ElementId::NamedInteger("channel-tab".into(), index as u64),
            &tab.name,
            false,
            true,
            move |window, cx| open(index, window, cx),
        )
    });
    let hidden: Vec<(usize, String)> = tabs
        .iter()
        .enumerate()
        .skip(visible)
        .map(|(index, tab)| (index, tab.name.clone()))
        .collect();
    h_flex()
        .w_full()
        .h(px(BAR_HEIGHT))
        .flex_none()
        .px(px(BAR_PADDING))
        .gap(px(TAB_GAP))
        .items_center()
        .child(posts)
        .child(shared)
        .children(configured)
        .when(!hidden.is_empty(), |bar| {
            bar.child(overflow_button(&hidden, actions))
        })
        .into_any_element()
}

pub fn render_meeting_bar(pane: ChannelPane, select: SelectPane) -> AnyElement {
    let chat = tab_label(
        "meeting-tab-chat".into(),
        CHAT_LABEL,
        pane == ChannelPane::Posts,
        false,
        {
            let select = select.clone();
            move |_, cx| select(ChannelPane::Posts, cx)
        },
    );
    let recap = tab_label(
        "meeting-tab-recap".into(),
        RECAP_LABEL,
        pane == ChannelPane::Recap,
        false,
        move |_, cx| select(ChannelPane::Recap, cx),
    )
    .debug_selector(|| "shell/islands/main/recap-tab".into());
    h_flex()
        .w_full()
        .h(px(BAR_HEIGHT))
        .flex_none()
        .px(px(BAR_PADDING))
        .gap(px(TAB_GAP))
        .items_center()
        .child(chat)
        .child(recap)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use store::ChannelTabRecord;

    use super::{TabAction, fallback_tab_link, tab_action, visible_tab_count};

    fn tab(name: &str, open_url: Option<&str>) -> ChannelTabRecord {
        ChannelTabRecord {
            tab_id: "tab-1".to_owned(),
            name: name.to_owned(),
            definition_id: "app".to_owned(),
            open_url: open_url.map(str::to_owned),
        }
    }

    fn website(url: &str) -> ChannelTabRecord {
        ChannelTabRecord {
            definition_id: "com.microsoft.teamspace.tab.web".to_owned(),
            ..tab("Docs", Some(url))
        }
    }

    #[test]
    fn website_tabs_open_in_the_browser() {
        assert_eq!(
            tab_action(&website("https://docs.example"), None),
            TabAction::Browser("https://docs.example".to_owned())
        );
    }

    #[test]
    fn other_tabs_with_a_page_open_it_in_the_browser() {
        let notes = tab("Notes", Some("https://onenote.example/web"));
        assert_eq!(
            tab_action(&notes, None),
            TabAction::Browser("https://onenote.example/web".to_owned())
        );
    }

    #[test]
    fn app_tabs_resolve_their_link_once_and_then_reuse_it() {
        let board = tab("Board", None);
        assert_eq!(tab_action(&board, None), TabAction::ResolveLink);
        assert_eq!(
            tab_action(&board, Some("https://teams.example/l/tab")),
            TabAction::Browser("https://teams.example/l/tab".to_owned())
        );
    }

    #[test]
    fn the_fallback_link_is_the_hand_built_deep_link() {
        assert_eq!(
            fallback_tab_link(&tab("Board", None), "19:c@thread.tacv2"),
            "https://teams.microsoft.com/l/channel/19:c@thread.tacv2/tab%3A%3Atab-1?label=Board"
        );
    }

    #[test]
    fn every_tab_stays_visible_while_it_fits() {
        let tabs = [tab("Docs", None), tab("Planner", None)];
        assert_eq!(visible_tab_count(&tabs, 600.), 2);
    }

    #[test]
    fn tabs_that_do_not_fit_move_into_the_overflow() {
        let tabs: Vec<ChannelTabRecord> = (0..8)
            .map(|n| tab(&format!("Tab number {n}"), None))
            .collect();
        let visible = visible_tab_count(&tabs, 420.);
        assert!(visible > 0 && visible < tabs.len());
        assert_eq!(visible_tab_count(&tabs, 40.), 0);
    }
}
