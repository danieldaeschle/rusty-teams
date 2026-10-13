use calling::Reaction;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

use super::call_organizer::CAMERA_MENU_WIDTH;
use super::call_view::{CONTROL_SIZE, reaction_glyph};
use super::call_view_menu::view_items;
use crate::app_state::AppState;
use crate::call::CallModel;

pub(super) const CONTROL_GAP: f32 = 12.;
pub(super) const CONTROLS_PADDING: f32 = 16.;
const LEAVE_WIDTH: f32 = 116.;

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

pub fn fixed_controls_width(has_camera_menu: bool, has_more: bool, has_leave_menu: bool) -> f32 {
    let mut widths = vec![
        CONTROL_SIZE + CAMERA_MENU_WIDTH,
        CONTROL_SIZE + if has_camera_menu { CAMERA_MENU_WIDTH } else { 0. },
        CONTROL_SIZE,
        LEAVE_WIDTH,
    ];
    if has_more {
        widths.push(CONTROL_SIZE);
    }
    if has_leave_menu {
        widths.push(CONTROL_SIZE);
    }
    widths.iter().sum::<f32>() + CONTROL_GAP * (widths.len() - 1) as f32
}

pub fn folded_for_width(available: f32, fixed: f32, present: &[Foldable]) -> Vec<Foldable> {
    let room = available - 2. * CONTROLS_PADDING - fixed;
    let fitting = if room <= 0. {
        0
    } else {
        (room / (CONTROL_SIZE + CONTROL_GAP)).floor() as usize
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
                    PopupMenuItem::new(format!("{} {}", reaction_glyph(reaction), reaction.name()))
                        .disabled(!can_react)
                        .on_click(move |_, _, cx| {
                            app.update(cx, |state, cx| state.send_call_reaction(reaction, cx))
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
    use super::{CONTROLS_PADDING, Foldable, fixed_controls_width, folded_for_width};

    const ALL: [Foldable; 5] = [
        Foldable::View,
        Foldable::Reactions,
        Foldable::RaiseHand,
        Foldable::People,
        Foldable::Chat,
    ];
    const SLOT: f32 = 44. + 12.;
    const FIXED: f32 = 400.;

    fn width_for(fitting: f32) -> f32 {
        FIXED + 2. * CONTROLS_PADDING + fitting * SLOT
    }

    #[test]
    fn nothing_folds_when_everything_fits() {
        assert!(folded_for_width(width_for(5.), FIXED, &ALL).is_empty());
        assert!(folded_for_width(width_for(9.), FIXED, &ALL).is_empty());
    }

    #[test]
    fn folds_view_first_then_reactions() {
        assert_eq!(folded_for_width(width_for(4.), FIXED, &ALL), [Foldable::View]);
        assert_eq!(
            folded_for_width(width_for(3.), FIXED, &ALL),
            [Foldable::View, Foldable::Reactions]
        );
    }

    #[test]
    fn everything_folds_when_only_the_fixed_controls_fit() {
        assert_eq!(folded_for_width(width_for(0.), FIXED, &ALL), ALL);
        assert_eq!(folded_for_width(100., FIXED, &ALL), ALL);
    }

    #[test]
    fn only_present_controls_are_counted() {
        let present = [Foldable::View, Foldable::People];
        assert!(folded_for_width(width_for(2.), FIXED, &present).is_empty());
        assert_eq!(
            folded_for_width(width_for(1.), FIXED, &present),
            [Foldable::View]
        );
    }

    #[test]
    fn fixed_width_counts_optional_controls() {
        let base = fixed_controls_width(false, false, false);
        assert_eq!(fixed_controls_width(true, false, false), base + 28.);
        assert_eq!(fixed_controls_width(false, true, false), base + 44. + 12.);
        assert_eq!(fixed_controls_width(false, false, true), base + 44. + 12.);
    }
}
