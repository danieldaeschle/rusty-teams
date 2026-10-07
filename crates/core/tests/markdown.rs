use serde::Deserialize;
use teams_core::markdown_to_html;

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
