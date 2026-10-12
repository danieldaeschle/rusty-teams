use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    switch::Switch,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::call_view::{CONTROL_SIZE, DISABLED_OPACITY, MENU_WIDTH, round_control};
use super::shortcuts::keys_for_label;
use crate::app_state::AppState;
use crate::call::{CallLayout, CallModel};
use crate::theme;

pub const FULL_SCREEN_LABEL: &str = "Full screen";
const CONTENT_HINT: &str = "Only while someone shares";

fn hint_row(label: &'static str, hint: String, dimmed: bool) -> Div {
    h_flex()
        .w_full()
        .gap(px(16.))
        .items_center()
        .justify_between()
        .when(dimmed, |row| row.opacity(DISABLED_OPACITY))
        .child(label)
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child(hint),
        )
}

fn layout_item(app: &Entity<AppState>, model: &CallModel, layout: CallLayout) -> PopupMenuItem {
    let unavailable = layout == CallLayout::Content && !model.has_content();
    let item = if unavailable {
        PopupMenuItem::element(move |_, _| hint_row(layout.label(), CONTENT_HINT.to_owned(), true))
    } else {
        PopupMenuItem::new(layout.label())
    };
    let app = app.clone();
    item.checked(layout == model.effective_layout())
        .disabled(unavailable)
        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.set_call_layout(layout, cx)))
}

fn switch_item(
    id: &'static str,
    label: &'static str,
    on: bool,
    toggle: impl Fn(&mut AppState, &mut Context<AppState>) + Clone + 'static,
    app: &Entity<AppState>,
) -> PopupMenuItem {
    let app = app.clone();
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .w_full()
            .gap(px(16.))
            .items_center()
            .justify_between()
            .child(label)
            .child(Switch::new(id).checked(on))
    })
    .on_click(move |_, _, cx| app.update(cx, |state, cx| toggle(state, cx)))
}

fn view_items(
    popup: PopupMenu,
    app: &Entity<AppState>,
    model: &CallModel,
    fullscreen: bool,
) -> PopupMenu {
    let hide_me = model.view.hide_me;
    let (hide_me_app, full_screen_app) = (app.clone(), app.clone());
    let mut popup = popup
        .min_w(px(MENU_WIDTH))
        .item(PopupMenuItem::label("Layout"));
    for layout in [
        CallLayout::Gallery,
        CallLayout::Speaker,
        CallLayout::Content,
    ] {
        popup = popup.item(layout_item(app, model, layout));
    }
    popup
        .separator()
        .item(switch_item(
            "call-view-hide-no-video",
            "Hide people without video",
            model.view.hide_no_video,
            |state, cx| state.toggle_call_hide_no_video(cx),
            app,
        ))
        .item(
            PopupMenuItem::new(if hide_me { "Show me" } else { "Hide me" })
                .icon(if hide_me {
                    IconName::Eye
                } else {
                    IconName::EyeOff
                })
                .on_click(move |_, _, cx| {
                    hide_me_app.update(cx, |state, cx| state.set_call_hide_me(!hide_me, cx))
                }),
        )
        .item(switch_item(
            "call-view-chat-bubbles",
            "Chat bubbles",
            model.view.chat_bubbles,
            |state, cx| state.toggle_call_chat_bubbles(cx),
            app,
        ))
        .separator()
        .item(
            PopupMenuItem::element(move |_, _| {
                hint_row(
                    FULL_SCREEN_LABEL,
                    keys_for_label(FULL_SCREEN_LABEL).unwrap_or_default(),
                    false,
                )
            })
            .checked(fullscreen)
            .on_click(move |_, _, cx| {
                full_screen_app.update(cx, |state, cx| state.toggle_call_fullscreen(cx))
            }),
        )
}

pub(super) fn view_button(
    app: &Entity<AppState>,
    model: &CallModel,
    fullscreen: bool,
) -> AnyElement {
    let app = app.clone();
    let model = model.clone();
    Button::new("call-view")
        .ghost()
        .p_0()
        .size(px(CONTROL_SIZE))
        .rounded_full()
        .child(round_control(
            "call-view-face",
            IconName::LayoutGrid,
            theme::surface_raised(),
            theme::text(),
        ))
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |popup, _, _| {
            view_items(popup, &app, &model, fullscreen)
        })
        .into_any_element()
}
