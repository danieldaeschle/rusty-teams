use std::sync::LazyLock;

use regex::Regex;

static CODE_FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```[^\n`]*\n(.*?)```").unwrap());
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`\n]+)`").unwrap());
static BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*\n]+)\*\*").unwrap());
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]+)\]\((https?://[^)\s]+)\)").unwrap());

const LIST_MARKER: &str = "- ";

pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            other => escaped.push(other),
        }
    }
    escaped
}

pub fn plain_text_to_html(text: &str) -> String {
    escape_html(text).replace('\n', "<br>")
}

/// Teams chat HTML for light markdown, plus `- ` list lines as `<ul><li>`.
pub fn markdown_to_html(text: &str) -> String {
    let mut html = String::new();
    let mut position = 0;
    for fence in CODE_FENCE.captures_iter(text) {
        let whole = fence.get(0).expect("group 0 always exists");
        html.push_str(&block_html(&text[position..whole.start()]));
        html.push_str("<pre>");
        html.push_str(&escape_html(fence[1].trim_end_matches('\n')));
        html.push_str("</pre>");
        position = whole.end();
    }
    html.push_str(&block_html(&text[position..]));
    html
}

fn block_html(segment: &str) -> String {
    if !segment.lines().any(is_list_line) {
        return inline_html(segment);
    }
    let mut html = String::new();
    let mut text_run: Vec<&str> = Vec::new();
    let mut list_run: Vec<&str> = Vec::new();
    for line in segment.split('\n') {
        if is_list_line(line) {
            flush_text(&mut html, &mut text_run);
            list_run.push(&line[LIST_MARKER.len()..]);
        } else {
            flush_list(&mut html, &mut list_run);
            text_run.push(line);
        }
    }
    flush_list(&mut html, &mut list_run);
    flush_text(&mut html, &mut text_run);
    html
}

fn is_list_line(line: &str) -> bool {
    line.starts_with(LIST_MARKER)
}

fn flush_text(html: &mut String, run: &mut Vec<&str>) {
    if !run.is_empty() {
        html.push_str(&inline_html(&run.join("\n")));
        run.clear();
    }
}

fn flush_list(html: &mut String, run: &mut Vec<&str>) {
    if run.is_empty() {
        return;
    }
    html.push_str("<ul>");
    for item in run.drain(..) {
        html.push_str("<li>");
        html.push_str(&inline_html(item));
        html.push_str("</li>");
    }
    html.push_str("</ul>");
}

fn inline_html(text: &str) -> String {
    let escaped = plain_text_to_html(text);
    let coded = INLINE_CODE.replace_all(&escaped, "<code>${1}</code>");
    let bolded = BOLD.replace_all(&coded, "<b>${1}</b>");
    LINK.replace_all(&bolded, r#"<a href="${2}">${1}</a>"#)
        .into_owned()
}
