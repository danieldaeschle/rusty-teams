use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::settings_controls::Choice;
use crate::theme;
use crate::views::widgets::icon;

pub const LABEL_WIDTH: f32 = 92.;
const SELECT_HEIGHT: f32 = 32.;
const MENU_HEIGHT: f32 = 320.;
const MENU_WIDTH: f32 = 380.;
const ROW_WIDTH: f32 = MENU_WIDTH - 56.;
const DOT_SIZE: f32 = 6.;

#[derive(Clone)]
pub struct MenuRow {
    pub label: String,
    pub meta: Option<String>,
    pub running: bool,
    pub selected: bool,
    pub choose: Choice,
}

#[derive(Clone)]
pub enum MenuPart {
    Header(String),
    Separator,
    Row(MenuRow),
}

pub fn picker_row(label: &'static str, control: impl IntoElement) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_faint())
                .child(label),
        )
        .child(div().flex_1().min_w(px(0.)).child(control))
}

fn meta_text(meta: String, running: bool) -> Div {
    h_flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .text_size(px(11.))
        .text_color(theme::text_faint())
        .when(running, |meta_row| {
            meta_row.child(
                div()
                    .size(px(DOT_SIZE))
                    .rounded_full()
                    .bg(theme::accent_text()),
            )
        })
        .child(meta)
}

fn select_body(value: String, meta: Option<String>, running: bool, chevron: bool) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(13.))
                .child(value),
        )
        .children(meta.map(|meta| meta_text(meta, running)))
        .when(chevron, |body| {
            body.child(icon(IconName::ChevronDown, 14., theme::text_muted()))
        })
}

fn menu_from(menu: PopupMenu, parts: &[MenuPart]) -> PopupMenu {
    parts.iter().fold(menu, |menu, part| match part {
        MenuPart::Header(text) => menu.item(PopupMenuItem::label(text.clone())),
        MenuPart::Separator => menu.item(PopupMenuItem::separator()),
        MenuPart::Row(row) => {
            let choose = row.choose.clone();
            let (label, meta, running) = (row.label.clone(), row.meta.clone(), row.running);
            menu.item(
                PopupMenuItem::element(move |_, _| {
                    select_body(label.clone(), meta.clone(), running, false).w(px(ROW_WIDTH))
                })
                .checked(row.selected)
                .on_click(move |_, window, cx| choose(window, cx)),
            )
        }
    })
}

pub fn select(
    id: &'static str,
    value: String,
    meta: Option<String>,
    running: bool,
    parts: Vec<MenuPart>,
) -> impl IntoElement {
    Button::new(id)
        .ghost()
        .w_full()
        .h(px(SELECT_HEIGHT))
        .px(px(12.))
        .rounded(px(8.))
        .bg(theme::surface_raised())
        .child(select_body(value, meta, running, true))
        .dropdown_menu(move |menu, _, _| {
            menu_from(
                menu.min_w(px(MENU_WIDTH))
                    .max_w(px(MENU_WIDTH))
                    .max_h(px(MENU_HEIGHT))
                    .scrollable(true),
                &parts,
            )
        })
}

pub fn locked_select(value: String, meta: &'static str) -> Div {
    h_flex()
        .w_full()
        .h(px(SELECT_HEIGHT))
        .px(px(12.))
        .items_center()
        .rounded(px(8.))
        .bg(theme::surface_raised())
        .text_color(theme::text_faint())
        .child(select_body(value, Some(meta.to_owned()), false, false))
}
