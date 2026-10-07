use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::blocks::{Block, Inline, StyleFlags};
use crate::theme;

pub fn render_blocks(blocks: &[Block], id: &str, own: bool, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap(px(6.))
        .children(
            blocks
                .iter()
                .enumerate()
                .map(|(index, block)| render_block(block, format!("{id}-{index}"), own, cx)),
        )
        .into_any_element()
}

fn render_block(block: &Block, id: String, own: bool, cx: &App) -> AnyElement {
    match block {
        Block::Paragraph(inline) => render_inline(inline, id, own),
        Block::Code(code) => div()
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .rounded(px(6.))
            .bg(theme::code_background())
            .border_1()
            .border_color(theme::border_strong())
            .text_color(theme::text_strong())
            .font_family(code_font(cx))
            .text_size(px(12.5))
            .line_height(relative(1.5))
            .child(code.clone())
            .into_any_element(),
        Block::Quote(children) => div()
            .w_full()
            .px(px(8.))
            .py(px(3.))
            .border_l(px(3.))
            .border_color(theme::accent())
            .text_size(px(12.5))
            .text_color(theme::text_muted())
            .child(render_blocks(children, &id, own, cx))
            .into_any_element(),
        Block::ListItem(inline) => h_flex()
            .w_full()
            .gap(px(8.))
            .items_start()
            .child(div().child("\u{2022}"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(render_inline(inline, id, own)),
            )
            .into_any_element(),
    }
}

fn code_font(cx: &App) -> SharedString {
    use gpui_kit::component::ActiveTheme as _;
    cx.theme().mono_font_family.clone()
}

fn render_inline(inline: &Inline, id: String, own: bool) -> AnyElement {
    if inline.segments.iter().any(|segment| segment.style.mention) {
        return render_inline_with_chips(inline, &id, own);
    }
    let highlights = inline
        .segments
        .iter()
        .filter(|segment| !segment.style.is_plain())
        .map(|segment| (segment.range.clone(), highlight_for(segment.style, own)))
        .collect::<Vec<_>>();
    let styled = StyledText::new(inline.text.clone()).with_highlights(highlights);
    let links = inline.links();
    if links.is_empty() {
        return div()
            .w_full()
            .text_color(theme::text())
            .child(styled)
            .into_any_element();
    }
    let ranges = links
        .iter()
        .map(|(range, _)| range.clone())
        .collect::<Vec<_>>();
    let urls = links.into_iter().map(|(_, url)| url).collect::<Vec<_>>();
    div()
        .w_full()
        .text_color(theme::text())
        .child(
            InteractiveText::new(ElementId::Name(id.into()), styled).on_click(
                ranges,
                move |range_index, _, cx| {
                    if let Some(url) = urls.get(range_index) {
                        cx.open_url(url);
                    }
                },
            ),
        )
        .into_any_element()
}

fn highlight_for(style: StyleFlags, own: bool) -> HighlightStyle {
    let mut highlight = HighlightStyle::default();
    if style.bold {
        highlight.font_weight = Some(FontWeight::BOLD);
    }
    if style.italic {
        highlight.font_style = Some(FontStyle::Italic);
    }
    if style.code {
        highlight.background_color = Some(theme::code_background());
    }
    if style.link {
        let color = if own {
            theme::accent_soft()
        } else {
            theme::accent_text()
        };
        highlight.color = Some(color);
        highlight.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: false,
        });
    }
    highlight
}

fn render_inline_with_chips(inline: &Inline, id: &str, own: bool) -> AnyElement {
    let mut pieces: Vec<AnyElement> = Vec::new();
    for (segment_index, segment) in inline.segments.iter().enumerate() {
        let text = &inline.text[segment.range.clone()];
        if segment.style.mention {
            pieces.push(mention_chip(text.trim(), own));
            continue;
        }
        for (word_index, word) in text.split_inclusive(' ').enumerate() {
            let mut piece = div()
                .id(ElementId::Name(
                    format!("{id}-{segment_index}-{word_index}").into(),
                ))
                .whitespace_nowrap()
                .child(StyledText::new(word.to_owned()).with_highlights([(
                    0..word.len(),
                    highlight_for(segment.style, own),
                )]));
            if let Some(url) = segment.link.clone() {
                piece = piece
                    .cursor_pointer()
                    .on_click(move |_, _, cx| cx.open_url(&url));
            }
            pieces.push(piece.into_any_element());
        }
    }
    div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .text_color(theme::text())
        .children(pieces)
        .into_any_element()
}

fn mention_chip(text: &str, own: bool) -> AnyElement {
    div()
        .mx(px(1.))
        .px(px(6.))
        .rounded(px(6.))
        .bg(theme::mention_background(own))
        .text_color(theme::mention_text())
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .child(text.to_owned())
        .into_any_element()
}
