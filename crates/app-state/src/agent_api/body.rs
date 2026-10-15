use teams_core::{Draft, MentionInput};

use crate::render::{Block, layout_blocks};

const BODY_PREVIEW_CHARACTERS: usize = 1200;

fn body_preview(text: &str) -> String {
    let text = text.trim();
    let cut: String = text.chars().take(BODY_PREVIEW_CHARACTERS).collect();
    if cut.len() == text.len() {
        return cut;
    }
    match cut.rfind('\n') {
        Some(newline) => cut[..newline].to_owned(),
        None => cut,
    }
}

pub fn body_blocks(text: &str, mentions: &[mcp::ProposalMention]) -> Vec<Block> {
    let inputs: Vec<MentionInput> = mentions
        .iter()
        .map(|mention| MentionInput::user(&mention.user_id, &mention.name))
        .collect();
    let preview = body_preview(text);
    let (html, _) = teams_core::apply_mentions(&Draft::from_markdown(&preview).to_html(), &inputs);
    layout_blocks(&teams_core::html_to_spans(&html))
}

#[cfg(test)]
mod tests {
    use super::{BODY_PREVIEW_CHARACTERS, body_blocks, body_preview};
    use crate::render::Block;

    #[test]
    fn the_card_body_renders_markdown_like_a_sent_message() {
        let blocks = body_blocks("- item\n- `code`", &[]);
        let [Block::List { items, ordered, .. }] = blocks.as_slice() else {
            panic!("{blocks:?}");
        };
        assert!(!ordered);
        assert_eq!(items.len(), 2);
        let Block::Paragraph(inline) = &items[1][0] else {
            panic!("{items:?}");
        };
        assert_eq!(inline.text, "code");
        assert!(inline.segments.iter().any(|segment| segment.style.code));
    }

    #[test]
    fn long_bodies_are_cut_at_the_last_line_break() {
        let text = format!("{}\n{}", "a".repeat(1000), "b".repeat(500));
        assert_eq!(body_preview(&text), "a".repeat(1000));
        assert_eq!(body_preview("short"), "short");
    }

    #[test]
    fn long_bodies_are_cut_before_layout() {
        let long = "word ".repeat(2000);
        let blocks = body_blocks(&long, &[]);
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert!(inline.text.chars().count() <= BODY_PREVIEW_CHARACTERS);
    }

    #[test]
    fn mentions_and_links_render_as_styled_segments() {
        let blocks = body_blocks(
            "**Hi** @Ada Example, see https://example.com/x",
            &[mcp::ProposalMention {
                user_id: "u1".to_owned(),
                name: "Ada Example".to_owned(),
            }],
        );
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert!(inline.segments.iter().any(|segment| segment.style.bold));
        assert!(inline.segments.iter().any(|segment| segment.style.mention));
        assert!(inline.segments.iter().any(|segment| segment.link.is_some()));
        assert!(!inline.text.contains("**"));
    }
}
