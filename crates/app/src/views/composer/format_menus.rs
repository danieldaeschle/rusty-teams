use std::ops::Range;

use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{Draft, FormatState, LineKind, MarkKind, SizeStep, TypingStyle};

use super::Composer;
use super::FormatButton;
use crate::theme::{self, ElevatedSurface as _};

const SWATCH: f32 = 24.;
const SWATCH_GAP: f32 = 6.;
const SWATCH_COLUMNS: usize = 4;
const POPUP_PADDING: f32 = 8.;
const MENU_WIDTH: f32 = 180.;
const FONT_SIZE_MENU_WIDTH: f32 = 120.;
const MENU_PADDING: f32 = 4.;
const MENU_ROW_PADDING_X: f32 = 10.;
const MENU_ROW_PADDING_Y: f32 = 6.;
const STYLE_SIZES: [f32; 4] = [13., 20., 17., 15.];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FormatPopup {
    Highlight,
    FontColor,
    ParagraphStyle,
    FontSize,
}

pub(super) struct Swatch {
    pub(super) name: &'static str,
    pub(super) color: u32,
}

/// Teams' font colour palette (`categoryColors`, shades in the compose bundle).
pub(super) const FONT_COLORS: [Swatch; 8] = [
    Swatch {
        name: "Red",
        color: 0xB6424C,
    },
    Swatch {
        name: "Tuscan orange",
        color: 0xCD5937,
    },
    Swatch {
        name: "Sunglow",
        color: 0xFDC030,
    },
    Swatch {
        name: "Pear",
        color: 0xBDCB4C,
    },
    Swatch {
        name: "Eucalyptus",
        color: 0x2B9B62,
    },
    Swatch {
        name: "Faded jade",
        color: 0x37797B,
    },
    Swatch {
        name: "Fun blue",
        color: 0x1E53A3,
    },
    Swatch {
        name: "Tyrian purple",
        color: 0xA5397A,
    },
];

pub(super) const HIGHLIGHT_COLORS: [Swatch; 8] = [
    Swatch {
        name: "Kobi red",
        color: 0xDF9299,
    },
    Swatch {
        name: "Rose bud",
        color: 0xF4A593,
    },
    Swatch {
        name: "Goldenrod",
        color: 0xFDD472,
    },
    Swatch {
        name: "Primrose",
        color: 0xE5F18F,
    },
    Swatch {
        name: "Mantis",
        color: 0x82CDA8,
    },
    Swatch {
        name: "Regent St. blue",
        color: 0x9DD9DB,
    },
    Swatch {
        name: "Periwinkle grey",
        color: 0xC7D4E8,
    },
    Swatch {
        name: "Pale rose",
        color: 0xEBD3E1,
    },
];

pub(super) const PARAGRAPH_STYLES: [(Option<u8>, &str); 4] = [
    (None, "Paragraph"),
    (Some(1), "Heading 1"),
    (Some(2), "Heading 2"),
    (Some(3), "Heading 3"),
];

/// Teams' Font size menu (`compose_font_size`): Large is `x-large`, Medium removes the size,
/// Small is `xx-small`.
pub(super) const FONT_SIZES: [(Option<SizeStep>, FormatButton, &str); 3] = [
    (Some(SizeStep::Large), FormatButton::SizeLarge, "Large"),
    (None, FormatButton::SizeNormal, "Medium"),
    (Some(SizeStep::Small), FormatButton::SizeSmall, "Small"),
];

pub(super) fn paragraph_label(level: Option<u8>) -> &'static str {
    PARAGRAPH_STYLES
        .iter()
        .find(|(style, _)| *style == level)
        .map_or(PARAGRAPH_STYLES[0].1, |(_, label)| label)
}

impl FormatPopup {
    fn palette(self) -> &'static [Swatch] {
        match self {
            FormatPopup::FontColor => &FONT_COLORS,
            _ => &HIGHLIGHT_COLORS,
        }
    }

    fn clear_label(self) -> &'static str {
        match self {
            FormatPopup::FontColor => "Automatic",
            _ => "No highlight",
        }
    }

    fn marker(self) -> MarkKind {
        match self {
            FormatPopup::FontColor => MarkKind::Color(0),
            _ => MarkKind::Highlight(0),
        }
    }
}

fn swallow_mouse_down<E: InteractiveElement>(element: E) -> E {
    element.on_mouse_down(MouseButton::Left, |_, window, cx| {
        window.prevent_default();
        cx.stop_propagation();
    })
}

impl Composer {
    pub(super) fn toggle_format_popup(&mut self, popup: FormatPopup, cx: &mut Context<Self>) {
        self.table_picker = None;
        self.table_menu = None;
        self.format_popup = (self.format_popup != Some(popup)).then_some(popup);
        if self.format_bar_open() {
            self.toolbar_dismissed = Some(self.selection(cx));
        }
        cx.notify();
    }

    pub(super) fn toggle_format_overflow(&mut self, cx: &mut Context<Self>) {
        self.table_picker = None;
        self.table_menu = None;
        self.format_popup = None;
        self.format_overflow = !self.format_overflow;
        cx.notify();
    }

    fn close_format_popups(&mut self) {
        self.format_popup = None;
        self.format_overflow = false;
    }

    fn apply_font_size(&mut self, size: Option<SizeStep>, cx: &mut Context<Self>) {
        self.close_format_popups();
        self.set_size(size, cx);
    }

    pub(super) fn render_overflow_surface(&self, menu: Div, cx: &mut Context<Self>) -> AnyElement {
        menu.id("format-overflow-menu")
            .rounded(px(10.))
            .flyout_surface()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                let nested_open = this.format_popup.is_some()
                    || this.table_picker.is_some()
                    || this.table_menu.is_some();
                if !nested_open {
                    this.format_overflow = false;
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    /// Text typed at a collapsed cursor takes the change, a selection gets it at once.
    fn change_style(
        &mut self,
        typing: impl FnOnce(&mut TypingStyle, &[MarkKind]),
        apply: impl FnOnce(&mut Draft, Range<usize>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = self.selection(cx);
        if selection.is_empty() {
            let cursor = selection.start;
            let mut pending = self
                .pending_style
                .take()
                .filter(|pending| pending.at == cursor)
                .unwrap_or(TypingStyle {
                    at: cursor,
                    ..TypingStyle::default()
                });
            typing(&mut pending, &self.draft.style_at(cursor));
            self.pending_style = (!pending.is_empty()).then_some(pending);
        } else {
            apply(&mut self.draft, selection);
            self.commit(None, window, cx);
        }
        cx.notify();
    }

    fn apply_color(
        &mut self,
        popup: FormatPopup,
        color: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_format_popups();
        match popup {
            FormatPopup::FontColor => self.change_style(
                |pending, active| pending.set_color(color, active),
                |draft, range| draft.set_color(range, color),
                window,
                cx,
            ),
            _ => self.change_style(
                |pending, active| pending.set_highlight(color, active),
                |draft, range| draft.set_highlight(range, color),
                window,
                cx,
            ),
        }
    }

    pub(super) fn clear_formatting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        if selection.is_empty() {
            self.draft.clear_formatting(selection.clone());
            self.commit(None, window, cx);
        }
        self.change_style(
            |pending, active| pending.clear(active),
            |draft, range| draft.clear_formatting(range),
            window,
            cx,
        );
    }

    pub(super) fn set_paragraph_style(
        &mut self,
        level: Option<u8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_format_popups();
        if self.cell_has_focus(window, cx) {
            return;
        }
        let selection = self.selection(cx);
        self.draft.set_heading(selection, level);
        self.commit(None, window, cx);
        cx.notify();
    }

    pub(super) fn indent_blocks(
        &mut self,
        outdent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = self.selection(cx);
        self.draft.indent_blocks(selection, outdent);
        self.commit(None, window, cx);
        cx.notify();
    }

    pub(super) fn insert_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cell_has_focus(window, cx) {
            return;
        }
        let cursor = self.draft.insert_rule(self.selection(cx).end);
        self.commit(Some(cursor), window, cx);
        cx.notify();
    }

    pub(super) fn toggle_code_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cell_has_focus(window, cx) {
            return;
        }
        self.toggle_lines(LineKind::Code(None), window, cx);
    }

    fn current_color(&self, popup: FormatPopup, cx: &App) -> Option<u32> {
        let selection = self.selection(cx);
        let marker = popup.marker();
        if !selection.is_empty() {
            return self.draft.color_in(selection, &marker);
        }
        let style = self.draft.style_at(selection.start);
        let style = match self.pending_style.as_ref() {
            Some(pending) if pending.at == selection.start => pending.apply(style),
            _ => style,
        };
        style.into_iter().find_map(|kind| match kind {
            MarkKind::Color(color) if marker.same_format(&kind) => Some(color),
            MarkKind::Highlight(color) if marker.same_format(&kind) => Some(color),
            _ => None,
        })
    }

    pub(super) fn render_format_popup(
        &self,
        popup: FormatPopup,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.format_popup != Some(popup) {
            return None;
        }
        let content = match popup {
            FormatPopup::ParagraphStyle => self.render_paragraph_menu(cx),
            FormatPopup::FontSize => self.render_font_size_menu(cx),
            _ => self.render_color_picker(popup, cx),
        };
        Some(
            content
                .id(ElementId::Name(format!("format-popup-{popup:?}").into()))
                .rounded(px(10.))
                .flyout_surface()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.format_popup = None;
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    fn render_color_picker(&self, popup: FormatPopup, cx: &mut Context<Self>) -> Div {
        let current = self.current_color(popup, cx);
        let rows = popup.palette().chunks(SWATCH_COLUMNS).map(|row| {
            h_flex()
                .gap(px(SWATCH_GAP))
                .children(row.iter().map(|swatch| {
                    let color = swatch.color;
                    let name = swatch.name;
                    let selected = current == Some(color);
                    swallow_mouse_down(
                        div()
                            .id(ElementId::Name(
                                format!("swatch-{popup:?}-{color:06x}").into(),
                            ))
                            .size(px(SWATCH))
                            .rounded(px(SWATCH / 2.))
                            .bg(rgb(color))
                            .cursor_pointer()
                            .border_2()
                            .border_color(if selected {
                                theme::text()
                            } else {
                                transparent_black()
                            })
                            .hover(|swatch| swatch.opacity(0.85))
                            .tooltip(move |window, cx| Tooltip::new(name).build(window, cx)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply_color(popup, Some(color), window, cx)
                    }))
                }))
        });
        let clear = swallow_mouse_down(
            div()
                .id(ElementId::Name(format!("swatch-{popup:?}-clear").into()))
                .w_full()
                .py(px(MENU_ROW_PADDING_Y))
                .rounded(px(6.))
                .flex()
                .justify_center()
                .text_size(px(12.))
                .text_color(theme::text())
                .cursor_pointer()
                .hover(|row| row.bg(theme::popup_hover())),
        )
        .on_click(cx.listener(move |this, _, window, cx| this.apply_color(popup, None, window, cx)))
        .child(popup.clear_label());
        v_flex()
            .p(px(POPUP_PADDING))
            .gap(px(SWATCH_GAP))
            .children(rows)
            .child(clear)
    }

    fn render_font_size_menu(&self, cx: &mut Context<Self>) -> Div {
        let selection = self.selection(cx);
        let rows = FONT_SIZES.iter().map(|(size, button, label)| {
            let size = *size;
            let current =
                size.is_some() && self.format_state(*button, &selection) == FormatState::On;
            swallow_mouse_down(
                div()
                    .id(ElementId::Name(format!("font-size-{label}").into()))
                    .px(px(MENU_ROW_PADDING_X))
                    .py(px(MENU_ROW_PADDING_Y))
                    .rounded(px(6.))
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .when(current, |row| row.font_weight(FontWeight::BOLD))
                    .cursor_pointer()
                    .hover(|row| row.bg(theme::popup_hover())),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.apply_font_size(size, cx)))
            .child(*label)
        });
        v_flex()
            .min_w(px(FONT_SIZE_MENU_WIDTH))
            .p(px(MENU_PADDING))
            .children(rows)
    }

    fn render_paragraph_menu(&self, cx: &mut Context<Self>) -> Div {
        let current = self.draft.heading_at(self.selection(cx).start);
        let rows = PARAGRAPH_STYLES
            .iter()
            .zip(STYLE_SIZES)
            .map(|((level, label), size)| {
                let level = *level;
                swallow_mouse_down(
                    div()
                        .id(ElementId::Name(format!("paragraph-style-{level:?}").into()))
                        .px(px(MENU_ROW_PADDING_X))
                        .py(px(MENU_ROW_PADDING_Y))
                        .rounded(px(6.))
                        .text_size(px(size))
                        .text_color(theme::text())
                        .when(level.is_some(), |row| row.font_weight(FontWeight::BOLD))
                        .cursor_pointer()
                        .when(level == current, |row| row.bg(theme::popup_hover()))
                        .hover(|row| row.bg(theme::popup_hover())),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_paragraph_style(level, window, cx)
                }))
                .child(*label)
            });
        v_flex()
            .min_w(px(MENU_WIDTH))
            .p(px(MENU_PADDING))
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    use teams_core::SizeStep;

    use super::{FONT_COLORS, FONT_SIZES, HIGHLIGHT_COLORS, PARAGRAPH_STYLES, paragraph_label};

    #[test]
    fn font_sizes_read_like_teams() {
        assert_eq!(
            FONT_SIZES.map(|(_, _, label)| label),
            ["Large", "Medium", "Small"]
        );
        assert_eq!(
            FONT_SIZES.map(|(size, _, _)| size),
            [Some(SizeStep::Large), None, Some(SizeStep::Small)]
        );
    }

    #[test]
    fn palettes_hold_the_eight_teams_colours_each() {
        assert_eq!(FONT_COLORS.len(), 8);
        assert_eq!(HIGHLIGHT_COLORS.len(), 8);
        assert_eq!(FONT_COLORS[0].color, 0xB6424C);
        assert_eq!(HIGHLIGHT_COLORS[2].name, "Goldenrod");
    }

    #[test]
    fn paragraph_styles_read_like_teams() {
        assert_eq!(
            PARAGRAPH_STYLES.map(|(_, label)| label),
            ["Paragraph", "Heading 1", "Heading 2", "Heading 3"]
        );
        assert_eq!(paragraph_label(Some(2)), "Heading 2");
        assert_eq!(paragraph_label(None), "Paragraph");
    }
}
