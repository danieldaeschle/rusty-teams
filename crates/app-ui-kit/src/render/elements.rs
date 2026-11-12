use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::blocks::{Block, Inline, StyleFlags, TableCell};
use super::flow_text::{FlowSegment, FlowText};
use super::hooks::jump_to_quoted;
use super::selectable::{Pill, SelectableRichText};
use super::syntax::{self, Token};
use crate::widgets::symbol;
use app_base::theme;

const LIST_GAP: f32 = 2.;
const INDENT_STEP: f32 = teams_core::INDENT_PIXELS as f32;
const MARKER_WIDTH: f32 = 18.;
const WIDE_MARKER_WIDTH: f32 = 26.;
const BULLETS: [&str; 3] = ["\u{2022}", "\u{25e6}", "\u{25aa}"];
const CODE_HEADER_MIN_LINES: usize = 3;
const PLAIN_LANGUAGE: &str = "text";
const MIN_CONTRAST: f32 = 4.5;
const HIGHLIGHT_ALPHA: f32 = 0.3;
const CODE_LINE_PADDING_X: f32 = 6.;
const CODE_LINE_PADDING_Y: f32 = 4.;
const CODE_LINE_RADIUS: f32 = 4.;
pub const CODE_LINE_TAG: &str = "message/code-line";
pub const CODE_BLOCK_TAG: &str = "message/code-block";

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
        Block::CodeLine { code } => render_code_line(code, id, own, cx),
        Block::Reply {
            message_id,
            children,
        } => div()
            .id(SharedString::from(format!("{id}-reply")))
            .w_full()
            .when_some(message_id.clone(), |quote, message_id| {
                quote
                    .cursor_pointer()
                    .hover(|quote| quote.bg(theme::row_hover()))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        jump_to_quoted(message_id.clone(), cx);
                    })
            })
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
        Block::Indent { level, children } => div()
            .w_full()
            .pl(px(f32::from(*level) * INDENT_STEP))
            .child(render_block_list(children, &id, own, depth, cx))
            .into_any_element(),
        Block::List {
            ordered,
            start,
            items,
        } => render_list(*ordered, *start, items, &id, own, depth, cx),
        Block::Image { .. } => div().into_any_element(),
        Block::Table { header, rows } => render_table(*header, rows, &id, own, cx, &|_| None),
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
        .debug_selector(|| CODE_BLOCK_TAG.into())
        .w_full()
        .rounded(px(8.))
        .overflow_hidden()
        .border_1()
        .border_color(theme::code_frame())
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

fn render_code_line(code: &str, id: String, own: bool, cx: &App) -> AnyElement {
    h_flex()
        .w_full()
        .child(
            div()
                .debug_selector(|| CODE_LINE_TAG.into())
                .min_w_0()
                .flex_shrink(1.)
                .px(px(CODE_LINE_PADDING_X))
                .py(px(CODE_LINE_PADDING_Y))
                .rounded(px(CODE_LINE_RADIUS))
                .border_1()
                .border_color(theme::code_frame())
                .font_family(code_font(cx))
                .text_size(px(12.5))
                .line_height(relative(1.5))
                .text_color(code_text(own))
                .child(SelectableRichText::new(
                    ElementId::Name(id.into()),
                    code.to_owned(),
                    Vec::new(),
                )),
        )
        .into_any_element()
}

fn code_text(own: bool) -> Hsla {
    if own {
        theme::accent_soft()
    } else {
        theme::red_soft()
    }
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

/// `image` draws an image of a cell; cells without a drawn image show only their text.
pub fn render_table(
    header: bool,
    rows: &[Vec<TableCell>],
    id: &str,
    own: bool,
    cx: &App,
    image: &dyn Fn(&str) -> Option<AnyElement>,
) -> AnyElement {
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
                        Some(content) => cell
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .children(content.images.iter().filter_map(|url| image(url)))
                            .when(!content.inline.is_blank(), |cell| {
                                cell.child(render_inline(
                                    &content.inline,
                                    format!("{id}-{row_index}-{column}"),
                                    own,
                                    cx,
                                ))
                            }),
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

const KEYCAP: char = '\u{20e3}';
const VARIATION_SELECTOR: char = '\u{fe0f}';

/// Font fallback maps the keycap base digit to the text font and leaves the combining keycap as a box.
pub fn keycap_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    for (keycap_at, _) in text.match_indices(KEYCAP) {
        let before = &text[..keycap_at];
        let before = before.strip_suffix(VARIATION_SELECTOR).unwrap_or(before);
        let Some(base) = before.chars().next_back() else {
            continue;
        };
        if base.is_ascii_digit() || base == '#' || base == '*' {
            ranges.push(before.len() - base.len_utf8()..keycap_at + KEYCAP.len_utf8());
        }
    }
    ranges
}

fn render_inline(inline: &Inline, id: String, own: bool, cx: &App) -> AnyElement {
    let highlights = inline
        .segments
        .iter()
        .filter(|segment| !segment.style.is_plain())
        .map(|segment| (segment.range.clone(), highlight_for(segment.style, own)))
        .collect::<Vec<_>>();
    let mono = code_font(cx);
    let emoji_family = SharedString::from(theme::EMOJI_FONT_FAMILY);
    let mut font_overrides: Vec<_> = inline
        .segments
        .iter()
        .filter(|segment| segment.style.code)
        .map(|segment| (segment.range.clone(), mono.clone()))
        .chain(
            keycap_ranges(&inline.text)
                .into_iter()
                .map(|range| (range, emoji_family.clone())),
        )
        .chain(
            inline
                .segments
                .iter()
                .filter(|segment| segment.emoji.is_some())
                .map(|segment| (segment.range.clone(), emoji_family.clone())),
        )
        .collect();
    font_overrides.sort_by_key(|(range, _)| range.start);
    let emoji = inline.emoji();
    let highlights = highlights
        .into_iter()
        .chain(emoji.iter().map(|(range, _)| {
            (
                range.clone(),
                HighlightStyle {
                    color: Some(transparent_black()),
                    ..Default::default()
                },
            )
        }))
        .collect::<Vec<_>>();
    let inline_images = emoji
        .into_iter()
        .filter_map(|(range, url)| Some((range, super::hooks::inline_image(&url, cx)?)))
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
                highlight: match segment.emoji {
                    Some(_) => HighlightStyle {
                        color: Some(transparent_black()),
                        ..Default::default()
                    },
                    None => highlight_for(segment.style, own),
                },
                family: if segment.emoji.is_some() {
                    Some(SharedString::from(theme::EMOJI_FONT_FAMILY))
                } else {
                    segment.style.code.then(|| mono.clone())
                },
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
                .pills(pills)
                .inline_images(inline_images),
        )
        .into_any_element()
}

fn pill_for(range: std::ops::Range<usize>, style: StyleFlags, own: bool) -> Option<Pill> {
    if style.code {
        return Some(Pill {
            range,
            fill: theme::inline_code_fill(own),
            border: Some(theme::code_frame()),
            radius: px(4.),
        });
    }
    Some(Pill {
        range,
        fill: highlight_fill(style.background?),
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
        highlight.color = Some(code_text(own));
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

pub fn highlight_fill(color: u32) -> Hsla {
    let mut fill: Hsla = rgb(color).into();
    fill.a = HIGHLIGHT_ALPHA;
    fill
}

fn readable(color: u32, own: bool) -> Hsla {
    readable_on(
        color,
        if own {
            theme::bubble_own()
        } else {
            theme::bubble_other()
        },
    )
}

/// `color` moved towards black or white until it reads on `background`.
pub fn readable_on(color: u32, background: Hsla) -> Hsla {
    let background = Rgba::from(background);
    let mut foreground = rgb(color);
    let target = if luminance(background) > 0.5 { 0. } else { 1. };
    for _ in 0..10 {
        if contrast(foreground, background) >= MIN_CONTRAST {
            break;
        }
        foreground = Rgba {
            r: foreground.r + (target - foreground.r) * 0.2,
            g: foreground.g + (target - foreground.g) * 0.2,
            b: foreground.b + (target - foreground.b) * 0.2,
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

    use super::{MIN_CONTRAST, contrast, keycap_ranges, readable};
    use app_base::theme;

    #[test]
    fn keycap_sequences_cover_the_base_the_selector_and_the_keycap() {
        let text = "\u{1f522} 4\u{fe0f}\u{20e3} After #\u{20e3} x\u{20e3}";
        let found: Vec<&str> = keycap_ranges(text).into_iter().map(|range| &text[range]).collect();
        assert_eq!(found, ["4\u{fe0f}\u{20e3}", "#\u{20e3}"]);
    }

    #[test]
    fn dark_text_color_is_lifted_to_readable_contrast() {
        let lifted = Rgba::from(readable(0x000080, false));
        assert!(contrast(lifted, Rgba::from(theme::bubble_other())) >= MIN_CONTRAST);
    }

    #[test]
    fn light_text_color_stays_as_sent() {
        assert_eq!(Rgba::from(readable(0xffd700, false)), rgb(0xffd700));
    }

    #[test]
    fn light_text_color_is_darkened_on_a_light_bubble() {
        theme::set_thread_shade(Some(theme::Shade::Light));
        let darkened = Rgba::from(readable(0xffd700, false));
        let bubble = Rgba::from(theme::bubble_other());
        theme::set_thread_shade(None);
        assert!(contrast(darkened, bubble) >= MIN_CONTRAST);
        assert!(darkened.r < 1.);
    }
}
