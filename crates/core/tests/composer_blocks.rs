use teams_core::{
    Draft, DraftLine, LineKind, Mark, MarkKind, TABLE_MARK, Table, TypingStyle, html_to_spans,
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
    assert!(html.contains("<pre class=\"language-rust\">fn x() {}</pre>"));
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
