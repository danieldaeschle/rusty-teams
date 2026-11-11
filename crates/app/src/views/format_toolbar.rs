use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::PopupTriggerTooltip as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::FormatState;

use super::widgets::{icon, symbol};
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
const DISABLED_OPACITY: f32 = 0.4;
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
    TableInsertLine,
    TableRemoveLine,
    Highlight,
    FontColor,
    ParagraphStyle,
    ClearFormatting,
    Outdent,
    Indent,
    CodeBlock,
    Rule,
    FontSize,
    More,
}

const STYLE_GROUP: &[FormatButton] = &[
    FormatButton::Bold,
    FormatButton::Italic,
    FormatButton::Underline,
    FormatButton::Strike,
];
const SCRIPT_GROUP: &[FormatButton] = &[FormatButton::Superscript, FormatButton::Subscript];
const SIZE_GROUP: &[FormatButton] = &[FormatButton::FontSize];
const INLINE_GROUP: &[FormatButton] = &[FormatButton::Code, FormatButton::Link];
const LINE_GROUP: &[FormatButton] = &[
    FormatButton::Bulleted,
    FormatButton::Numbered,
    FormatButton::Quote,
];
const TABLE_GROUP: &[FormatButton] = &[
    FormatButton::Table,
    FormatButton::TableInsertLine,
    FormatButton::TableRemoveLine,
];

const FONT_GROUP: &[FormatButton] = &[
    FormatButton::Highlight,
    FormatButton::FontColor,
    FormatButton::FontSize,
    FormatButton::ParagraphStyle,
    FormatButton::ClearFormatting,
];
const PARAGRAPH_GROUP: &[FormatButton] = &[
    FormatButton::Outdent,
    FormatButton::Indent,
    FormatButton::Bulleted,
    FormatButton::Numbered,
];
const INSERT_GROUP: &[FormatButton] = &[
    FormatButton::Quote,
    FormatButton::Link,
    FormatButton::Code,
    FormatButton::CodeBlock,
    FormatButton::Rule,
];

/// The floating bar over a selection: text formats only, tables live in the format bar.
pub const SELECTION_GROUPS: [&[FormatButton]; 5] = [
    STYLE_GROUP,
    SCRIPT_GROUP,
    SIZE_GROUP,
    INLINE_GROUP,
    LINE_GROUP,
];

/// The format bar the Format button opens, in Teams' group order; Teams' last group (undo,
/// redo) is left out, so the table group ends the bar.
pub const BAR_GROUPS: [&[FormatButton]; 6] = [
    STYLE_GROUP,
    SCRIPT_GROUP,
    FONT_GROUP,
    PARAGRAPH_GROUP,
    INSERT_GROUP,
    TABLE_GROUP,
];

pub const PARAGRAPH_STYLE_WIDTH: f32 = 104.;
const SEPARATOR_WIDTH: f32 = 1. + 2. * SEPARATOR_MARGIN;
const MENU_ROW_HEIGHT: f32 = 32.;
const MENU_ROW_PADDING_X: f32 = 10.;
const MENU_PADDING: f32 = 4.;
const MENU_WIDTH: f32 = 220.;

/// The format bar split like Teams' overflow: buttons leave from the end into More options,
/// each side keeps the group boundaries of its own buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarFit {
    pub visible: Vec<Vec<FormatButton>>,
    pub overflow: Vec<Vec<FormatButton>>,
}

fn groups_width<G: AsRef<[FormatButton]>>(groups: &[G]) -> f32 {
    let buttons = groups.iter().flat_map(|group| group.as_ref().iter());
    let widths: f32 = buttons.clone().map(|button| button.width()).sum();
    let gaps = buttons.count().saturating_sub(1) as f32 * BUTTON_GAP;
    let separators = groups.len().saturating_sub(1) as f32 * (SEPARATOR_WIDTH + BUTTON_GAP);
    widths + gaps + separators
}

fn split_groups(groups: &[&[FormatButton]], count: usize) -> BarFit {
    let mut visible = Vec::new();
    let mut overflow = Vec::new();
    let mut taken = 0;
    for group in groups {
        let shown = group.len().min(count - taken.min(count));
        taken += group.len();
        if shown > 0 {
            visible.push(group[..shown].to_vec());
        }
        if shown < group.len() {
            overflow.push(group[shown..].to_vec());
        }
    }
    BarFit { visible, overflow }
}

pub fn fit_groups(available: f32, groups: &[&[FormatButton]]) -> BarFit {
    let total: usize = groups.iter().map(|group| group.len()).sum();
    if groups_width(groups) <= available {
        return split_groups(groups, total);
    }
    let more = BUTTON_GAP + FormatButton::More.width();
    let count = (0..total)
        .rev()
        .find(|count| groups_width(&split_groups(groups, *count).visible) + more <= available)
        .unwrap_or(0);
    split_groups(groups, count)
}

pub const TOGGLE_SHORTCUT: &str = "Ctrl+Shift+X";

pub fn toggle_tooltip(open: bool) -> String {
    let label = if open {
        "Hide Formatting options"
    } else {
        "Show Formatting options"
    };
    format!("{label} ({TOGGLE_SHORTCUT})")
}

pub fn toggle_symbol(open: bool) -> &'static str {
    if open {
        "text_format_fill"
    } else {
        "text_format"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonState {
    pub format: FormatState,
    pub enabled: bool,
    pub label: Option<&'static str>,
}

impl ButtonState {
    pub fn enabled(format: FormatState) -> Self {
        ButtonState {
            format,
            enabled: true,
            label: None,
        }
    }

    pub fn disabled() -> Self {
        ButtonState {
            format: FormatState::Off,
            enabled: false,
            label: None,
        }
    }

    pub fn with_label(self, label: &'static str) -> Self {
        ButtonState {
            label: Some(label),
            ..self
        }
    }
}

impl FormatButton {
    fn tooltip(self) -> &'static str {
        match self {
            FormatButton::Bold => "Bold (Ctrl+B)",
            FormatButton::Italic => "Italic (Ctrl+I)",
            FormatButton::Underline => "Underline (Ctrl+U)",
            FormatButton::Strike => "Strikethrough (Ctrl+Alt+X)",
            FormatButton::Superscript => "Superscript (Ctrl+Shift+=)",
            FormatButton::Subscript => "Subscript (Ctrl+=)",
            FormatButton::SizeSmall => "Small text",
            FormatButton::SizeNormal => "Normal text",
            FormatButton::SizeLarge => "Large text",
            FormatButton::Code => "Code (Ctrl+Shift+C)",
            FormatButton::Link => "Link (Ctrl+K)",
            FormatButton::Bulleted => "Bulleted list",
            FormatButton::Numbered => "Numbered list",
            FormatButton::Quote => "Quote (Ctrl+Alt+4)",
            FormatButton::Table => "Insert table",
            FormatButton::TableInsertLine => "Insert column or row (Ctrl+Alt+=)",
            FormatButton::TableRemoveLine => "Delete column, row, or table (Ctrl+Alt+-)",
            FormatButton::Highlight => "Text highlight colour",
            FormatButton::FontColor => "Font colour",
            FormatButton::ParagraphStyle => "Rich style",
            FormatButton::ClearFormatting => "Clear all formatting",
            FormatButton::Outdent => "Decrease indent",
            FormatButton::Indent => "Increase indent",
            FormatButton::CodeBlock => "Code block (Ctrl+Shift+B)",
            FormatButton::Rule => "Insert horizontal rule",
            FormatButton::FontSize => "Font size",
            FormatButton::More => "More options",
        }
    }

    pub fn menu_label(self) -> &'static str {
        let tooltip = self.tooltip();
        tooltip.split(" (").next().unwrap_or(tooltip)
    }

    fn edits_table(self) -> bool {
        matches!(
            self,
            FormatButton::TableInsertLine | FormatButton::TableRemoveLine
        )
    }

    /// Opens a picker or menu below itself instead of acting at once.
    pub fn opens_popup(self) -> bool {
        matches!(
            self,
            FormatButton::Highlight
                | FormatButton::FontColor
                | FormatButton::FontSize
                | FormatButton::ParagraphStyle
                | FormatButton::Table
                | FormatButton::TableInsertLine
                | FormatButton::TableRemoveLine
                | FormatButton::More
        )
    }

    fn content(self, color: Hsla, label: Option<&'static str>) -> AnyElement {
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
            FormatButton::TableInsertLine => symbol("table_chart", 20., color).into_any_element(),
            FormatButton::TableRemoveLine => symbol("grid_off", 20., color).into_any_element(),
            FormatButton::Highlight => {
                symbol("format_ink_highlighter", 20., color).into_any_element()
            }
            FormatButton::FontColor => symbol("format_color_text", 20., color).into_any_element(),
            FormatButton::ParagraphStyle => h_flex()
                .w_full()
                .px(px(6.))
                .gap(px(4.))
                .items_center()
                .justify_between()
                .text_size(px(13.))
                .text_color(color)
                .child(label.unwrap_or_default())
                .child(icon(IconName::ChevronDown, 12., color))
                .into_any_element(),
            FormatButton::ClearFormatting => symbol("format_clear", 20., color).into_any_element(),
            FormatButton::Outdent => {
                symbol("format_indent_decrease", 20., color).into_any_element()
            }
            FormatButton::Indent => symbol("format_indent_increase", 20., color).into_any_element(),
            FormatButton::CodeBlock => symbol("code_blocks", 20., color).into_any_element(),
            FormatButton::Rule => symbol("horizontal_rule", 20., color).into_any_element(),
            FormatButton::FontSize => symbol("format_size", 20., color).into_any_element(),
            FormatButton::More => icon(IconName::Ellipsis, 18., color).into_any_element(),
        }
    }

    fn width(self) -> f32 {
        match self {
            FormatButton::ParagraphStyle => PARAGRAPH_STYLE_WIDTH,
            _ => BUTTON,
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
    2. * PADDING + groups_width(&SELECTION_GROUPS)
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
        state: &'a dyn Fn(FormatButton) -> ButtonState,
        on_press: PressHandler,
        overlay: &'a mut dyn FnMut(FormatButton) -> Option<AnyElement>,
    },
    Link {
        field: &'a Entity<InputState>,
        refused: bool,
        on_apply: ApplyHandler,
    },
}

fn button(
    format: FormatButton,
    state: ButtonState,
    on_press: PressHandler,
    overlay: Option<AnyElement>,
) -> impl IntoElement {
    let on = state.enabled && state.format == FormatState::On;
    let color = if on {
        theme::accent_soft()
    } else {
        theme::text_soft()
    };
    div()
        .id(ElementId::Name(format!("format-{format:?}").into()))
        .relative()
        .w(px(format.width()))
        .h(px(BUTTON))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .popup_trigger_tooltip_text(format.opens_popup() && on, format.tooltip())
        .child(format.content(color, state.label))
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .when(!state.enabled, |button| button.opacity(DISABLED_OPACITY))
        .when(state.enabled, |button| {
            button
                .cursor_pointer()
                .when(on, |button| {
                    button.bg(theme::accent().opacity(ACTIVE_ALPHA))
                })
                .hover(|button| button.bg(theme::popup_hover()))
                .when(state.format == FormatState::Mixed, |button| {
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
                .on_click(move |_, window, cx| on_press(format, window, cx))
        })
        .children(overlay.map(|overlay| overlay_below(overlay, BUTTON + GAP / 2.)))
}

fn overlay_below(overlay: AnyElement, offset: f32) -> Div {
    div().absolute().top(px(offset)).left_0().child(
        deferred(
            anchored()
                .snap_to_window_with_margin(px(GAP))
                .child(overlay),
        )
        .with_priority(2),
    )
}

fn separator() -> Div {
    div()
        .w(px(1.))
        .h(px(BUTTON - 8.))
        .mx(px(SEPARATOR_MARGIN))
        .bg(theme::popup_hover())
}

fn groups_row<G: AsRef<[FormatButton]>>(
    groups: &[G],
    state: &dyn Fn(FormatButton) -> ButtonState,
    on_press: PressHandler,
    overlay: &mut dyn FnMut(FormatButton) -> Option<AnyElement>,
) -> Div {
    let mut row = h_flex().gap(px(BUTTON_GAP)).items_center();
    for (index, group) in groups.iter().enumerate() {
        if index > 0 {
            row = row.child(separator());
        }
        for format in group.as_ref().iter().copied() {
            row = row.child(button(
                format,
                state(format),
                on_press.clone(),
                overlay(format),
            ));
        }
    }
    row
}

fn bar(
    state: &dyn Fn(FormatButton) -> ButtonState,
    on_press: PressHandler,
    overlay: &mut dyn FnMut(FormatButton) -> Option<AnyElement>,
) -> Div {
    groups_row(&SELECTION_GROUPS, state, on_press, overlay)
}

/// The format bar inside the composer box. `overlay` hangs a popup under a button; the
/// More button's popup is the overflow menu, built with `render_overflow_menu`.
pub fn render_format_bar(
    fit: &BarFit,
    state: &dyn Fn(FormatButton) -> ButtonState,
    on_press: PressHandler,
    overlay: &mut dyn FnMut(FormatButton) -> Option<AnyElement>,
) -> Div {
    let row = groups_row(&fit.visible, state, on_press.clone(), overlay);
    if menu_groups(&fit.overflow, state).is_empty() {
        return row;
    }
    row.child(button(
        FormatButton::More,
        state(FormatButton::More),
        on_press,
        overlay(FormatButton::More),
    ))
}

/// The groups that do not fit, as menu rows with a divider between groups.
pub fn render_overflow_menu(
    groups: &[Vec<FormatButton>],
    state: &dyn Fn(FormatButton) -> ButtonState,
    on_press: PressHandler,
    overlay: &mut dyn FnMut(FormatButton) -> Option<AnyElement>,
) -> Div {
    let mut menu = v_flex().w(px(MENU_WIDTH)).py(px(MENU_PADDING));
    for (index, group) in menu_groups(groups, state).iter().enumerate() {
        if index > 0 {
            menu = menu.child(
                div()
                    .w_full()
                    .h(px(1.))
                    .my(px(MENU_PADDING))
                    .bg(theme::popup_hover()),
            );
        }
        for format in group.iter().copied() {
            menu = menu.child(menu_row(
                format,
                state(format),
                on_press.clone(),
                overlay(format),
            ));
        }
    }
    menu
}

fn menu_groups(
    groups: &[Vec<FormatButton>],
    state: &dyn Fn(FormatButton) -> ButtonState,
) -> Vec<Vec<FormatButton>> {
    groups
        .iter()
        .map(|group| {
            group
                .iter()
                .copied()
                .filter(|format| !format.edits_table() || state(*format).enabled)
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

fn menu_row(
    format: FormatButton,
    state: ButtonState,
    on_press: PressHandler,
    overlay: Option<AnyElement>,
) -> impl IntoElement {
    let on = state.enabled && state.format == FormatState::On;
    let color = if on {
        theme::accent_soft()
    } else {
        theme::text_soft()
    };
    let content = match format {
        FormatButton::ParagraphStyle => symbol("format_h1", 20., color).into_any_element(),
        _ => format.content(color, None),
    };
    div()
        .id(ElementId::Name(
            format!("format-overflow-{format:?}").into(),
        ))
        .relative()
        .mx(px(MENU_PADDING))
        .h(px(MENU_ROW_HEIGHT))
        .flex_none()
        .px(px(MENU_ROW_PADDING_X))
        .flex()
        .items_center()
        .gap(px(GAP))
        .rounded(px(6.))
        .child(div().w(px(BUTTON)).flex().justify_center().child(content))
        .child(
            div()
                .flex_1()
                .text_size(px(13.))
                .text_color(theme::text())
                .child(format.menu_label()),
        )
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .when(!state.enabled, |row| row.opacity(DISABLED_OPACITY))
        .when(state.enabled, |row| {
            row.cursor_pointer()
                .when(on, |row| row.bg(theme::accent().opacity(ACTIVE_ALPHA)))
                .hover(|row| row.bg(theme::popup_hover()))
                .on_click(move |_, window, cx| on_press(format, window, cx))
        })
        .children(overlay.map(|overlay| overlay_below(overlay, MENU_ROW_HEIGHT)))
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
        Mode::Bar {
            state,
            on_press,
            overlay,
        } => bar(state, on_press, overlay),
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

    use super::{
        ARROW_MARGIN, BAR_GROUPS, BUTTON, ButtonState, FormatButton, FormatState, SELECTION_GROUPS,
        bar_width, fit_groups, groups_width, menu_groups, place, toggle_symbol, toggle_tooltip,
    };

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
    fn the_selection_bar_holds_the_text_formats_without_the_table() {
        assert_eq!(
            SELECTION_GROUPS
                .iter()
                .map(|group| group.len())
                .sum::<usize>(),
            12
        );
        assert!(
            SELECTION_GROUPS
                .iter()
                .all(|group| !group.contains(&FormatButton::Table))
        );
        assert!(
            SELECTION_GROUPS
                .iter()
                .any(|group| group == &[FormatButton::FontSize])
        );
        assert!(bar_width() > 12. * BUTTON);
    }

    #[test]
    fn the_format_bar_follows_the_teams_groups_and_keeps_every_selection_format() {
        assert_eq!(
            BAR_GROUPS[BAR_GROUPS.len() - 1],
            &[
                FormatButton::Table,
                FormatButton::TableInsertLine,
                FormatButton::TableRemoveLine
            ][..]
        );
        assert_eq!(
            BAR_GROUPS[2][..3],
            [
                FormatButton::Highlight,
                FormatButton::FontColor,
                FormatButton::FontSize
            ]
        );
        assert_eq!(
            BAR_GROUPS[3],
            &[
                FormatButton::Outdent,
                FormatButton::Indent,
                FormatButton::Bulleted,
                FormatButton::Numbered
            ][..]
        );
        assert_eq!(BAR_GROUPS[4].last(), Some(&FormatButton::Rule));
        for button in SELECTION_GROUPS.iter().flat_map(|group| group.iter()) {
            assert!(BAR_GROUPS.iter().any(|group| group.contains(button)));
        }
    }

    fn flat(groups: &[Vec<FormatButton>]) -> Vec<FormatButton> {
        groups.iter().flatten().copied().collect()
    }

    #[test]
    fn a_wide_bar_shows_every_button_without_more_options() {
        let fit = fit_groups(groups_width(&BAR_GROUPS), &BAR_GROUPS);
        assert_eq!(fit.visible.len(), BAR_GROUPS.len());
        assert!(fit.overflow.is_empty());
    }

    #[test]
    fn single_buttons_leave_from_the_end_and_keep_their_groups() {
        let all: Vec<FormatButton> = BAR_GROUPS
            .iter()
            .flat_map(|group| group.iter())
            .copied()
            .collect();
        let fit = fit_groups(groups_width(&BAR_GROUPS) - 1., &BAR_GROUPS);
        assert_eq!(flat(&fit.overflow), all[all.len() - 2..]);
        assert_eq!(
            fit.overflow,
            vec![vec![
                FormatButton::TableInsertLine,
                FormatButton::TableRemoveLine
            ]]
        );
        assert_eq!(fit.visible.last(), Some(&vec![FormatButton::Table]));
        let mut both = flat(&fit.visible);
        both.extend(flat(&fit.overflow));
        assert_eq!(both, all);
    }

    #[test]
    fn a_cut_between_groups_leaves_no_empty_group_on_either_side() {
        let two = groups_width(&BAR_GROUPS[..2]);
        let fit = fit_groups(two + 2. + BUTTON, &BAR_GROUPS);
        assert_eq!(
            fit.visible,
            vec![BAR_GROUPS[0].to_vec(), BAR_GROUPS[1].to_vec()]
        );
        assert_eq!(fit.overflow.len(), BAR_GROUPS.len() - 2);
        assert!(
            fit.overflow
                .iter()
                .chain(&fit.visible)
                .all(|group| !group.is_empty())
        );
        let fit = fit_groups(two + 2. + BUTTON - 1., &BAR_GROUPS);
        assert_eq!(fit.visible.len(), 2);
        assert_eq!(fit.visible[1], vec![FormatButton::Superscript]);
        assert_eq!(fit.overflow[0], vec![FormatButton::Subscript]);
    }

    #[test]
    fn outside_a_table_the_menu_drops_the_table_edits_and_their_group() {
        let fit = fit_groups(groups_width(&BAR_GROUPS) - 1., &BAR_GROUPS);
        let outside = |format: FormatButton| match format {
            FormatButton::TableInsertLine | FormatButton::TableRemoveLine => {
                ButtonState::disabled()
            }
            _ => ButtonState::enabled(FormatState::Off),
        };
        assert!(menu_groups(&fit.overflow, &outside).is_empty());
        let inside = |_| ButtonState::enabled(FormatState::Off);
        assert_eq!(menu_groups(&fit.overflow, &inside), fit.overflow);
    }

    #[test]
    fn a_tiny_bar_keeps_only_more_options() {
        let fit = fit_groups(BUTTON, &BAR_GROUPS);
        assert!(fit.visible.is_empty());
        assert_eq!(fit.overflow.len(), BAR_GROUPS.len());
    }

    #[test]
    fn overflow_rows_read_without_the_shortcut() {
        assert_eq!(FormatButton::CodeBlock.menu_label(), "Code block");
        assert_eq!(FormatButton::Rule.menu_label(), "Insert horizontal rule");
        assert_eq!(FormatButton::More.tooltip(), "More options");
        assert_eq!(FormatButton::FontSize.tooltip(), "Font size");
    }

    #[test]
    fn the_format_toggle_reads_like_teams() {
        assert_eq!(
            toggle_tooltip(false),
            "Show Formatting options (Ctrl+Shift+X)"
        );
        assert_eq!(
            toggle_tooltip(true),
            "Hide Formatting options (Ctrl+Shift+X)"
        );
        assert_ne!(toggle_symbol(false), toggle_symbol(true));
        assert_eq!(FormatButton::Strike.tooltip(), "Strikethrough (Ctrl+Alt+X)");
    }
}
