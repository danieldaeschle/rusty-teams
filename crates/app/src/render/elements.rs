use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::blocks::{Block, Inline, StyleFlags};
use super::selectable::SelectableRichText;
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
            .child(SelectableRichText::new(
                ElementId::Name(id.into()),
                code.clone(),
                Vec::new(),
            ))
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
    let highlights = inline
        .segments
        .iter()
        .filter(|segment| !segment.style.is_plain())
        .map(|segment| (segment.range.clone(), highlight_for(segment.style, own)))
        .collect::<Vec<_>>();
    div()
        .w_full()
        .text_color(theme::text())
        .child(
            SelectableRichText::new(ElementId::Name(id.into()), inline.text.clone(), highlights)
                .links(inline.links()),
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
