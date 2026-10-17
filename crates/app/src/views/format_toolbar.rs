use std::rc::Rc;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::FormatState;

use super::widgets::symbol;
use crate::theme::{self, ElevatedSurface as _};

const PADDING: f32 = 4.;
const BUTTON: f32 = 28.;
const BUTTON_GAP: f32 = 2.;
const SEPARATOR_MARGIN: f32 = 4.;
const GAP: f32 = 8.;
const ARROW_WIDTH: f32 = 14.;
const ARROW_HEIGHT: f32 = 7.;
const ARROW_MARGIN: f32 = 12.;
const LINK_FIELD_WIDTH: f32 = 240.;
const APPLY_WIDTH: f32 = 64.;
const ACTIVE_ALPHA: f32 = 0.22;
const SMALL_LETTER: f32 = 11.;
const NORMAL_LETTER: f32 = 15.;
const LARGE_LETTER: f32 = 19.;
const SCRIPT_LETTER: f32 = 10.;
const SCRIPT_SHIFT: f32 = 4.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatButton {
    Bold,
    Italic,
    Underline,
    Strike,
    Superscript,
    Subscript,
    SizeSmall,
    SizeNormal,
    SizeLarge,
    Code,
    Link,
    Bulleted,
    Numbered,
    Quote,
    Table,
}

pub const GROUPS: [&[FormatButton]; 6] = [
    &[
        FormatButton::Bold,
        FormatButton::Italic,
        FormatButton::Underline,
        FormatButton::Strike,
    ],
    &[FormatButton::Superscript, FormatButton::Subscript],
    &[
        FormatButton::SizeSmall,
        FormatButton::SizeNormal,
        FormatButton::SizeLarge,
    ],
    &[FormatButton::Code, FormatButton::Link],
    &[
        FormatButton::Bulleted,
        FormatButton::Numbered,
        FormatButton::Quote,
    ],
    &[FormatButton::Table],
];

impl FormatButton {
    fn tooltip(self) -> &'static str {
        match self {
            FormatButton::Bold => "Bold (Ctrl+B)",
            FormatButton::Italic => "Italic (Ctrl+I)",
            FormatButton::Underline => "Underline (Ctrl+U)",
            FormatButton::Strike => "Strikethrough (Ctrl+Shift+X)",
            FormatButton::Superscript => "Superscript (Ctrl+Shift+=)",
            FormatButton::Subscript => "Subscript (Ctrl+=)",
            FormatButton::SizeSmall => "Small text",
            FormatButton::SizeNormal => "Normal text",
            FormatButton::SizeLarge => "Large text",
            FormatButton::Code => "Code (Ctrl+Shift+C)",
            FormatButton::Link => "Link (Ctrl+K)",
            FormatButton::Bulleted => "Bulleted list",
            FormatButton::Numbered => "Numbered list",
            FormatButton::Quote => "Quote",
            FormatButton::Table => "Insert table",
        }
    }

    fn content(self, color: Hsla) -> AnyElement {
        let letter = |text: &'static str| div().text_size(px(15.)).text_color(color).child(text);
        match self {
            FormatButton::Bold => letter("B").font_weight(FontWeight::BOLD).into_any_element(),
            FormatButton::Italic => letter("I").italic().into_any_element(),
            FormatButton::Underline => letter("U").underline().into_any_element(),
            FormatButton::Strike => letter("S").line_through().into_any_element(),
            FormatButton::Superscript => script("x", "2", true, color),
            FormatButton::Subscript => script("x", "2", false, color),
            FormatButton::SizeSmall => sized_letter(SMALL_LETTER, color),
            FormatButton::SizeNormal => sized_letter(NORMAL_LETTER, color),
            FormatButton::SizeLarge => sized_letter(LARGE_LETTER, color),
            FormatButton::Code => symbol("code", 18., color).into_any_element(),
            FormatButton::Link => symbol("link", 18., color).into_any_element(),
            FormatButton::Bulleted => symbol("format_list_bulleted", 18., color).into_any_element(),
            FormatButton::Numbered => symbol("format_list_numbered", 18., color).into_any_element(),
            FormatButton::Quote => symbol("format_quote", 18., color).into_any_element(),
            FormatButton::Table => symbol("table", 18., color).into_any_element(),
        }
    }
}

fn script(base: &'static str, mark: &'static str, raised: bool, color: Hsla) -> AnyElement {
    h_flex()
        .items_baseline()
        .text_color(color)
        .child(div().text_size(px(15.)).child(base))
        .child(
            div()
                .text_size(px(SCRIPT_LETTER))
                .when(raised, |mark| mark.mb(px(SCRIPT_SHIFT)))
                .when(!raised, |mark| mark.mt(px(SCRIPT_SHIFT)))
                .child(mark),
        )
        .into_any_element()
}

fn sized_letter(size: f32, color: Hsla) -> AnyElement {
    div()
        .text_size(px(size))
        .text_color(color)
        .child("A")
        .into_any_element()
}

pub fn bar_width() -> f32 {
    let buttons: usize = GROUPS.iter().map(|group| group.len()).sum();
    let gaps: usize = GROUPS.iter().map(|group| group.len() - 1).sum();
    let separators = (GROUPS.len() - 1) as f32 * (1. + 2. * SEPARATOR_MARGIN);
    2. * PADDING + buttons as f32 * BUTTON + gaps as f32 * BUTTON_GAP + separators
}

pub fn link_width() -> f32 {
    2. * PADDING + LINK_FIELD_WIDTH + BUTTON_GAP * 2. + APPLY_WIDTH
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub left: Pixels,
    pub bottom: Pixels,
    pub arrow_x: Pixels,
}

/// Centered over `center_x`, `GAP` above `top`, kept between the composer edges.
pub fn place(center_x: Pixels, top: Pixels, edges: (Pixels, Pixels), width: f32) -> Placement {
    let width = px(width);
    let (left_edge, right_edge) = edges;
    let left = (center_x - width / 2.)
        .max(left_edge)
        .min(right_edge - width);
    Placement {
        left,
        bottom: top - px(GAP),
        arrow_x: (center_x - left)
            .max(px(ARROW_MARGIN))
            .min(width - px(ARROW_MARGIN)),
    }
}

pub type PressHandler = Rc<dyn Fn(FormatButton, &mut Window, &mut App)>;
pub type ApplyHandler = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum Mode<'a> {
    Bar {
        state: &'a dyn Fn(FormatButton) -> FormatState,
        on_press: PressHandler,
    },
    Link {
        field: &'a Entity<InputState>,
        refused: bool,
        on_apply: ApplyHandler,
    },
}

fn button(format: FormatButton, state: FormatState, on_press: PressHandler) -> impl IntoElement {
    let on = state == FormatState::On;
    let color = if on {
        theme::accent_soft()
    } else {
        theme::text_soft()
    };
    div()
        .id(ElementId::Name(format!("format-{format:?}").into()))
        .relative()
        .size(px(BUTTON))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .when(on, |button| {
            button.bg(theme::accent().opacity(ACTIVE_ALPHA))
        })
        .hover(|button| button.bg(theme::popup_hover()))
        .tooltip(move |window, cx| Tooltip::new(format.tooltip()).build(window, cx))
        .child(format.content(color))
        .when(state == FormatState::Mixed, |button| {
            button.child(
                div()
                    .absolute()
                    .bottom(px(2.))
                    .left(px(7.))
                    .w(px(BUTTON - 16.))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(theme::accent()),
            )
        })
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_click(move |_, window, cx| on_press(format, window, cx))
}

fn separator() -> Div {
    div()
        .w(px(1.))
        .h(px(BUTTON - 8.))
        .mx(px(SEPARATOR_MARGIN))
        .bg(theme::popup_hover())
}

fn bar(state: &dyn Fn(FormatButton) -> FormatState, on_press: PressHandler) -> Div {
    let mut row = h_flex().gap(px(BUTTON_GAP)).items_center();
    for (index, group) in GROUPS.iter().enumerate() {
        if index > 0 {
            row = row.child(separator());
        }
        for format in group.iter().copied() {
            row = row.child(button(format, state(format), on_press.clone()));
        }
    }
    row
}

fn link_editor(field: &Entity<InputState>, refused: bool, on_apply: ApplyHandler) -> Div {
    let row = h_flex()
        .gap(px(BUTTON_GAP * 2.))
        .items_center()
        .child(
            div()
                .w(px(LINK_FIELD_WIDTH))
                .h(px(BUTTON))
                .rounded(px(6.))
                .border_1()
                .border_color(if refused {
                    theme::red()
                } else {
                    theme::accent()
                })
                .bg(theme::popup_hover())
                .child(Input::new(field).appearance(false).bordered(false)),
        )
        .child(
            div()
                .id("format-link-apply")
                .w(px(APPLY_WIDTH))
                .h(px(BUTTON))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .bg(theme::accent())
                .text_color(theme::on_accent())
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|button| button.opacity(0.9))
                .child("Apply")
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| on_apply(window, cx)),
        );
    v_flex().child(row).when(refused, |editor| {
        editor.child(
            div()
                .pt(px(4.))
                .px(px(2.))
                .text_size(px(11.5))
                .text_color(theme::red_soft())
                .child("Only http, https and mailto links"),
        )
    })
}

fn arrow(offset: Pixels) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let tip = point(bounds.origin.x + offset, bounds.origin.y + px(ARROW_HEIGHT));
            let mut builder = PathBuilder::fill();
            builder.move_to(point(
                tip.x - px(ARROW_WIDTH / 2.),
                bounds.origin.y - px(1.),
            ));
            builder.line_to(point(
                tip.x + px(ARROW_WIDTH / 2.),
                bounds.origin.y - px(1.),
            ));
            builder.line_to(point(tip.x, tip.y));
            builder.close();
            if let Ok(path) = builder.build() {
                window.paint_path(path, theme::popup());
            }
        },
    )
    .w_full()
    .h(px(ARROW_HEIGHT))
}

pub fn render(placement: Placement, mode: Mode<'_>) -> AnyElement {
    let content = match mode {
        Mode::Bar { state, on_press } => bar(state, on_press),
        Mode::Link {
            field,
            refused,
            on_apply,
        } => link_editor(field, refused, on_apply),
    };
    deferred(
        anchored()
            .position(point(placement.left, placement.bottom))
            .anchor(Anchor::BottomLeft)
            .child(
                v_flex()
                    .id("format-toolbar")
                    .occlude()
                    .child(
                        div()
                            .p(px(PADDING))
                            .rounded(px(10.))
                            .flyout_surface()
                            .child(content),
                    )
                    .child(arrow(placement.arrow_x)),
            ),
    )
    .with_priority(1)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::{ARROW_MARGIN, BUTTON, FormatButton, GROUPS, bar_width, place};

    #[test]
    fn the_bar_centers_over_the_selection() {
        let placement = place(px(400.), px(300.), (px(100.), px(900.)), 200.);
        assert_eq!(placement.left, px(300.));
        assert_eq!(placement.bottom, px(292.));
        assert_eq!(placement.arrow_x, px(100.));
    }

    #[test]
    fn the_bar_stays_inside_the_composer_and_the_arrow_follows_the_selection() {
        let near_left = place(px(120.), px(300.), (px(100.), px(900.)), 200.);
        assert_eq!(near_left.left, px(100.));
        assert_eq!(near_left.arrow_x, px(20.));
        let near_right = place(px(899.), px(300.), (px(100.), px(900.)), 200.);
        assert_eq!(near_right.left, px(700.));
        assert_eq!(near_right.arrow_x, px(200. - ARROW_MARGIN));
    }

    #[test]
    fn a_bar_wider_than_the_composer_ends_at_its_right_edge() {
        let placement = place(px(560.), px(300.), (px(500.), px(800.)), 400.);
        assert_eq!(placement.left, px(400.));
        assert_eq!(placement.arrow_x, px(160.));
    }

    #[test]
    fn the_bar_holds_fifteen_buttons_in_six_groups_with_the_table_last() {
        assert_eq!(GROUPS.iter().map(|group| group.len()).sum::<usize>(), 15);
        assert_eq!(GROUPS.last().copied(), Some(&[FormatButton::Table][..]));
        assert!(bar_width() > 15. * BUTTON);
    }
}
