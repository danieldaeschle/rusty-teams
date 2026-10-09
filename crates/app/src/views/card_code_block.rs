use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::CardCodeBlock;

use super::adaptive_card::{CardContext, slot_handler};
use super::widgets::icon;
use crate::theme;

const COLLAPSED_LINES: usize = 10;
const CODE_TEXT_SIZE: f32 = 12.5;
const CODE_LINE_HEIGHT: f32 = 1.45;
const CODE_RADIUS: f32 = 6.;
const CODE_PADDING: f32 = 8.;
const HEADER_PADDING_X: f32 = 10.;
const HEADER_PADDING_Y: f32 = 4.;
const GUTTER_DIGIT_WIDTH: f32 = 8.;
const GUTTER_GAP: f32 = 12.;
const COPY_ICON_SIZE: f32 = 14.;
const TAB_REPLACEMENT: &str = "    ";
const COPY_TOOLTIP: &str = "Copy code";

pub(super) fn code_block_view(
    block: &CardCodeBlock,
    id: &str,
    context: &CardContext,
) -> AnyElement {
    let lines: Vec<String> = block
        .code
        .lines()
        .map(|line| line.replace('\t', TAB_REPLACEMENT))
        .collect();
    let expanded_key = format!("{id}-expanded");
    let expanded = context.state.slot(&expanded_key, 0) == 1;
    let hidden = lines.len().saturating_sub(COLLAPSED_LINES);
    let shown = if expanded {
        lines.len()
    } else {
        lines.len().min(COLLAPSED_LINES)
    };
    let last_number = block.start_line + lines.len().saturating_sub(1);
    let gutter_width = last_number.to_string().len() as f32 * GUTTER_DIGIT_WIDTH;
    let code = block.code.clone();
    let toggle = slot_handler(context, &expanded_key, usize::from(!expanded));
    v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(CODE_RADIUS))
        .bg(theme::code_surface())
        .border_1()
        .border_color(theme::code_header_border())
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .px(px(HEADER_PADDING_X))
                .py(px(HEADER_PADDING_Y))
                .border_b_1()
                .border_color(theme::code_header_border())
                .text_size(px(11.5))
                .text_color(theme::text_muted())
                .child(block.language.clone().unwrap_or_default())
                .child(
                    div()
                        .id(ElementId::Name(format!("{id}-copy").into()))
                        .cursor_pointer()
                        .p(px(2.))
                        .rounded(px(4.))
                        .hover(|button| button.bg(theme::row_hover()))
                        .tooltip(|window, cx| Tooltip::new(COPY_TOOLTIP).build(window, cx))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                        })
                        .child(icon(IconName::Copy, COPY_ICON_SIZE, theme::text_muted())),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .p(px(CODE_PADDING))
                .font_family(context.cx.theme().mono_font_family.clone())
                .text_size(px(CODE_TEXT_SIZE))
                .line_height(relative(CODE_LINE_HEIGHT))
                .children(lines.iter().take(shown).enumerate().map(|(offset, line)| {
                    h_flex()
                        .w_full()
                        .gap(px(GUTTER_GAP))
                        .child(
                            div()
                                .w(px(gutter_width))
                                .flex_none()
                                .text_right()
                                .text_color(theme::text_faint())
                                .child((block.start_line + offset).to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_color(theme::text_strong())
                                .child(if line.is_empty() {
                                    " ".to_owned()
                                } else {
                                    line.clone()
                                }),
                        )
                })),
        )
        .when(hidden > 0, |frame| {
            frame.child(
                div()
                    .id(ElementId::Name(format!("{id}-expand").into()))
                    .w_full()
                    .px(px(HEADER_PADDING_X))
                    .py(px(HEADER_PADDING_Y))
                    .border_t_1()
                    .border_color(theme::code_header_border())
                    .text_size(px(12.))
                    .text_color(theme::accent_text())
                    .cursor_pointer()
                    .hover(|footer| footer.bg(theme::row_hover()))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        toggle(cx)
                    })
                    .child(if expanded {
                        "Collapse".to_owned()
                    } else {
                        format!("Expand ({hidden} more lines)")
                    }),
            )
        })
        .into_any_element()
}
