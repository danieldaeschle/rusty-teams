use std::collections::HashMap;

pub(crate) use chatsvc::graph_events::{attribute, unescape};

const MENTION_ITEMTYPE: &str = "schema.skype.com/Mention";
const INPUT_EXTENSION_ITEMTYPE: &str = "schema.skype.com/InputExtension";
const FORWARD_ITEMTYPE: &str = "schema.skype.com/Forward";
const EMOTICON_CLASS: &str = "animated-emoticon";
const REPLY_ITEMTYPE: &str = "schema.skype.com/Reply";

/// A chat service body in the shape Graph serves it: `<at>` mentions and `<attachment>` markers.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct GraphBody {
    pub html: String,
    pub mention_texts: HashMap<String, String>,
    pub forwards: Vec<String>,
    pub quotes: Vec<QuotedMessage>,
}

/// A Teams reply quote, which Graph serves as a `messageReference` attachment.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct QuotedMessage {
    pub message_id: String,
    pub sender_mri: Option<String>,
    pub sender_name: Option<String>,
    pub preview: String,
}

/// `forward_ids` name the `<attachment>` that replaces each forward quote, in order.
pub(crate) fn graph_body(content: &str, forward_ids: &[String]) -> GraphBody {
    let mut body = GraphBody::default();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        body.html.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(element) = Element::open_at(rest) else {
            body.html.push('<');
            rest = &rest[1..];
            continue;
        };
        let itemtype = attribute(element.tag, "itemtype").unwrap_or_default();
        let class = attribute(element.tag, "class").unwrap_or_default();
        let Some(rewrite) = Rewrite::of(element.name, itemtype, class) else {
            body.html.push_str(element.tag);
            rest = &rest[element.tag.len()..];
            continue;
        };
        let Some((inner, after)) = element.inner_and_rest(rest) else {
            body.html.push_str(element.tag);
            rest = &rest[element.tag.len()..];
            continue;
        };
        let item_id = attribute(element.tag, "itemid").unwrap_or_default();
        match rewrite {
            Rewrite::Mention => {
                let text = plain_text(inner);
                body.mention_texts.insert(item_id.to_owned(), text);
                body.html
                    .push_str(&format!("<at id=\"{item_id}\">{inner}</at>"));
            }
            Rewrite::Emoticon => match emoji_tag(inner) {
                Some(tag) => body.html.push_str(&tag),
                None => body.html.push_str(&rest[..rest.len() - after.len()]),
            },
            Rewrite::Reply if !item_id.is_empty() => {
                body.html
                    .push_str(&format!("<attachment id=\"{item_id}\"></attachment>"));
                body.quotes.push(quoted_message(item_id, inner));
            }
            Rewrite::Reply => body.html.push_str(&rest[..rest.len() - after.len()]),
            Rewrite::Card => body
                .html
                .push_str(&format!("<attachment id=\"{item_id}\"></attachment>")),
            Rewrite::Forward => {
                match forward_ids.get(body.forwards.len()) {
                    Some(id) => body
                        .html
                        .push_str(&format!("<attachment id=\"{id}\"></attachment>")),
                    None => body.html.push_str(&rest[..rest.len() - after.len()]),
                }
                body.forwards.push(inner.to_owned());
            }
        }
        rest = after;
    }
    body.html.push_str(rest);
    body
}

#[derive(Clone, Copy)]
enum Rewrite {
    Mention,
    Emoticon,
    Reply,
    Card,
    Forward,
}

impl Rewrite {
    fn of(name: &str, itemtype: &str, class: &str) -> Option<Rewrite> {
        match name {
            "span" if itemtype.contains(MENTION_ITEMTYPE) => Some(Rewrite::Mention),
            "span" if class.contains(EMOTICON_CLASS) => Some(Rewrite::Emoticon),
            "span" if itemtype.contains(INPUT_EXTENSION_ITEMTYPE) => Some(Rewrite::Card),
            "blockquote" if itemtype.contains(FORWARD_ITEMTYPE) => Some(Rewrite::Forward),
            "blockquote" if itemtype.contains(REPLY_ITEMTYPE) => Some(Rewrite::Reply),
            _ => None,
        }
    }
}

struct Element<'a> {
    name: &'a str,
    tag: &'a str,
}

impl<'a> Element<'a> {
    fn open_at(text: &'a str) -> Option<Element<'a>> {
        let end = text.find('>')? + 1;
        let tag = &text[..end];
        let name_end = tag[1..].find(|character: char| {
            character.is_whitespace() || character == '>' || character == '/'
        })? + 1;
        let name = &tag[1..name_end];
        (!name.is_empty()
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric()))
        .then_some(Element { name, tag })
    }

    /// The text between this element's tags and what follows its closing tag, nested elements of the same name included.
    fn inner_and_rest(&self, text: &'a str) -> Option<(&'a str, &'a str)> {
        let open = format!("<{}", self.name);
        let close = format!("</{}>", self.name);
        let inner_start = self.tag.len();
        let mut depth = 1;
        let mut position = inner_start;
        while depth > 0 {
            let next_close = text[position..].find(&close)? + position;
            match text[position..].find(&open).map(|found| found + position) {
                Some(next_open)
                    if next_open < next_close && opens_element(text, next_open, &open) =>
                {
                    depth += 1;
                    position = next_open + open.len();
                }
                _ => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((
                            &text[inner_start..next_close],
                            &text[next_close + close.len()..],
                        ));
                    }
                    position = next_close + close.len();
                }
            }
        }
        None
    }
}

fn quoted_message(message_id: &str, inner: &str) -> QuotedMessage {
    let property = |name: &str| -> Option<(&str, &str)> {
        let marker = format!("itemprop=\"{name}\"");
        let at = inner.find(&marker)?;
        let tag_start = inner[..at].rfind('<')?;
        let element = Element::open_at(&inner[tag_start..])?;
        let (content, _) = element.inner_and_rest(&inner[tag_start..])?;
        Some((element.tag, content))
    };
    let sender = property("mri");
    QuotedMessage {
        message_id: message_id.to_owned(),
        sender_mri: sender
            .and_then(|(tag, _)| attribute(tag, "itemid"))
            .filter(|mri| !mri.is_empty())
            .map(str::to_owned),
        sender_name: sender
            .map(|(_, content)| plain_text(content))
            .filter(|name| !name.is_empty()),
        preview: property("preview")
            .map(|(_, content)| plain_text(content))
            .unwrap_or_default(),
    }
}

/// Graph's `<emoji>` for a Teams emoticon span, read from the emoji image inside it.
fn emoji_tag(inner: &str) -> Option<String> {
    let start = inner.find("<img")?;
    let end = inner[start..].find('>')? + start;
    let image = &inner[start..end];
    let id = attribute(image, "itemid")?;
    let alt = attribute(image, "alt").unwrap_or_default();
    let title = attribute(image, "title").unwrap_or_default();
    Some(format!(
        "<emoji id=\"{id}\" alt=\"{alt}\" title=\"{title}\"></emoji>"
    ))
}

fn opens_element(text: &str, at: usize, open: &str) -> bool {
    text[at + open.len()..]
        .chars()
        .next()
        .is_some_and(|next| next.is_whitespace() || next == '>')
}

pub(crate) fn plain_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    unescape(text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_spans_become_at_tags() {
        let body = graph_body(
            r#"<p>hi <span itemtype="http://schema.skype.com/Mention" itemscope="" itemid="0">Ada &amp; Co</span>!</p>"#,
            &[],
        );
        assert_eq!(body.html, r#"<p>hi <at id="0">Ada &amp; Co</at>!</p>"#);
        assert_eq!(body.mention_texts["0"], "Ada & Co");
    }

    #[test]
    fn emoticon_spans_stay_and_nested_spans_close_right() {
        let html = r#"<p><span itemtype="http://schema.skype.com/Mention" itemid="1"><span>Bo</span></span> <span title="Smile" type="(smile)" class="animated-emoticon-20-smile" itemscope=""><img itemtype="http://schema.skype.com/Emoji" alt="🙂"></span></p>"#;
        let body = graph_body(html, &[]);
        assert!(
            body.html
                .starts_with(r#"<p><at id="1"><span>Bo</span></at> <span title="Smile""#)
        );
        assert_eq!(body.mention_texts["1"], "Bo");
    }

    #[test]
    fn emoticon_spans_become_graph_emoji() {
        let html = r#"<p>ok<span title="Smile" type="(smile)" class="animated-emoticon-20-smile" itemscope=""><img itemscope="" itemtype="http://schema.skype.com/Emoji" itemid="smile" src="https://x/e.png" title="Smile" alt="🙂"></span></p>"#;
        assert_eq!(
            graph_body(html, &[]).html,
            r#"<p>ok<emoji id="smile" alt="🙂" title="Smile"></emoji></p>"#
        );
    }

    #[test]
    fn reply_quotes_become_message_references() {
        let html = r#"<blockquote itemscope="" itemtype="http://schema.skype.com/Reply" itemid="1751975663649"><strong itemprop="mri" itemid="8:orgid:ann">Ann</strong><span itemprop="time" itemid="1751975663649"></span><p itemprop="preview">old &amp; true</p></blockquote><p>new</p>"#;
        let body = graph_body(html, &[]);
        assert_eq!(
            body.html,
            r#"<attachment id="1751975663649"></attachment><p>new</p>"#
        );
        assert_eq!(
            body.quotes,
            vec![QuotedMessage {
                message_id: "1751975663649".into(),
                sender_mri: Some("8:orgid:ann".into()),
                sender_name: Some("Ann".into()),
                preview: "old & true".into(),
            }]
        );
    }

    #[test]
    fn card_placeholders_and_forwards_become_attachment_markers() {
        let html = r#"<p>see</p><span itemid="card-1" itemscope="" itemtype="http://schema.skype.com/InputExtension"><span itemprop="cardId"></span></span><blockquote itemtype="http://schema.skype.com/Forward"><p>old</p></blockquote>"#;
        let body = graph_body(html, &["m7".to_owned()]);
        assert_eq!(
            body.html,
            r#"<p>see</p><attachment id="card-1"></attachment><attachment id="m7"></attachment>"#
        );
        assert_eq!(body.forwards, vec!["<p>old</p>".to_owned()]);
    }

    #[test]
    fn a_forward_without_context_keeps_its_quote() {
        let html =
            r#"<blockquote itemtype="http://schema.skype.com/Forward"><p>old</p></blockquote>"#;
        assert_eq!(graph_body(html, &[]).html, html);
    }

    #[test]
    fn broken_markup_is_kept() {
        assert_eq!(
            graph_body("a < b <span itemtype=\"x\">", &[]).html,
            "a < b <span itemtype=\"x\">"
        );
    }
}
