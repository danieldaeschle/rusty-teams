use std::ops::Range;

const HIT_START: &str = "<c0>";
const HIT_END: &str = "</c0>";
const ELLIPSIS: &str = "<ddd/>";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HighlightedText {
    pub text: String,
    pub hits: Vec<Range<usize>>,
}

pub fn split_highlights(summary: &str) -> HighlightedText {
    let mut highlighted = HighlightedText::default();
    let mut rest = summary;
    let mut hit_start: Option<usize> = None;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(HIT_START) {
            hit_start.get_or_insert(highlighted.text.len());
            rest = after;
        } else if let Some(after) = rest.strip_prefix(HIT_END) {
            if let Some(start) = hit_start
                .take()
                .filter(|start| *start < highlighted.text.len())
            {
                highlighted.hits.push(start..highlighted.text.len());
            }
            rest = after;
        } else if let Some(after) = rest.strip_prefix(ELLIPSIS) {
            highlighted.text.push_str("...");
            rest = after;
        } else {
            let next = rest.chars().next().unwrap_or_default();
            highlighted.text.push(next);
            rest = &rest[next.len_utf8()..];
        }
    }
    highlighted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_markers_become_byte_ranges_over_the_plain_text() {
        let highlighted = split_highlights("Grüße an <c0>Christian</c0> und <c0>Anna</c0>");
        assert_eq!(highlighted.text, "Grüße an Christian und Anna");
        let words: Vec<&str> = highlighted
            .hits
            .iter()
            .map(|hit| &highlighted.text[hit.clone()])
            .collect();
        assert_eq!(words, ["Christian", "Anna"]);
    }

    #[test]
    fn ellipsis_marker_and_unclosed_hit_are_tolerated() {
        let highlighted = split_highlights("<ddd/>end of <c0>text");
        assert_eq!(highlighted.text, "...end of text");
        assert!(highlighted.hits.is_empty());
    }
}
