use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::component::{
    button::Button,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme;
use crate::views::widgets::icon;

pub const ROW_INSET: f32 = 12.;
pub const SELECT_WIDTH: f32 = 216.;
const ROW_PADDING: f32 = 12.;
const ROW_GAP: f32 = 12.;
const ROW_RADIUS: f32 = 8.;
const BUTTON_HEIGHT: f32 = 32.;
const BUTTON_RADIUS: f32 = 8.;
const SCROLL_MENU_AFTER: usize = 10;
const MENU_MAX_HEIGHT: f32 = 320.;

pub struct Choice {
    pub label: SharedString,
    pub checked: bool,
    pub choose: Rc<dyn Fn(&mut App)>,
}

impl Choice {
    pub fn new(
        label: impl Into<SharedString>,
        checked: bool,
        choose: impl Fn(&mut App) + 'static,
    ) -> Self {
        Choice {
            label: label.into(),
            checked,
            choose: Rc::new(choose),
        }
    }
}

pub fn switch(on: bool) -> Div {
    div()
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .rounded_full()
        .relative()
        .bg(if on {
            theme::accent()
        } else {
            theme::border_strong()
        })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 18. } else { 2. }))
                .size(px(16.))
                .rounded_full()
                .bg(theme::text()),
        )
}

pub fn titled(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    detail_color: Hsla,
) -> Div {
    let detail = detail.into();
    v_flex()
        .flex_1()
        .min_w(px(0.))
        .gap(px(2.))
        .child(
            div()
                .text_size(px(14.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(title.into()),
        )
        .when(!detail.is_empty(), |column| {
            column.child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(detail_color)
                    .child(detail),
            )
        })
}

pub fn row() -> Div {
    h_flex()
        .w_full()
        .px(px(ROW_INSET))
        .py(px(ROW_PADDING))
        .gap(px(ROW_GAP))
        .items_center()
        .rounded(px(ROW_RADIUS))
}

pub fn control_row(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    row()
        .child(titled(title, detail, theme::text_muted()))
        .child(control)
}

pub fn toggle_row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    detail_color: Hsla,
    on: bool,
) -> Stateful<Div> {
    row()
        .id(id)
        .cursor_pointer()
        .hover(|row| row.bg(theme::popup_hover()))
        .child(titled(title, detail, detail_color))
        .child(switch(on))
}

pub fn caption(label: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .px(px(ROW_INSET))
        .pt(px(18.))
        .pb(px(6.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(label.into())
}

pub fn text_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(BUTTON_HEIGHT))
        .px(px(14.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(BUTTON_RADIUS))
        .border_1()
        .border_color(theme::border_strong())
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .hover(|button| button.bg(theme::popup_hover()))
        .child(label.into())
}

pub fn select_selector(id: &str) -> String {
    format!("settings-select/{id}")
}

pub fn select(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    choices: Vec<Choice>,
) -> impl IntoElement {
    select_with_width(id, label, SELECT_WIDTH, disabled, choices)
}

pub fn select_with_width(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    width: f32,
    disabled: bool,
    choices: Vec<Choice>,
) -> impl IntoElement {
    let scrollable = choices.len() > SCROLL_MENU_AFTER;
    let id: ElementId = id.into();
    Button::new(id.clone())
        .debug_selector(|| select_selector(&id.to_string()))
        .outline()
        .w(px(width))
        .flex_none()
        .disabled(disabled)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap(px(8.))
                .items_center()
                .justify_between()
                .child(div().flex_1().min_w_0().truncate().child(label.into()))
                .child(icon(IconName::ChevronDown, 12., theme::text_muted())),
        )
        .dropdown_menu(move |menu, _, _| {
            let menu = if scrollable {
                menu.max_h(px(MENU_MAX_HEIGHT)).scrollable(true)
            } else {
                menu
            };
            choices.iter().fold(menu, |menu, choice| {
                let choose = choice.choose.clone();
                menu.item(
                    PopupMenuItem::new(choice.label.clone())
                        .checked(choice.checked)
                        .on_click(move |_, _, cx| choose(cx)),
                )
            })
        })
        .listbox()
}
