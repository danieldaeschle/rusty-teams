use scraper::{ElementRef, Html, Node};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text(String),
    Bold(Vec<Span>),
    Italic(Vec<Span>),
    Code(String),
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    Link {
        url: String,
        children: Vec<Span>,
    },
    Mention {
        name: String,
    },
    LineBreak,
    ListItem(Vec<Span>),
    Image {
        hosted_content_url: String,
    },
    Quote(Vec<Span>),
}

const BLOCK_ELEMENTS: [&str; 12] = [
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "ul", "ol", "hr",
];
const SKIPPED_ELEMENTS: [&str; 4] = ["script", "style", "attachment", "head"];
const LANGUAGE_PREFIX: &str = "language-";
const EMOJI_ITEMTYPE: &str = "schema.skype.com/Emoji";
const MAX_CONSECUTIVE_BREAKS: usize = 2;

pub fn html_to_spans(html: &str) -> Vec<Span> {
    let fragment = Html::parse_fragment(html);
    let mut spans = convert_children(fragment.root_element());
    tidy(&mut spans);
    spans
}

fn convert_children(element: ElementRef<'_>) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut break_pending = false;
    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                let text: &str = text;
                if break_pending && text.trim().is_empty() {
                    continue;
                }
                if break_pending && !spans.is_empty() {
                    spans.push(Span::LineBreak);
                }
                break_pending = false;
                push_text(&mut spans, text);
            }
            Node::Element(_) => {
                let Some(child_element) = ElementRef::wrap(child) else {
                    continue;
                };
                let is_block = BLOCK_ELEMENTS.contains(&child_element.value().name());
                if (is_block || break_pending) && !spans.is_empty() {
                    spans.push(Span::LineBreak);
                }
                convert_element(child_element, &mut spans);
                break_pending = is_block;
            }
            _ => {}
        }
    }
    spans
}

fn convert_element(element: ElementRef<'_>, spans: &mut Vec<Span>) {
    let name = element.value().name();
    if SKIPPED_ELEMENTS.contains(&name) {
        return;
    }
    match name {
        "b" | "strong" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            spans.push(Span::Bold(convert_children(element)))
        }
        "i" | "em" => spans.push(Span::Italic(convert_children(element))),
        "code" => spans.push(Span::Code(raw_text(element))),
        "pre" | "codeblock" => spans.push(code_block(element)),
        "a" => push_link(element, spans),
        "at" => spans.push(Span::Mention {
            name: raw_text(element).trim().to_owned(),
        }),
        "br" | "hr" => spans.push(Span::LineBreak),
        "li" => spans.push(Span::ListItem(convert_children(element))),
        "blockquote" => spans.push(Span::Quote(convert_children(element))),
        "emoji" => push_emoji(element, spans),
        "img" => push_image(element, spans),
        _ => spans.extend(convert_children(element)),
    }
}

fn code_block(element: ElementRef<'_>) -> Span {
    let language = language_of(element).or_else(|| {
        element
            .descendants()
            .filter_map(ElementRef::wrap)
            .find_map(language_of)
    });
    Span::CodeBlock {
        language,
        code: raw_text(element).trim_end_matches('\n').to_owned(),
    }
}

fn language_of(element: ElementRef<'_>) -> Option<String> {
    element
        .value()
        .classes()
        .find_map(|class| class.strip_prefix(LANGUAGE_PREFIX))
        .filter(|language| !language.is_empty())
        .map(str::to_owned)
}

fn push_link(element: ElementRef<'_>, spans: &mut Vec<Span>) {
    let children = convert_children(element);
    match element.value().attr("href") {
        Some(url) if !url.is_empty() => spans.push(Span::Link {
            url: url.to_owned(),
            children,
        }),
        _ => spans.extend(children),
    }
}

fn push_emoji(element: ElementRef<'_>, spans: &mut Vec<Span>) {
    let glyph = element
        .value()
        .attr("alt")
        .map(str::to_owned)
        .filter(|alt| !alt.is_empty())
        .unwrap_or_else(|| raw_text(element));
    push_text(spans, &glyph);
}

fn push_image(element: ElementRef<'_>, spans: &mut Vec<Span>) {
    let value = element.value();
    let is_emoji = value
        .attr("itemtype")
        .is_some_and(|itemtype| itemtype.contains(EMOJI_ITEMTYPE));
    if is_emoji {
        if let Some(alt) = value.attr("alt") {
            push_text(spans, alt);
        }
        return;
    }
    if let Some(source) = value.attr("src").filter(|source| !source.is_empty()) {
        spans.push(Span::Image {
            hosted_content_url: source.to_owned(),
        });
    }
}

fn raw_text(element: ElementRef<'_>) -> String {
    element.text().collect()
}

fn push_text(spans: &mut Vec<Span>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Span::Text(existing)) = spans.last_mut() {
        existing.push_str(text);
    } else {
        spans.push(Span::Text(text.to_owned()));
    }
}

fn tidy(spans: &mut Vec<Span>) {
    for span in spans.iter_mut() {
        match span {
            Span::Bold(children)
            | Span::Italic(children)
            | Span::ListItem(children)
            | Span::Quote(children)
            | Span::Link { children, .. } => tidy(children),
            _ => {}
        }
    }
    collapse_whitespace(spans);
    limit_breaks(spans);
    trim_breaks(spans);
}

fn collapse_whitespace(spans: &mut Vec<Span>) {
    let mut collapsed: Vec<Span> = Vec::with_capacity(spans.len());
    let count = spans.len();
    for (index, span) in std::mem::take(spans).into_iter().enumerate() {
        let Span::Text(text) = span else {
            collapsed.push(span);
            continue;
        };
        let mut text = squeeze_spaces(&text);
        let at_edge = |neighbour: Option<&Span>| {
            matches!(
                neighbour,
                None | Some(Span::LineBreak | Span::ListItem(_) | Span::Quote(_))
            )
        };
        if at_edge(collapsed.last()) {
            text = text.trim_start().to_owned();
        }
        if index + 1 == count {
            text = text.trim_end().to_owned();
        }
        if !text.is_empty() {
            collapsed.push(Span::Text(text));
        }
    }
    for index in 0..collapsed.len() {
        let followed_by_break = matches!(collapsed.get(index + 1), Some(Span::LineBreak));
        if followed_by_break && let Span::Text(text) = &mut collapsed[index] {
            *text = text.trim_end().to_owned();
        }
    }
    collapsed.retain(|span| !matches!(span, Span::Text(text) if text.is_empty()));
    *spans = collapsed;
}

fn squeeze_spaces(text: &str) -> String {
    let mut squeezed = String::with_capacity(text.len());
    let mut previous_space = false;
    for character in text.chars() {
        let is_space = character.is_whitespace() && character != '\u{a0}';
        if is_space {
            if !previous_space {
                squeezed.push(' ');
            }
        } else {
            squeezed.push(character);
        }
        previous_space = is_space;
    }
    squeezed
}

fn limit_breaks(spans: &mut Vec<Span>) {
    let mut run = 0;
    spans.retain(|span| {
        if matches!(span, Span::LineBreak) {
            run += 1;
            run <= MAX_CONSECUTIVE_BREAKS
        } else {
            run = 0;
            true
        }
    });
}

fn trim_breaks(spans: &mut Vec<Span>) {
    while matches!(spans.last(), Some(Span::LineBreak)) {
        spans.pop();
    }
    let leading = spans
        .iter()
        .take_while(|span| matches!(span, Span::LineBreak))
        .count();
    spans.drain(..leading);
}
