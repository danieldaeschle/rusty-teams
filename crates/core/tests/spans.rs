use serde_json::json;
use store::MessageRecord;
use teams_core::{Span, card_content_text, html_to_spans, message_spans};

fn text(value: &str) -> Span {
    Span::Text(value.to_owned())
}

#[test]
fn plain_paragraph() {
    assert_eq!(
        html_to_spans("<p>Hello world</p>"),
        vec![text("Hello world")]
    );
}

#[test]
fn paragraphs_are_separated_by_one_break() {
    assert_eq!(
        html_to_spans("<p>one</p>\n<p>two</p><p>three</p>"),
        vec![
            text("one"),
            Span::LineBreak,
            text("two"),
            Span::LineBreak,
            text("three")
        ]
    );
}

#[test]
fn inline_formatting_nests() {
    assert_eq!(
        html_to_spans("<p>a <b>bold <i>both</i></b> <em>it</em> <strong>s</strong></p>"),
        vec![
            text("a "),
            Span::Bold(vec![text("bold "), Span::Italic(vec![text("both")])]),
            text(" "),
            Span::Italic(vec![text("it")]),
            text(" "),
            Span::Bold(vec![text("s")]),
        ]
    );
}

#[test]
fn mention_uses_the_visible_name() {
    assert_eq!(
        html_to_spans(r#"<p><at id="0">Ada Example</at> please look</p>"#),
        vec![
            Span::Mention {
                name: "Ada Example".to_owned()
            },
            text(" please look")
        ]
    );
}

#[test]
fn link_keeps_url_and_label() {
    assert_eq!(
        html_to_spans(r#"<a href="https://example.test/x?a=1&amp;b=2">docs</a>"#),
        vec![Span::Link {
            url: "https://example.test/x?a=1&b=2".to_owned(),
            children: vec![text("docs")],
        }]
    );
}

#[test]
fn anchor_without_href_is_plain_text() {
    assert_eq!(html_to_spans("<a>nothing</a>"), vec![text("nothing")]);
}

#[test]
fn inline_code_and_entities() {
    assert_eq!(
        html_to_spans("<p>run <code>a &lt; b</code> &amp; done</p>"),
        vec![
            text("run "),
            Span::Code("a < b".to_owned()),
            text(" & done")
        ]
    );
}

#[test]
fn codeblock_with_language_class() {
    assert_eq!(
        html_to_spans("<p>x</p><pre class=\"language-rust\">fn main() {\n    1 &lt; 2\n}\n</pre>"),
        vec![
            text("x"),
            Span::LineBreak,
            Span::CodeBlock {
                language: Some("rust".to_owned()),
                code: "fn main() {\n    1 < 2\n}".to_owned(),
            },
        ]
    );
}

#[test]
fn teams_codeblock_wrapper() {
    assert_eq!(
        html_to_spans(r#"<codeblock class="language-js"><pre>let a = 1;</pre></codeblock>"#),
        vec![Span::CodeBlock {
            language: Some("js".to_owned()),
            code: "let a = 1;".to_owned(),
        }]
    );
}

#[test]
fn pre_without_language() {
    assert_eq!(
        html_to_spans("<pre>a\n  b</pre>"),
        vec![Span::CodeBlock {
            language: None,
            code: "a\n  b".to_owned()
        }]
    );
}

#[test]
fn language_is_found_on_inner_code_element() {
    assert_eq!(
        html_to_spans(r#"<pre><code class="language-python">print(1)</code></pre>"#),
        vec![Span::CodeBlock {
            language: Some("python".to_owned()),
            code: "print(1)".to_owned(),
        }]
    );
}

#[test]
fn hosted_content_image() {
    let url = "https://graph.microsoft.com/v1.0/chats/19:x@thread.v2/messages/1/hostedContents/aGVsbG8/$value";
    assert_eq!(
        html_to_spans(&format!(
            r#"<p>look</p><p><img src="{url}" width="300" itemtype="http://schema.skype.com/AMSImage"></p>"#
        )),
        vec![
            text("look"),
            Span::LineBreak,
            Span::Image {
                hosted_content_url: url.to_owned()
            }
        ]
    );
}

#[test]
fn emoji_element_and_emoji_image_become_text() {
    assert_eq!(
        html_to_spans(
            r#"<p>hi <emoji id="smile" alt="😄" title="Smile"></emoji> and <img itemtype="http://schema.skype.com/Emoji" alt="👍" src="https://statics.example.test/e.png"></p>"#
        ),
        vec![text("hi 😄 and 👍")]
    );
}

#[test]
fn reply_quote() {
    let html = r#"<blockquote itemscope="" itemtype="http://schema.skype.com/Reply" itemid="170"><strong itemprop="mri">Bob Sample</strong><span itemprop="time">12:01</span><p itemprop="preview">original text</p></blockquote><p>my answer</p>"#;
    assert_eq!(
        html_to_spans(html),
        vec![
            Span::Quote(vec![
                Span::Bold(vec![text("Bob Sample")]),
                text("12:01"),
                Span::LineBreak,
                text("original text"),
            ]),
            Span::LineBreak,
            text("my answer"),
        ]
    );
}

#[test]
fn list_items() {
    assert_eq!(
        html_to_spans("<ul>\n<li>one</li>\n<li>two <b>b</b></li>\n</ul>"),
        vec![
            Span::ListItem(vec![text("one")]),
            Span::ListItem(vec![text("two "), Span::Bold(vec![text("b")])]),
        ]
    );
}

#[test]
fn line_breaks() {
    assert_eq!(
        html_to_spans("<p>a<br>b</p>"),
        vec![text("a"), Span::LineBreak, text("b")]
    );
}

#[test]
fn attachment_markers_and_scripts_are_dropped() {
    assert_eq!(
        html_to_spans(r#"<p>file</p><attachment id="abc"></attachment><script>alert(1)</script>"#),
        vec![text("file")]
    );
}

#[test]
fn empty_and_system_bodies_yield_nothing() {
    assert!(html_to_spans("").is_empty());
    assert!(html_to_spans("<systemEventMessage/>").is_empty());
}

#[test]
fn whitespace_collapses() {
    assert_eq!(
        html_to_spans("<div>\n  a \n\t b  </div>"),
        vec![text("a b")]
    );
}

#[test]
fn card_text_follows_python_order() {
    let card = json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "text": "Deploy finished"},
            {"type": "FactSet", "facts": [{"title": "Env", "value": "prod"}]},
            {"type": "TextBlock", "text": "   "}
        ],
        "title": "Header"
    });
    assert_eq!(
        card_content_text(&card.to_string()).unwrap(),
        "Header\nDeploy finished\nEnv\nprod"
    );
}

fn record_with(body_html: &str, attachments_json: &str) -> MessageRecord {
    MessageRecord {
        conversation_id: "c".to_owned(),
        message_id: "m".to_owned(),
        reply_to_id: None,
        sender_id: None,
        sender_name: None,
        created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        edited_at: None,
        deleted: false,
        body_html: body_html.to_owned(),
        attachments_json: attachments_json.to_owned(),
        reactions_json: "[]".to_owned(),
        mentions_json: "[]".to_owned(),
    }
}

#[test]
fn message_spans_append_card_text_only_for_cards() {
    let attachments = r#"[
        {"content_type":"application/vnd.microsoft.card.adaptive","name":null,"url":null,"text":"Line one\nLine two"},
        {"content_type":"reference","name":"a.pdf","url":"https://files.example.test/a.pdf","text":"[attachment: a.pdf]"}
    ]"#;
    assert_eq!(
        message_spans(&record_with("<p>body</p>", attachments)),
        vec![
            text("body"),
            Span::LineBreak,
            text("Line one"),
            Span::LineBreak,
            text("Line two")
        ]
    );
    assert_eq!(
        message_spans(&record_with("", attachments)),
        vec![text("Line one"), Span::LineBreak, text("Line two")]
    );
}

#[test]
fn card_text_matches_the_golden_output() {
    #[derive(serde::Deserialize)]
    struct Golden {
        content: String,
        expected: Option<String>,
    }
    let goldens: Vec<Golden> =
        serde_json::from_str(include_str!("fixtures/card_golden.json")).unwrap();
    for golden in goldens {
        assert_eq!(
            teams_core::card_content_text(&golden.content),
            golden.expected,
            "{}",
            golden.content
        );
    }
}
