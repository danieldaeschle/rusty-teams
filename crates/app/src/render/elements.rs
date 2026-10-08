use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::blocks::{Block, Inline, StyleFlags};
use super::flow_text::{FlowSegment, FlowText};
use super::selectable::{Pill, SelectableRichText};
use super::syntax::{self, Token};
use crate::theme;
use crate::views::widgets::symbol;

const LIST_GAP: f32 = 2.;
const MARKER_WIDTH: f32 = 18.;
const WIDE_MARKER_WIDTH: f32 = 26.;
const BULLETS: [&str; 3] = ["\u{2022}", "\u{25e6}", "\u{25aa}"];
const CODE_HEADER_MIN_LINES: usize = 3;
const PLAIN_LANGUAGE: &str = "text";
const MIN_CONTRAST: f32 = 4.5;
const HIGHLIGHT_ALPHA: f32 = 0.3;

pub fn render_blocks(blocks: &[Block], id: &str, own: bool, cx: &App) -> AnyElement {
    render_block_list(blocks, id, own, 0, cx)
}

fn render_block_list(blocks: &[Block], id: &str, own: bool, depth: usize, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap(px(6.))
        .children(
            blocks
                .iter()
                .enumerate()
                .map(|(index, block)| render_block(block, format!("{id}-{index}"), own, depth, cx)),
        )
        .into_any_element()
}

fn render_block(block: &Block, id: String, own: bool, depth: usize, cx: &App) -> AnyElement {
    match block {
        Block::Paragraph(inline) => render_inline(inline, id, own, cx),
        Block::Heading { level, inline } => div()
            .w_full()
            .text_size(px(heading_size(*level)))
            .line_height(relative(1.3))
            .font_weight(FontWeight::BOLD)
            .child(render_inline(inline, id, own, cx))
            .into_any_element(),
        Block::Code { language, code } => render_code(language.as_deref(), code, id, own, cx),
        Block::Reply(children) => div()
            .w_full()
            .px(px(8.))
            .py(px(3.))
            .border_l(px(3.))
            .border_color(theme::accent())
            .rounded_r(px(4.))
            .bg(theme::quote_fill())
            .text_size(px(12.5))
            .text_color(theme::text_muted())
            .child(render_block_list(children, &id, own, depth, cx))
            .into_any_element(),
        Block::Quote(children) => div()
            .w_full()
            .px(px(8.))
            .py(px(2.))
            .border_l(px(3.))
            .border_color(theme::border_strong())
            .rounded_r(px(4.))
            .bg(theme::quote_fill())
            .text_color(theme::text_soft())
            .child(render_block_list(children, &id, own, depth, cx))
            .into_any_element(),
        Block::List {
            ordered,
            start,
            items,
        } => render_list(*ordered, *start, items, &id, own, depth, cx),
        Block::Image { .. } => div().into_any_element(),
        Block::Table { header, rows } => render_table(*header, rows, &id, own, cx),
        Block::Rule => div()
            .w_full()
            .h(px(1.))
            .my(px(2.))
            .bg(theme::rule(own))
            .into_any_element(),
    }
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 18.,
        2 => 16.,
        _ => 14.,
    }
}

fn render_code(language: Option<&str>, code: &str, id: String, own: bool, cx: &App) -> AnyElement {
    let highlights = language
        .and_then(|language| syntax::highlight(code, language))
        .unwrap_or_default()
        .into_iter()
        .map(|(range, token)| (range, token_style(token)))
        .collect();
    let body = div()
        .w_full()
        .px(px(10.))
        .py(px(8.))
        .child(SelectableRichText::new(
            ElementId::Name(id.clone().into()),
            code.to_owned(),
            highlights,
        ));
    let show_header = language.is_some() || code.lines().count() >= CODE_HEADER_MIN_LINES;
    v_flex()
        .w_full()
        .rounded(px(8.))
        .overflow_hidden()
        .bg(theme::code_surface())
        .text_color(theme::text_strong())
        .font_family(code_font(cx))
        .text_size(px(12.5))
        .line_height(relative(1.5))
        .when(show_header, |container| {
            container.child(code_header(
                language.unwrap_or(PLAIN_LANGUAGE),
                code,
                &id,
                own,
            ))
        })
        .child(body)
        .into_any_element()
}

fn code_header(language: &str, code: &str, id: &str, own: bool) -> Div {
    let muted = if own {
        theme::own_meta()
    } else {
        theme::text_muted()
    };
    let code = code.to_owned();
    h_flex()
        .w_full()
        .h(px(26.))
        .pl(px(10.))
        .pr(px(6.))
        .justify_between()
        .items_center()
        .border_b_1()
        .border_color(theme::code_header_border())
        .font_family(theme::font_family())
        .text_size(px(11.))
        .text_color(muted)
        .child(language.to_owned())
        .child(
            h_flex()
                .id(ElementId::Name(format!("{id}-copy").into()))
                .h(px(20.))
                .px(px(6.))
                .gap(px(4.))
                .items_center()
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|button| button.bg(theme::inline_code_fill(false)))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()))
                })
                .child(symbol("content_copy", 13., muted))
                .child("Copy"),
        )
}

fn token_style(token: Token) -> HighlightStyle {
    match token {
        Token::Keyword => HighlightStyle {
            color: Some(theme::accent_text()),
            ..Default::default()
        },
        Token::String | Token::Number => HighlightStyle {
            color: Some(theme::accent_soft()),
            ..Default::default()
        },
        Token::Comment => HighlightStyle {
            color: Some(theme::text_faint()),
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        },
    }
}

fn render_list(
    ordered: bool,
    start: u32,
    items: &[Vec<Block>],
    id: &str,
    own: bool,
    depth: usize,
    cx: &App,
) -> AnyElement {
    let last_number = start as usize + items.len().saturating_sub(1);
    let marker_width = if ordered && last_number >= 10 {
        WIDE_MARKER_WIDTH
    } else {
        MARKER_WIDTH
    };
    let marker_color = if own {
        theme::own_meta()
    } else {
        theme::text_muted()
    };
    v_flex()
        .w_full()
        .gap(px(LIST_GAP))
        .children(items.iter().enumerate().map(|(index, item)| {
            let marker = if ordered {
                format!("{}.", start as usize + index)
            } else {
                BULLETS[depth.min(BULLETS.len() - 1)].to_owned()
            };
            h_flex()
                .w_full()
                .items_start()
                .child(
                    div()
                        .flex_none()
                        .w(px(marker_width))
                        .when(ordered, |marker| marker.pr(px(4.)).text_right())
                        .text_color(marker_color)
                        .child(marker),
                )
                .child(div().flex_1().min_w_0().child(render_list_item(
                    item,
                    &format!("{id}-{index}"),
                    own,
                    depth + 1,
                    cx,
                )))
        }))
        .into_any_element()
}

fn render_list_item(blocks: &[Block], id: &str, own: bool, depth: usize, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap(px(LIST_GAP))
        .children(
            blocks
                .iter()
                .enumerate()
                .map(|(index, block)| render_block(block, format!("{id}-{index}"), own, depth, cx)),
        )
        .into_any_element()
}

fn render_table(header: bool, rows: &[Vec<Inline>], id: &str, own: bool, cx: &App) -> AnyElement {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    v_flex()
        .w_full()
        .border_t_1()
        .border_l_1()
        .border_color(theme::border_strong())
        .text_size(px(13.))
        .children(rows.iter().enumerate().map(|(row_index, row)| {
            let is_header = header && row_index == 0;
            h_flex()
                .w_full()
                .items_stretch()
                .when(is_header, |line| {
                    line.bg(theme::table_header())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_strong())
                })
                .children((0..columns).map(|column| {
                    let cell = div()
                        .flex_1()
                        .min_w_0()
                        .px(px(8.))
                        .py(px(4.))
                        .border_r_1()
                        .border_b_1()
                        .border_color(theme::border_strong());
                    match row.get(column) {
                        Some(inline) => cell.child(render_inline(
                            inline,
                            format!("{id}-{row_index}-{column}"),
                            own,
                            cx,
                        )),
                        None => cell,
                    }
                }))
        }))
        .into_any_element()
}

fn code_font(cx: &App) -> SharedString {
    use gpui_kit::component::ActiveTheme as _;
    cx.theme().mono_font_family.clone()
}

fn render_inline(inline: &Inline, id: String, own: bool, cx: &App) -> AnyElement {
    let highlights = inline
        .segments
        .iter()
        .filter(|segment| !segment.style.is_plain())
        .map(|segment| (segment.range.clone(), highlight_for(segment.style, own)))
        .collect::<Vec<_>>();
    let mono = code_font(cx);
    let font_overrides = inline
        .segments
        .iter()
        .filter(|segment| segment.style.code)
        .map(|segment| (segment.range.clone(), mono.clone()))
        .collect();
    let pills = inline
        .segments
        .iter()
        .filter_map(|segment| pill_for(segment.range.clone(), segment.style, own))
        .collect();
    if inline
        .segments
        .iter()
        .any(|segment| segment.style.is_flowed())
    {
        let segments = inline
            .segments
            .iter()
            .map(|segment| FlowSegment {
                range: segment.range.clone(),
                highlight: highlight_for(segment.style, own),
                family: segment.style.code.then(|| mono.clone()),
                scale: segment.style.font_scale(),
                raise: segment.style.baseline_raise(),
            })
            .collect();
        return div()
            .w_full()
            .text_color(theme::text())
            .child(
                FlowText::new(ElementId::Name(id.into()), inline.text.clone(), segments)
                    .links(inline.links())
                    .pills(pills),
            )
            .into_any_element();
    }
    div()
        .w_full()
        .text_color(theme::text())
        .child(
            SelectableRichText::new(ElementId::Name(id.into()), inline.text.clone(), highlights)
                .links(inline.links())
                .font_overrides(font_overrides)
                .pills(pills),
        )
        .into_any_element()
}

fn pill_for(range: std::ops::Range<usize>, style: StyleFlags, own: bool) -> Option<Pill> {
    if style.code {
        return Some(Pill {
            range,
            fill: theme::inline_code_fill(own),
            border: Some(theme::inline_code_border()),
            radius: px(4.),
        });
    }
    let background = style.background?;
    let mut fill: Hsla = rgb(background).into();
    fill.a = HIGHLIGHT_ALPHA;
    Some(Pill {
        range,
        fill,
        border: None,
        radius: px(3.),
    })
}

fn highlight_for(style: StyleFlags, own: bool) -> HighlightStyle {
    let mut highlight = HighlightStyle::default();
    if style.bold {
        highlight.font_weight = Some(FontWeight::BOLD);
    }
    if style.italic {
        highlight.font_style = Some(FontStyle::Italic);
    }
    if let Some(color) = style.color {
        highlight.color = Some(readable(color, own));
    }
    if style.code {
        highlight.color = Some(if own {
            theme::accent_soft()
        } else {
            theme::text_strong()
        });
    }
    if style.underline {
        highlight.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: None,
            wavy: false,
        });
    }
    if style.strike {
        highlight.color = Some(if own {
            theme::own_meta()
        } else {
            theme::text_muted()
        });
        highlight.strikethrough = Some(StrikethroughStyle {
            thickness: px(1.),
            color: None,
        });
    }
    let accent = if own {
        theme::accent_soft()
    } else {
        theme::accent_text()
    };
    if style.mention {
        highlight.font_weight = Some(FontWeight::BOLD);
        highlight.color = Some(accent);
    }
    if style.link {
        highlight.color = Some(accent);
        highlight.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(accent),
            wavy: false,
        });
    }
    highlight
}

fn readable(color: u32, own: bool) -> Hsla {
    let background = Rgba::from(if own {
        theme::bubble_own()
    } else {
        theme::bubble_other()
    });
    let mut foreground = rgb(color);
    for _ in 0..10 {
        if contrast(foreground, background) >= MIN_CONTRAST {
            break;
        }
        foreground = Rgba {
            r: foreground.r + (1. - foreground.r) * 0.2,
            g: foreground.g + (1. - foreground.g) * 0.2,
            b: foreground.b + (1. - foreground.b) * 0.2,
            a: 1.,
        };
    }
    foreground.into()
}

fn contrast(first: Rgba, second: Rgba) -> f32 {
    let (lighter, darker) = {
        let (a, b) = (luminance(first), luminance(second));
        if a > b { (a, b) } else { (b, a) }
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn luminance(color: Rgba) -> f32 {
    let channel = |value: f32| {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Rgba, rgb};

    use super::{MIN_CONTRAST, contrast, readable};
    use crate::theme;

    #[test]
    fn dark_text_color_is_lifted_to_readable_contrast() {
        let lifted = Rgba::from(readable(0x000080, false));
        assert!(contrast(lifted, Rgba::from(theme::bubble_other())) >= MIN_CONTRAST);
    }

    #[test]
    fn light_text_color_stays_as_sent() {
        assert_eq!(Rgba::from(readable(0xffd700, false)), rgb(0xffd700));
    }
}
