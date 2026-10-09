const MARKERS: [char; 2] = ['*', '_'];
const CODE_OPEN: &str = "<code>";
const CODE_CLOSE: &str = "</code>";
const LINE_BREAK: &str = "<br>";
const LINK_TARGET_OPEN: &str = "](";
const URL_SCHEMES: [&str; 2] = ["https://", "http://"];

pub(crate) fn italicize(escaped: &str) -> String {
    let characters: Vec<char> = escaped.chars().collect();
    let mut output = String::with_capacity(escaped.len());
    let mut index = 0;
    while index < characters.len() {
        if let Some(end) = protected_end(&characters, index) {
            output.extend(&characters[index..end]);
            index = end;
            continue;
        }
        let character = characters[index];
        if let Some(close) = italic_close(&characters, index) {
            output.push_str("<i>");
            output.extend(&characters[index + 1..close]);
            output.push_str("</i>");
            index = close + 1;
            continue;
        }
        output.push(character);
        index += 1;
    }
    output
}

fn starts_with(characters: &[char], index: usize, text: &str) -> bool {
    text.chars()
        .enumerate()
        .all(|(offset, expected)| characters.get(index + offset) == Some(&expected))
}

fn protected_end(characters: &[char], index: usize) -> Option<usize> {
    if starts_with(characters, index, CODE_OPEN) {
        let close =
            (index..characters.len()).find(|&at| starts_with(characters, at, CODE_CLOSE))?;
        return Some(close + CODE_CLOSE.chars().count());
    }
    if characters[index] == ']' && starts_with(characters, index, LINK_TARGET_OPEN) {
        let close = (index..characters.len()).find(|&at| characters[at] == ')')?;
        return Some(close + 1);
    }
    let boundary = index == 0 || !characters[index - 1].is_alphanumeric();
    if boundary
        && URL_SCHEMES
            .iter()
            .any(|scheme| starts_with(characters, index, scheme))
    {
        let end = (index..characters.len())
            .find(|&at| characters[at].is_whitespace() || characters[at] == '<')
            .unwrap_or(characters.len());
        return Some(end);
    }
    None
}

fn italic_close(characters: &[char], open: usize) -> Option<usize> {
    let marker = characters[open];
    if !MARKERS.contains(&marker) {
        return None;
    }
    if open > 0 && characters[open - 1].is_alphanumeric() {
        return None;
    }
    let first = *characters.get(open + 1)?;
    if first.is_whitespace() || MARKERS.contains(&first) {
        return None;
    }
    for close in open + 2..characters.len() {
        let character = characters[close];
        if character == '\n' || starts_with(characters, close, LINE_BREAK) {
            return None;
        }
        if character != marker || characters[close - 1].is_whitespace() {
            continue;
        }
        let closes = characters
            .get(close + 1)
            .is_none_or(|next| !next.is_alphanumeric() && *next != marker);
        if closes {
            return Some(close);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::italicize;

    #[test]
    fn underscore_and_star_pairs_become_italic() {
        assert_eq!(italicize("a _b_ c"), "a <i>b</i> c");
        assert_eq!(italicize("a *b c* d"), "a <i>b c</i> d");
        assert_eq!(italicize("_a_ _b_"), "<i>a</i> <i>b</i>");
    }

    #[test]
    fn snake_case_and_inner_underscores_stay() {
        assert_eq!(italicize("snake_case_word"), "snake_case_word");
        assert_eq!(italicize("_x_y"), "_x_y");
        assert_eq!(italicize("2*3*4"), "2*3*4");
    }

    #[test]
    fn urls_code_and_link_targets_stay() {
        assert_eq!(
            italicize("see https://a.example/_x_/_y_ now"),
            "see https://a.example/_x_/_y_ now"
        );
        assert_eq!(
            italicize("<code>_x_</code> _y_"),
            "<code>_x_</code> <i>y</i>"
        );
        assert_eq!(
            italicize("<a>[t](https://a.example/_x_)</a>"),
            "<a>[t](https://a.example/_x_)</a>"
        );
    }

    #[test]
    fn unpaired_and_spaced_markers_stay() {
        assert_eq!(italicize("_a"), "_a");
        assert_eq!(italicize("* item"), "* item");
        assert_eq!(italicize("a _ b _"), "a _ b _");
        assert_eq!(italicize("**"), "**");
        assert_eq!(italicize("_a<br>b_"), "_a<br>b_");
    }
}
