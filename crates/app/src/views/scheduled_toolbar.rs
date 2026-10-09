use gpui_kit::assets::IconName;
use gpui_kit::base::GlobalState;
use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::message_actions::{Action, HoverChange, bar_button};
use super::widgets::icon;
use crate::theme;

pub struct ScheduledMenu {
    pub key: String,
    pub edit: Action,
    pub send_now: Action,
    pub delete: Action,
    pub hover: HoverChange,
}

fn element_id(prefix: &str, key: &str) -> ElementId {
    ElementId::Name(format!("{prefix}-{key}").into())
}

fn text_button(id: ElementId, label: &'static str) -> Stateful<Div> {
    bar_button(id)
        .w_auto()
        .px(px(8.))
        .text_size(px(12.5))
        .text_color(theme::text_soft())
        .child(label)
}

pub fn scheduled_toolbar(menu: ScheduledMenu) -> AnyElement {
    let ScheduledMenu {
        key,
        edit,
        send_now,
        delete,
        hover,
    } = menu;
    h_flex()
        .id(element_id("scheduled-toolbar", &key))
        .p(px(2.))
        .gap(px(2.))
        .items_center()
        .rounded(px(8.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border_strong())
        .occlude()
        .on_hover(move |hovered, _, cx| hover(*hovered, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            GlobalState::suppress_text_selection(cx)
        })
        .child(
            text_button(element_id("scheduled-edit", &key), "Edit").on_click(
                move |_, window, cx| {
                    cx.stop_propagation();
                    edit(window, cx);
                },
            ),
        )
        .child(
            text_button(element_id("scheduled-send-now", &key), "Send now").on_click(
                move |_, window, cx| {
                    cx.stop_propagation();
                    send_now(window, cx);
                },
            ),
        )
        .child(
            bar_button(element_id("scheduled-delete", &key))
                .child(icon(IconName::Trash, 16., theme::red_soft()))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    delete(window, cx);
                }),
        )
        .into_any_element()
}
