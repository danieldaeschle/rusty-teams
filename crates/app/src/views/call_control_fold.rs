use calling::Reaction;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

use super::call_toolbar::{GROUP_DIVIDER_WIDTH, GROUP_GAP, SPLIT_CHEVRON_WIDTH, TOOL_GAP, TOOL_WIDTH};
use super::call_view::reaction_glyph;
use super::call_view_menu::view_items;
use crate::app_state::AppState;
use crate::call::CallModel;

pub(super) const TITLE_RESERVE: f32 = 280.;
const LEAVE_WIDTH: f32 = 96.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foldable {
    View,
    Reactions,
    RaiseHand,
    People,
    Chat,
}

#[derive(Clone, Debug, Default)]
pub struct FoldedControls {
    pub items: Vec<Foldable>,
    pub fullscreen: bool,
    pub people_open: bool,
    pub chat_open: bool,
    pub chat_unread: u32,
}

impl FoldedControls {
    pub fn folds(&self, control: Foldable) -> bool {
        self.items.contains(&control)
    }
}

pub fn fixed_controls_width(has_camera_menu: bool, has_more: bool, has_leave_menu: bool, has_pop_out: bool) -> f32 {
    let devices = [
        TOOL_WIDTH + if has_camera_menu { SPLIT_CHEVRON_WIDTH } else { 0. },
        TOOL_WIDTH + SPLIT_CHEVRON_WIDTH,
        TOOL_WIDTH,
    ];
    let devices_width = devices.iter().sum::<f32>() + TOOL_GAP * (devices.len() - 1) as f32;
    let more_width = if has_more { TOOL_WIDTH } else { 0. } + if has_pop_out { TOOL_WIDTH + TOOL_GAP } else { 0. };
    let leave_width = LEAVE_WIDTH
        + if has_leave_menu {
            SPLIT_CHEVRON_WIDTH + 4. + 1.
        } else {
            0.
        };
    devices_width + more_width + leave_width + 5. * GROUP_GAP + 2. * GROUP_DIVIDER_WIDTH
}

pub fn in_more_menu(folded: Vec<Foldable>) -> Vec<Foldable> {
    let mut items = vec![Foldable::View];
    for control in folded {
        items.push(control);
        if control == Foldable::Reactions {
            items.push(Foldable::RaiseHand);
        }
    }
    items
}

pub fn folded_for_width(available: f32, fixed: f32, present: &[Foldable]) -> Vec<Foldable> {
    let room = available - TITLE_RESERVE - fixed;
    let fitting = if room <= 0. {
        0
    } else {
        (room / (TOOL_WIDTH + TOOL_GAP)).floor() as usize
    };
    let folded_count = present.len().saturating_sub(fitting);
    present[..folded_count].to_vec()
}

pub fn folded_items(
    mut popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    app: &Entity<AppState>,
    model: &CallModel,
    folded: &FoldedControls,
) -> PopupMenu {
    if folded.items.is_empty() {
        return popup;
    }
    for item in &folded.items {
        popup = match item {
            Foldable::View => view_submenu(popup, window, cx, app, model, folded.fullscreen),
            Foldable::Reactions => reactions_submenu(popup, window, cx, app, model),
            Foldable::RaiseHand => hand_item(popup, app, model),
            Foldable::People => people_item(popup, app, folded.people_open),
            Foldable::Chat => chat_item(popup, app, folded),
        };
    }
    popup.separator()
}

fn view_submenu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    app: &Entity<AppState>,
    model: &CallModel,
    fullscreen: bool,
) -> PopupMenu {
    let app = app.clone();
    let model = model.clone();
    popup.submenu_with_icon(
        Some(Icon::new(IconName::LayoutGrid)),
        "View",
        window,
        cx,
        move |menu, _, _| view_items(menu, &app, &model, fullscreen),
    )
}

fn reactions_submenu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    app: &Entity<AppState>,
    model: &CallModel,
) -> PopupMenu {
    let app = app.clone();
    let can_react = model.can_react();
    popup.submenu_with_icon(
        Some(Icon::new(IconName::FaceSlightlySmiling)),
        "Reactions",
        window,
        cx,
        move |mut menu, _, _| {
            for reaction in Reaction::ALL {
                let app = app.clone();
                menu = menu.item(
                    PopupMenuItem::new(format!("{} {}", reaction_glyph(&reaction), reaction.name()))
                        .disabled(!can_react)
                        .on_click(move |_, _, cx| {
                            app.update(cx, |state, cx| state.send_call_reaction(reaction.clone(), cx))
                        }),
                );
            }
            menu
        },
    )
}

fn hand_item(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel) -> PopupMenu {
    let app = app.clone();
    let raised = model.own_hand.is_some();
    popup.item(
        PopupMenuItem::new(if raised { "Lower hand" } else { "Raise hand" })
            .icon(IconName::Hand)
            .disabled(!model.can_react())
            .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_hand(cx))),
    )
}

fn people_item(popup: PopupMenu, app: &Entity<AppState>, open: bool) -> PopupMenu {
    let app = app.clone();
    popup.item(
        PopupMenuItem::new(if open { "Close the people list" } else { "Show people" })
            .icon(IconName::Users)
            .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_people(cx))),
    )
}

fn chat_item(popup: PopupMenu, app: &Entity<AppState>, folded: &FoldedControls) -> PopupMenu {
    let app = app.clone();
    let label = match (folded.chat_open, folded.chat_unread) {
        (true, _) => "Close the chat".to_owned(),
        (false, 0) => "Open the chat".to_owned(),
        (false, unread) => format!("Open the chat ({unread})"),
    };
    popup.item(
        PopupMenuItem::new(label)
            .icon(IconName::MessageCircle)
            .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_chat(cx))),
    )
}

#[cfg(test)]
mod tests {
    use super::{Foldable, TITLE_RESERVE, fixed_controls_width, folded_for_width, in_more_menu};

    const ALL: [Foldable; 3] = [Foldable::Reactions, Foldable::People, Foldable::Chat];
    const SLOT: f32 = 56. + 2.;
    const FIXED: f32 = 400.;

    fn width_for(fitting: f32) -> f32 {
        FIXED + TITLE_RESERVE + fitting * SLOT
    }

    #[test]
    fn nothing_folds_when_everything_fits() {
        assert!(folded_for_width(width_for(3.), FIXED, &ALL).is_empty());
        assert!(folded_for_width(width_for(9.), FIXED, &ALL).is_empty());
    }

    #[test]
    fn folds_reactions_first_then_people() {
        assert_eq!(folded_for_width(width_for(2.), FIXED, &ALL), [Foldable::Reactions]);
        assert_eq!(
            folded_for_width(width_for(1.), FIXED, &ALL),
            [Foldable::Reactions, Foldable::People]
        );
    }

    #[test]
    fn everything_folds_when_only_the_fixed_controls_fit() {
        assert_eq!(folded_for_width(width_for(0.), FIXED, &ALL), ALL);
        assert_eq!(folded_for_width(100., FIXED, &ALL), ALL);
    }

    #[test]
    fn view_always_sits_in_more_and_the_hand_follows_the_reactions() {
        assert_eq!(in_more_menu(Vec::new()), [Foldable::View]);
        assert_eq!(
            in_more_menu(vec![Foldable::Reactions, Foldable::Chat]),
            [Foldable::View, Foldable::Reactions, Foldable::RaiseHand, Foldable::Chat]
        );
    }

    #[test]
    fn fixed_width_counts_optional_controls() {
        let base = fixed_controls_width(false, false, false, false);
        assert_eq!(fixed_controls_width(true, false, false, false), base + 20.);
        assert_eq!(fixed_controls_width(false, true, false, false), base + 56.);
        assert_eq!(fixed_controls_width(false, false, true, false), base + 25.);
        assert_eq!(fixed_controls_width(false, false, false, true), base + 58.);
    }
}
