use teams_core::{
    Draft, DraftLine, LineKind, Mark, MarkKind, RULE_MARK, TABLE_MARK, Table, TypingStyle,
    html_to_spans,
};

fn typed(draft: &mut Draft, text: &str) {
    let mut typing: Option<TypingStyle> = None;
    for character in text.chars() {
        let cursor = draft.text().len();
        let mut next = draft.text().to_owned();
        next.push(character);
        let cursor = draft.apply_edit(&next, cursor + character.len_utf8(), typing.as_ref());
        typing = draft.convert_typed(cursor).map(|(_, style)| style);
        draft.take_edits();
    }
}

fn typed_draft(text: &str) -> Draft {
    let mut draft = Draft::default();
    typed(&mut draft, text);
    draft
}

fn enter(draft: &mut Draft, cursor: usize, shift: bool) -> Option<usize> {
    let result = draft.break_line(cursor, shift);
    draft.take_edits();
    result
}

fn table_of(rows: &[&[&str]]) -> Table {
    Table::from_rows(
        rows.iter()
            .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
            .collect(),
    )
}

#[test]
fn enter_in_a_quote_adds_a_quote_line_and_enter_on_the_empty_one_leaves() {
    let mut draft = typed_draft(">abc");
    let cursor = enter(&mut draft, 3, false).unwrap();
    assert_eq!(draft.text(), "abc\n");
    assert_eq!(draft.lines(), [LineKind::Quote, LineKind::Quote]);
    let cursor = enter(&mut draft, cursor, false).unwrap();
    assert_eq!(draft.lines(), [LineKind::Quote, LineKind::Text]);
    assert_eq!(cursor, 4);
    assert_eq!(enter(&mut draft, cursor, false), None);
    assert_eq!(draft.to_html(), "<blockquote>abc</blockquote>");
}

#[test]
fn enter_on_an_empty_quote_line_in_the_middle_splits_the_quote() {
    let mut draft = Draft::from_markdown("> a\n> \n> b");
    enter(&mut draft, 2, false).unwrap();
    assert_eq!(
        draft.lines(),
        [LineKind::Quote, LineKind::Text, LineKind::Quote]
    );
}

#[test]
fn shift_enter_keeps_breaking_inside_the_quote() {
    let mut draft = typed_draft(">a");
    let cursor = enter(&mut draft, 1, true).unwrap();
    let cursor = enter(&mut draft, cursor, true).unwrap();
    assert_eq!(cursor, 3);
    assert_eq!(
        draft.lines(),
        [LineKind::Quote, LineKind::Quote, LineKind::Quote]
    );
}

#[test]
fn backspace_at_the_start_of_a_quote_leaves_it_or_joins_the_line_above() {
    let mut empty = typed_draft(">");
    assert_eq!(empty.backspace_at_start(0), Some(0));
    assert_eq!(empty.lines(), [LineKind::Text]);

    let mut joined = Draft::from_markdown("x\n> abc");
    assert_eq!(joined.backspace_at_start(2), Some(1));
    assert_eq!(joined.text(), "xabc");
    assert_eq!(joined.lines(), [LineKind::Text]);

    let mut inner = Draft::from_markdown("> a\n> b");
    assert_eq!(inner.backspace_at_start(2), Some(1));
    assert_eq!(inner.text(), "ab");
    assert_eq!(inner.lines(), [LineKind::Quote]);
}

#[test]
fn down_at_the_end_of_a_closing_quote_opens_a_text_line_below() {
    let mut draft = typed_draft(">abc");
    assert_eq!(draft.leave_quote_at_end(1), None);
    assert_eq!(draft.leave_quote_at_end(3), Some(4));
    assert_eq!(draft.lines(), [LineKind::Quote, LineKind::Text]);
    assert_eq!(Draft::plain("x").leave_quote_at_end(1), None);
}

#[test]
fn teams_shortcuts_make_headings_bold_italic_and_code() {
    let heading = typed_draft("## Plan");
    assert_eq!(heading.text(), "Plan");
    assert_eq!(heading.lines(), [LineKind::Heading(2)]);
    assert_eq!(heading.to_html(), "<h2>Plan</h2>");
    assert_eq!(typed_draft("#### deep").lines(), [LineKind::Heading(3)]);
    assert_eq!(typed_draft("#nope").lines(), [LineKind::Text]);

    let inline = typed_draft("*b* _i_ `c`");
    assert_eq!(inline.text(), "b i c");
    assert_eq!(
        inline.marks(),
        [
            Mark {
                range: 0..1,
                kind: MarkKind::Bold
            },
            Mark {
                range: 2..3,
                kind: MarkKind::Italic
            },
            Mark {
                range: 4..5,
                kind: MarkKind::Code
            },
        ]
    );
}

#[test]
fn enter_in_a_heading_is_left_to_send_and_backspace_makes_it_text() {
    let mut draft = typed_draft("# a");
    assert_eq!(enter(&mut draft, 1, false), None);
    assert_eq!(draft.backspace_at_start(0), Some(0));
    assert_eq!(draft.lines(), [LineKind::Text]);
}

#[test]
fn an_inserted_table_takes_an_empty_line_and_gets_a_line_after_it() {
    let mut draft = Draft::default();
    let index = draft.insert_table(0, Table::new(2, 3));
    assert_eq!(index, 0);
    assert_eq!(draft.text(), format!("{TABLE_MARK}\n"));
    assert!(draft.lines()[0].is_table());
    assert_eq!(draft.lines()[1], LineKind::Text);

    let mut below = Draft::plain("hello");
    assert_eq!(below.insert_table(2, Table::new(1, 1)), 1);
    assert_eq!(below.text(), format!("hello\n{TABLE_MARK}\n"));
}

#[test]
fn a_table_is_sent_as_teams_html_between_the_text() {
    let mut draft = Draft::plain("before");
    let index = draft.insert_table(6, Table::new(1, 2));
    draft.set_table(index, table_of(&[&["a", "b"]]));
    let end = draft.text().len();
    let mut text = draft.text().to_owned();
    text.push_str("after");
    draft.apply_edit(&text, end + 5, None);
    assert_eq!(
        draft.trimmed().to_html(),
        "before<figure class=\"table\"><table class=\"copy-paste-table\"><tbody>\
         <tr><td>a</td><td>b</td></tr></tbody></table></figure>after"
    );
    assert_eq!(draft.plain_text(), "before\na\tb\nafter");
    assert!(
        !Draft::from_lines(&[DraftLine {
            kind: LineKind::Table(Table::new(1, 1)),
            number: None,
            content: TABLE_MARK.to_string(),
            marks: Vec::new(),
        }])
        .plain_text()
        .contains(TABLE_MARK)
    );
}

#[test]
fn text_typed_next_to_a_table_moves_to_its_own_line() {
    let mut draft = Draft::default();
    draft.insert_table(0, Table::new(1, 1));
    let mark = TABLE_MARK.len_utf8();
    let mut text = draft.text().to_owned();
    text.insert(mark, 'x');
    let cursor = draft.apply_edit(&text, mark + 1, None);
    assert_eq!(draft.text(), format!("{TABLE_MARK}\nx\n"));
    assert_eq!(cursor, mark + 2);
    assert!(draft.lines()[0].is_table());
    assert_eq!(draft.lines()[1], LineKind::Text);

    let mut text = draft.text().to_owned();
    text.insert(0, 'y');
    let cursor = draft.apply_edit(&text, 1, None);
    assert_eq!(draft.text(), format!("y\n{TABLE_MARK}\nx\n"));
    assert_eq!(cursor, 1);
    assert!(draft.lines()[1].is_table());
}

#[test]
fn deleting_the_mark_drops_the_table() {
    let mut draft = Draft::plain("a");
    draft.insert_table(1, Table::new(1, 1));
    draft.apply_edit("a\n\n", 2, None);
    assert!(draft.lines().iter().all(|kind| !kind.is_table()));
}

#[test]
fn delete_before_a_table_and_backspace_after_it_find_the_table() {
    let mut draft = Draft::plain("a");
    let index = draft.insert_table(1, Table::new(1, 1));
    assert_eq!(draft.table_after(1), Some(index));
    assert_eq!(draft.delete_forward(1), Some(1));
    assert!(draft.lines()[index].is_table());
    let after = draft.text().len();
    assert_eq!(draft.table_before(after), Some(index));
    assert_eq!(draft.table_before(0), None);
}

#[test]
fn leaving_a_table_lands_beside_it_and_adds_a_line_when_needed() {
    let mut draft = Draft::default();
    draft.insert_table(0, Table::new(1, 1));
    assert_eq!(draft.line_beside_table(0, false), 0);
    assert_eq!(draft.lines()[0], LineKind::Text);
    assert!(draft.lines()[1].is_table());
    let below = draft.line_beside_table(1, true);
    assert_eq!(draft.line_at(below), 2);
}

#[test]
fn removing_a_table_keeps_the_lines_around_it() {
    let mut draft = Draft::from_markdown("> a");
    let index = draft.insert_table(3, Table::new(1, 1));
    let end = draft.text().len();
    let mut text = draft.text().to_owned();
    text.push('z');
    draft.apply_edit(&text, end + 1, None);
    let cursor = draft.remove_table(index);
    assert_eq!(draft.text(), "a\nz");
    assert_eq!(draft.lines(), [LineKind::Quote, LineKind::Text]);
    assert_eq!(cursor, 2);
}

#[test]
fn quote_and_list_buttons_leave_a_table_alone() {
    let mut draft = Draft::plain("a");
    let index = draft.insert_table(1, Table::new(1, 1));
    let end = draft.text().len();
    draft.toggle_lines(0..end, LineKind::Quote);
    assert!(draft.lines()[index].is_table());
}

#[test]
fn a_copied_table_pastes_into_the_middle_of_a_line() {
    let mut source = Draft::default();
    let index = source.insert_table(0, table_of(&[&["1", "2"]]));
    let copied = source.slice(0..TABLE_MARK.len_utf8());
    let mut draft = Draft::plain("abcd");
    draft.insert_lines(2..2, &copied);
    assert_eq!(draft.text(), format!("ab\n{TABLE_MARK}\ncd"));
    assert_eq!(draft.table(1), source.table(index));
}

#[test]
fn pasted_markdown_becomes_headings_lists_quotes_code_and_tables() {
    let markdown = "# Title\n\nSome **bold**, *it*, ~~gone~~, `code` and [docs](https://x.de)\n\n\
                    1. one\n   1. nested\n2. two\n- dot\n  - inner\n\n> quoted\n\n```rust\nfn x() {}\n```\n\n\
                    | A | B |\n|---|---|\n| **1** | 2 |\n";
    let draft = Draft::from_markdown(markdown);
    let kinds: Vec<LineKind> = draft
        .lines()
        .iter()
        .filter(|kind| **kind != LineKind::Text)
        .cloned()
        .collect();
    assert_eq!(
        kinds,
        [
            LineKind::Heading(1),
            LineKind::Numbered(0),
            LineKind::Numbered(1),
            LineKind::Numbered(0),
            LineKind::Bullet(0),
            LineKind::Bullet(1),
            LineKind::Quote,
            LineKind::Code(Some("rust".into())),
            LineKind::Table(table_of(&[&["A", "B"], &["1", "2"]])),
        ]
    );
    let html = draft.trimmed().to_html();
    assert!(html.starts_with("<h1>Title</h1>"), "{html}");
    assert!(html.contains("<b>bold</b>, <i>it</i>, <s>gone</s>, <code>code</code>"));
    assert!(html.contains("<a href=\"https://x.de\">docs</a>"));
    assert!(html.contains("<blockquote>quoted</blockquote>"));
    assert!(html.contains("<pre class=\"language-rust skipProofing\" spellcheck=\"false\"><code>fn x() {}</code></pre>"));
    assert!(
        html.ends_with("<td>1</td><td>2</td></tr></tbody></table></figure>"),
        "{html}"
    );
}

#[test]
fn text_with_pipes_but_no_delimiter_row_stays_text() {
    assert!(!teams_core::has_markdown("a | b\nc | d"));
    assert!(teams_core::has_markdown("| a | b |\n| - | - |"));
    assert!(teams_core::has_markdown("## Heading"));
}

#[test]
fn received_tables_and_headings_edit_as_tables_and_headings() {
    let spans = html_to_spans(
        "<h2>Notes</h2><table><tbody><tr><td><p>a</p></td><td>b<br>c</td></tr></tbody></table><p>end</p>",
    );
    let draft = Draft::from_spans(&spans);
    assert_eq!(draft.lines()[0], LineKind::Heading(2));
    assert_eq!(
        draft.lines()[1],
        LineKind::Table(table_of(&[&["a", "b\nc"]]))
    );
    assert_eq!(draft.lines()[2], LineKind::Text);
}

#[test]
fn pasting_markdown_with_a_table_leaves_the_cursor_after_the_paste() {
    let markdown = "## Plan\n- dot\n> quoted\n\n| Area | Owner |\n|---|---|\n| Sync | Priya |\nend";
    let lines = Draft::from_markdown(markdown).slice(0..usize::MAX);
    let mut draft = Draft::default();
    let cursor = draft.insert_lines(0..0, &lines);
    assert_eq!(cursor, draft.text().len(), "{:?}", draft.text());
}

#[test]
fn a_sent_table_edits_back_into_the_same_table() {
    let table = table_of(&[&["a", "x\ny"], &["", "b"]]);
    let draft = Draft::from_spans(&html_to_spans(&table.to_html()));
    let LineKind::Table(received) = &draft.lines()[0] else {
        panic!("{:?}", draft.lines());
    };
    assert_eq!(received.rows[0], ["a", "x\ny"]);
    assert!(received.rows[1][0].trim().is_empty());
    assert_eq!(received.to_html(), table.to_html());
}

#[test]
fn a_table_as_chatsvc_stores_it_reads_back_into_the_sent_table() {
    let table = table_of(&[&["a", "b"], &["1", "2"], &["line one\nline two", ""]]);
    let stored = "<table>\n<tbody>\n<tr>\n<td>a</td>\n<td>b</td>\n</tr>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n<tr>\n<td>\n<p>line one</p>\n<p>line two</p>\n</td>\n<td>&nbsp;</td>\n</tr>\n</tbody>\n</table>";
    let draft = Draft::from_spans(&html_to_spans(stored));
    let LineKind::Table(received) = &draft.lines()[0] else {
        panic!("{:?}", draft.lines());
    };
    assert_eq!(received.rows[..2], table.rows[..2]);
    assert_eq!(received.rows[2][0], "line one\nline two");
    assert!(received.rows[2][1].trim().is_empty());
    assert_eq!(received.to_html(), table.to_html());
}

#[test]
fn font_colour_and_highlight_send_teams_spans() {
    let mut draft = Draft::plain("red and marked");
    draft.set_color(0..3, Some(0xB6424C));
    draft.set_highlight(8..14, Some(0xFDD472));
    assert_eq!(
        draft.to_html(),
        "<span style=\"color:#B6424C;\">red</span> and \
         <span style=\"background-color:#FDD472;\">marked</span>"
    );
    draft.set_color(1..3, Some(0x1E53A3));
    assert_eq!(draft.color_in(0..1, &MarkKind::Color(0)), Some(0xB6424C));
    assert_eq!(draft.color_in(1..3, &MarkKind::Color(0)), Some(0x1E53A3));
    draft.set_color(0..3, None);
    assert_eq!(draft.color_in(0..3, &MarkKind::Color(0)), None);
}

#[test]
fn received_colours_come_back_as_marks() {
    let html = "<p><span style=\"color:#B6424C;\">a</span> \
                <span style=\"background-color: rgb(253, 212, 114)\">b</span></p>";
    let draft = Draft::from_spans(&html_to_spans(html));
    assert_eq!(draft.text(), "a b");
    assert!(draft.marks().contains(&Mark {
        range: 0..1,
        kind: MarkKind::Color(0xB6424C)
    }));
    assert!(draft.marks().contains(&Mark {
        range: 2..3,
        kind: MarkKind::Highlight(0xFDD472)
    }));
}

#[test]
fn typing_style_picks_and_drops_a_colour() {
    let mut typing = TypingStyle::default();
    typing.set_color(Some(0x2B9B62), &[MarkKind::Color(0xB6424C), MarkKind::Bold]);
    assert_eq!(
        typing.apply(vec![MarkKind::Color(0xB6424C), MarkKind::Bold]),
        [MarkKind::Bold, MarkKind::Color(0x2B9B62)]
    );
    typing.clear(&[MarkKind::Bold, MarkKind::Link("https://x.de".into())]);
    assert_eq!(
        typing.apply(vec![MarkKind::Bold, MarkKind::Link("https://x.de".into())]),
        [MarkKind::Link("https://x.de".into())]
    );
}

#[test]
fn clear_formatting_keeps_links_and_drops_the_indent() {
    let mut draft = Draft::from_markdown("**bold** [l](https://x.de)");
    draft.set_highlight(0..4, Some(0xDF9299));
    draft.indent_blocks(0..0, false);
    let length = draft.text().len();
    draft.clear_formatting(0..length);
    assert_eq!(draft.lines(), [LineKind::Text]);
    assert_eq!(
        draft.marks(),
        [Mark {
            range: 5..6,
            kind: MarkKind::Link("https://x.de".into())
        }]
    );
}

#[test]
fn indent_moves_paragraphs_by_teams_block_steps_and_nests_lists() {
    let mut draft = Draft::plain("a\nb");
    draft.indent_blocks(0..0, false);
    draft.indent_blocks(0..0, false);
    draft.indent_blocks(2..3, false);
    assert_eq!(draft.lines(), [LineKind::Indent(2), LineKind::Indent(1)]);
    assert_eq!(
        draft.to_html(),
        "<p style=\"margin-left:80px;\">a</p><p style=\"margin-left:40px;\">b</p>"
    );
    assert!(draft.can_outdent(0..0));
    draft.indent_blocks(0..3, true);
    assert_eq!(draft.lines(), [LineKind::Indent(1), LineKind::Text]);
    assert!(!draft.can_outdent(2..3));
    let mut list = Draft::from_markdown("- x");
    list.indent_blocks(0..0, false);
    assert_eq!(list.lines(), [LineKind::Bullet(1)]);
}

#[test]
fn shift_enter_keeps_the_indent_and_enter_is_left_to_send() {
    let mut draft = Draft::plain("a");
    draft.indent_blocks(0..0, false);
    assert_eq!(enter(&mut draft, 1, false), None);
    assert_eq!(enter(&mut draft, 1, true), Some(2));
    assert_eq!(draft.lines(), [LineKind::Indent(1), LineKind::Indent(1)]);
}

#[test]
fn received_margins_become_indent_levels() {
    let draft = Draft::from_spans(&html_to_spans(
        "<p style=\"margin-left:80px\">deep</p><p>flat</p><p style=\"margin-left: 40px;\">one</p>",
    ));
    assert_eq!(
        draft.lines(),
        [LineKind::Indent(2), LineKind::Text, LineKind::Indent(1)]
    );
    assert_eq!(draft.text(), "deep\nflat\none");
}

#[test]
fn a_rule_is_its_own_line_sent_as_hr() {
    let mut draft = Draft::plain("a");
    let cursor = draft.insert_rule(1);
    draft.take_edits();
    assert_eq!(draft.text(), format!("a\n{RULE_MARK}\n"));
    assert_eq!(cursor, draft.text().len());
    assert_eq!(
        draft.lines(),
        [LineKind::Text, LineKind::Rule, LineKind::Text]
    );
    assert_eq!(draft.trimmed().to_html(), "a<hr>");
    assert_eq!(draft.plain_text(), "a\n\n");
    assert_eq!(draft.backspace_at_start(cursor), Some(2));
    assert_eq!(draft.text(), "a\n");
    assert_eq!(draft.lines(), [LineKind::Text, LineKind::Text]);
}

#[test]
fn text_typed_onto_a_rule_line_moves_off_it() {
    let mut draft = Draft::default();
    draft.insert_rule(0);
    let mut next = draft.text().to_owned();
    next.insert(0, 'x');
    draft.apply_edit(&next, 1, None);
    assert_eq!(draft.text(), format!("x\n{RULE_MARK}\n"));
    assert_eq!(draft.lines()[1], LineKind::Rule);
}

#[test]
fn received_rules_come_back_as_rule_lines() {
    let draft = Draft::from_spans(&html_to_spans("<p>a</p><hr><p>b</p>"));
    assert_eq!(
        draft.lines(),
        [LineKind::Text, LineKind::Rule, LineKind::Text]
    );
    assert_eq!(draft.to_html(), "a<hr>b");
}

#[test]
fn paragraph_style_sets_and_clears_headings() {
    let mut draft = Draft::plain("title\nbody");
    draft.set_heading(0..0, Some(2));
    assert_eq!(draft.heading_at(0), Some(2));
    assert_eq!(draft.to_html(), "<h2>title</h2>body");
    draft.set_heading(0..8, Some(1));
    assert_eq!(draft.lines(), [LineKind::Heading(1), LineKind::Heading(1)]);
    draft.set_heading(0..0, None);
    assert_eq!(draft.lines(), [LineKind::Text, LineKind::Heading(1)]);
}

#[test]
fn code_blocks_send_the_teams_pre_code_shape_and_read_back_line_breaks() {
    let draft = Draft::from_lines(&[
        DraftLine {
            kind: LineKind::Code(None),
            number: None,
            content: "a < b".into(),
            marks: Vec::new(),
        },
        DraftLine {
            kind: LineKind::Code(None),
            number: None,
            content: "  c".into(),
            marks: Vec::new(),
        },
    ]);
    let html = draft.to_html();
    assert_eq!(
        html,
        "<pre class=\"language-plaintext skipProofing\" spellcheck=\"false\"><code>a &lt; b<br>  c</code></pre>"
    );
    let back = Draft::from_spans(&html_to_spans(&html));
    assert_eq!(back.lines(), [LineKind::Code(None), LineKind::Code(None)]);
    assert_eq!(back.text(), "a < b\n  c");
}
