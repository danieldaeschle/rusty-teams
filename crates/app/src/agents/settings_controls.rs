use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme;

pub const PAGE_PADDING: f32 = 20.;
const FIELD_HEIGHT: f32 = 32.;
const MONO_SIZE: f32 = 11.5;

pub type Choice = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct MenuEntry {
    pub label: String,
    pub selected: bool,
    pub choose: Choice,
}

pub fn field(
    label: &'static str,
    control: impl IntoElement,
    hint: Option<AnyElement>,
    problem: Option<String>,
) -> Div {
    v_flex()
        .w_full()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_soft())
                .child(label),
        )
        .child(control)
        .children(hint)
        .children(problem.map(|problem| {
            div()
                .text_size(px(12.))
                .text_color(theme::red_soft())
                .child(problem)
        }))
}

pub fn hint(text: &'static str) -> AnyElement {
    div()
        .text_size(px(11.))
        .text_color(theme::text_faint())
        .child(text)
        .into_any_element()
}

pub fn text_field(state: &Entity<InputState>, mono: bool, invalid: bool, cx: &App) -> Div {
    div()
        .w_full()
        .h(px(FIELD_HEIGHT))
        .px(px(12.))
        .flex()
        .items_center()
        .overflow_hidden()
        .rounded(px(8.))
        .bg(theme::surface_raised())
        .border_1()
        .border_color(if invalid {
            theme::red_soft()
        } else {
            theme::surface_raised()
        })
        .when(mono, |field| {
            field
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(MONO_SIZE))
        })
        .child(Input::new(state).appearance(false).bordered(false))
}

pub fn boxed(content: impl IntoElement, invalid: bool, cx: &App) -> Div {
    div()
        .w_full()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(theme::surface_raised())
        .border_1()
        .border_color(if invalid {
            theme::red_soft()
        } else {
            theme::surface_raised()
        })
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(MONO_SIZE))
        .child(content)
}

pub fn raised_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(FIELD_HEIGHT))
        .px(px(12.))
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .bg(theme::surface_raised())
        .hover(|button| button.bg(theme::popup_hover()))
        .text_size(px(13.))
        .child(label)
}

pub fn dropdown(id: &'static str, label: String, entries: Vec<MenuEntry>) -> impl IntoElement {
    Button::new(id)
        .outline()
        .w_full()
        .label(label)
        .icon(IconName::ChevronDown)
        .dropdown_menu(move |menu, _, _| {
            entries.iter().fold(menu, |menu, entry| {
                let choose = entry.choose.clone();
                menu.item(
                    PopupMenuItem::new(entry.label.clone())
                        .checked(entry.selected)
                        .on_click(move |_, window, cx| choose(window, cx)),
                )
            })
        })
}

pub fn link_dropdown(
    id: &'static str,
    label: &'static str,
    entries: Vec<MenuEntry>,
) -> impl IntoElement {
    Button::new(id)
        .link()
        .label(label)
        .dropdown_menu(move |menu, _, _| {
            entries.iter().fold(menu, |menu, entry| {
                let choose = entry.choose.clone();
                menu.item(
                    PopupMenuItem::new(entry.label.clone())
                        .on_click(move |_, window, cx| choose(window, cx)),
                )
            })
        })
}

pub fn section_header(title: &'static str, trailing: Option<AnyElement>) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .px(px(PAGE_PADDING))
        .pt(px(16.))
        .pb(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_faint())
        .child(title)
        .children(trailing)
}
