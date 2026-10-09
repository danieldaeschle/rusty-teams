use serde::Deserialize;
use teams_core::{card_markdown_to_html, markdown_to_html};

#[derive(Deserialize)]
struct Golden {
    input: String,
    expected: String,
}

#[test]
fn matches_the_teams_chat_html() {
    let goldens: Vec<Golden> =
        serde_json::from_str(include_str!("fixtures/markdown_golden.json")).unwrap();
    assert!(goldens.len() > 15);
    for golden in goldens {
        assert_eq!(
            markdown_to_html(&golden.input),
            golden.expected,
            "input: {:?}",
            golden.input
        );
    }
}

#[test]
fn list_lines_become_a_list() {
    assert_eq!(
        markdown_to_html("- one\n- **two**"),
        "<ul><li>one</li><li><b>two</b></li></ul>"
    );
}

#[test]
fn list_between_text_has_no_stray_breaks() {
    assert_eq!(
        markdown_to_html("intro\n- a\n- b\noutro"),
        "intro<ul><li>a</li><li>b</li></ul>outro"
    );
}

#[test]
fn separate_lists_stay_separate() {
    assert_eq!(
        markdown_to_html("- a\ntext\n- b"),
        "<ul><li>a</li></ul>text<ul><li>b</li></ul>"
    );
}

#[test]
fn list_marker_needs_a_space_and_line_start() {
    assert_eq!(markdown_to_html("-a\nx - y"), "-a<br>x - y");
}

#[test]
fn list_items_escape_html() {
    assert_eq!(markdown_to_html("- <b>"), "<ul><li>&lt;b&gt;</li></ul>");
}

#[test]
fn list_lines_inside_a_fence_stay_code() {
    assert_eq!(markdown_to_html("```\n- a\n```"), "<pre>- a</pre>");
}

#[test]
fn card_markdown_adds_italic_and_mentions_to_the_message_dialect() {
    assert_eq!(
        card_markdown_to_html("_soft_ and *slanted* and **bold**"),
        "<i>soft</i> and <i>slanted</i> and <b>bold</b>"
    );
    assert_eq!(
        card_markdown_to_html("hi <at>Ada & Co</at> <x>"),
        "hi <at>Ada &amp; Co</at> &lt;x&gt;"
    );
    assert_eq!(
        markdown_to_html("_soft_ <at>Ada</at>"),
        "_soft_ &lt;at&gt;Ada&lt;/at&gt;"
    );
}

#[test]
fn card_markdown_keeps_snake_case_and_urls() {
    assert_eq!(
        card_markdown_to_html("my_var_name https://a.example/_x_/ [l](https://a.example/_y_)"),
        "my_var_name https://a.example/_x_/ <a href=\"https://a.example/_y_\">l</a>"
    );
}
