use scraper::{ElementRef, Html, Node};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSize {
    Small,
    Large,
    Hidden,
    Pixels(u16),
}

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
        id: Option<String>,
    },
    Strike(Vec<Span>),
    Superscript(Vec<Span>),
    Subscript(Vec<Span>),
    Sized(FontSize, Vec<Span>),
    Underline(Vec<Span>),
    Colored {
        color: Option<u32>,
        background: Option<u32>,
        children: Vec<Span>,
    },
    Heading {
        level: u8,
        children: Vec<Span>,
    },
    LineBreak,
    Rule,
    List {
        ordered: bool,
        start: u32,
        items: Vec<Vec<Span>>,
    },
    Table {
        header: bool,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Image {
        hosted_content_url: String,
    },
    Quote(Vec<Span>),
    BlockQuote(Vec<Span>),
}

const BLOCK_ELEMENTS: [&str; 12] = [
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "ul", "ol", "hr",
];
const SKIPPED_ELEMENTS: [&str; 4] = ["script", "style", "attachment", "head"];
const LANGUAGE_PREFIX: &str = "language-";
const EMOJI_ITEMTYPE: &str = "schema.skype.com/Emoji";
const REPLY_ITEMTYPE: &str = "schema.skype.com/Reply";
const MAX_HEADING_LEVEL: u8 = 3;
const MARK_BACKGROUND: u32 = 0xffff00;
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
        "b" | "strong" => spans.push(Span::Bold(convert_children(element))),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => spans.push(Span::Heading {
            level: name[1..].parse::<u8>().unwrap_or(1).min(MAX_HEADING_LEVEL),
            children: convert_children(element),
        }),
        "i" | "em" => spans.push(Span::Italic(convert_children(element))),
        "s" | "strike" | "del" => spans.push(Span::Strike(convert_children(element))),
        "sup" => spans.push(Span::Superscript(convert_children(element))),
        "sub" => spans.push(Span::Subscript(convert_children(element))),
        "u" | "ins" => spans.push(Span::Underline(convert_children(element))),
        "span" | "font" | "mark" => push_colored(element, spans),
        "ul" | "ol" => spans.push(list(element)),
        "table" => spans.push(table(element)),
        "code" => spans.push(Span::Code(raw_text(element))),
        "pre" | "codeblock" => spans.push(code_block(element)),
        "a" => push_link(element, spans),
        "at" => spans.push(Span::Mention {
            name: raw_text(element).trim().to_owned(),
            id: element.value().attr("id").map(str::to_owned),
        }),
        "br" => spans.push(Span::LineBreak),
        "hr" => spans.push(Span::Rule),
        "li" => spans.push(Span::List {
            ordered: false,
            start: 1,
            items: vec![convert_children(element)],
        }),
        "blockquote" if is_reply(element) => spans.push(Span::Quote(convert_children(element))),
        "blockquote" => spans.push(Span::BlockQuote(convert_children(element))),
        "emoji" => push_emoji(element, spans),
        "img" => push_image(element, spans),
        _ => spans.extend(convert_children(element)),
    }
}

fn is_reply(element: ElementRef<'_>) -> bool {
    element
        .value()
        .attr("itemtype")
        .is_some_and(|itemtype| itemtype.contains(REPLY_ITEMTYPE))
}

fn list(element: ElementRef<'_>) -> Span {
    let mut items: Vec<Vec<Span>> = Vec::new();
    for child in element.children().filter_map(ElementRef::wrap) {
        match child.value().name() {
            "li" => items.push(convert_children(child)),
            "ul" | "ol" => match items.last_mut() {
                Some(previous) => previous.push(list(child)),
                None => items.push(vec![list(child)]),
            },
            _ => {}
        }
    }
    Span::List {
        ordered: element.value().name() == "ol",
        start: element
            .value()
            .attr("start")
            .and_then(|start| start.trim().parse().ok())
            .unwrap_or(1),
        items,
    }
}

fn table(element: ElementRef<'_>) -> Span {
    let rows: Vec<ElementRef<'_>> = element
        .descendants()
        .filter_map(ElementRef::wrap)
        .filter(|row| row.value().name() == "tr" && belongs_to(*row, element))
        .collect();
    let header = rows.first().is_some_and(|row| {
        let in_head = row
            .parent()
            .and_then(ElementRef::wrap)
            .is_some_and(|parent| parent.value().name() == "thead");
        let mut cells = cells_of(*row).peekable();
        in_head || (cells.peek().is_some() && cells.all(|cell| cell.value().name() == "th"))
    });
    Span::Table {
        header,
        rows: rows
            .into_iter()
            .map(|row| cells_of(row).map(convert_children).collect())
            .filter(|cells: &Vec<Vec<Span>>| !cells.is_empty())
            .collect(),
    }
}

fn belongs_to(row: ElementRef<'_>, table: ElementRef<'_>) -> bool {
    row.ancestors()
        .filter_map(ElementRef::wrap)
        .find(|ancestor| ancestor.value().name() == "table")
        .is_some_and(|owner| owner.id() == table.id())
}

fn cells_of<'a>(row: ElementRef<'a>) -> impl Iterator<Item = ElementRef<'a>> {
    row.children()
        .filter_map(ElementRef::wrap)
        .filter(|cell| matches!(cell.value().name(), "td" | "th"))
}

fn push_colored(element: ElementRef<'_>, spans: &mut Vec<Span>) {
    let value = element.value();
    let style = value.attr("style").unwrap_or_default();
    let color = style_value(style, "color")
        .or_else(|| value.attr("color"))
        .and_then(parse_color);
    let background = style_value(style, "background-color")
        .or_else(|| style_value(style, "background"))
        .and_then(parse_color)
        .or((value.name() == "mark").then_some(MARK_BACKGROUND));
    let size = style_value(style, "font-size").and_then(parse_font_size);
    let mut children = convert_children(element);
    if color.is_some() || background.is_some() {
        children = vec![Span::Colored {
            color,
            background,
            children,
        }];
    }
    match size {
        Some(size) => spans.push(Span::Sized(size, children)),
        None => spans.extend(children),
    }
}

fn parse_font_size(value: &str) -> Option<FontSize> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "xx-small" => Some(FontSize::Small),
        "x-large" => Some(FontSize::Large),
        "x-small" => Some(FontSize::Pixels(10)),
        "small" | "smaller" => Some(FontSize::Pixels(13)),
        "large" | "larger" => Some(FontSize::Pixels(18)),
        "xx-large" => Some(FontSize::Pixels(32)),
        "0" => Some(FontSize::Hidden),
        _ => {
            let pixels: f32 = value.strip_suffix("px")?.trim().parse().ok()?;
            if pixels <= 0. {
                Some(FontSize::Hidden)
            } else {
                Some(FontSize::Pixels(
                    pixels.round().min(f32::from(u16::MAX)) as u16
                ))
            }
        }
    }
}

fn style_value<'a>(style: &'a str, property: &str) -> Option<&'a str> {
    style.split(';').find_map(|declaration| {
        let (name, value) = declaration.split_once(':')?;
        (name.trim().eq_ignore_ascii_case(property)).then(|| value.trim())
    })
}

pub fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let expanded: String = hex.chars().flat_map(|digit| [digit, digit]).collect();
                u32::from_str_radix(&expanded, 16).ok()
            }
            6 => u32::from_str_radix(hex, 16).ok(),
            _ => None,
        };
    }
    if let Some(arguments) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let channels: Vec<u32> = arguments
            .split(',')
            .take(3)
            .map(|channel| {
                channel
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .map(|level| level.clamp(0., 255.) as u32)
            })
            .collect::<Option<_>>()?;
        let [red, green, blue] = channels[..] else {
            return None;
        };
        return Some(red << 16 | green << 8 | blue);
    }
    NAMED_COLORS
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, hex)| *hex)
}

const NAMED_COLORS: [(&str, u32); 14] = [
    ("black", 0x000000),
    ("white", 0xffffff),
    ("red", 0xff0000),
    ("green", 0x008000),
    ("lime", 0x00ff00),
    ("blue", 0x0000ff),
    ("yellow", 0xffff00),
    ("orange", 0xffa500),
    ("purple", 0x800080),
    ("fuchsia", 0xff00ff),
    ("aqua", 0x00ffff),
    ("teal", 0x008080),
    ("gray", 0x808080),
    ("grey", 0x808080),
];

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
            | Span::Strike(children)
            | Span::Superscript(children)
            | Span::Subscript(children)
            | Span::Sized(_, children)
            | Span::Underline(children)
            | Span::Colored { children, .. }
            | Span::Heading { children, .. }
            | Span::Quote(children)
            | Span::BlockQuote(children)
            | Span::Link { children, .. } => tidy(children),
            Span::List { items, .. } => items.iter_mut().for_each(tidy),
            Span::Table { rows, .. } => rows.iter_mut().flatten().for_each(tidy),
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
                None | Some(
                    Span::LineBreak
                        | Span::Rule
                        | Span::List { .. }
                        | Span::Table { .. }
                        | Span::Heading { .. }
                        | Span::Quote(_)
                        | Span::BlockQuote(_)
                )
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
