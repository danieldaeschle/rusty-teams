use chrono::FixedOffset;
use serde_json::json;
use teams_core::{
    AdaptiveCard, BadgeAppearance, BadgeShape, BadgeStyle, CardElement, ColumnWidth,
    ContainerStyle, FontSize, HorizontalAlignment, IconPosition, LabelPosition, Span, TargetWidth,
    TextColor, TextSize, VerticalAlignment, WidthClass, format_card_dates_in, split_overflow,
};

fn parse(card: serde_json::Value) -> AdaptiveCard {
    AdaptiveCard::parse(&card.to_string()).unwrap()
}

fn first_element(body: serde_json::Value) -> CardElement {
    parse(json!({"body": [body]})).items.remove(0).element
}

#[test]
fn text_blocks_default_to_one_truncated_line() {
    let CardElement::Text(text) =
        first_element(json!({"type": "TextBlock", "text": "a long line"}))
    else {
        panic!("text")
    };
    assert!(!text.wrap);
    assert_eq!(text.max_lines, None);
    assert_eq!(text.alignment, None);
    assert!(!text.monospace);
}

#[test]
fn text_block_layout_properties_are_parsed() {
    let CardElement::Text(text) = first_element(json!({
        "type": "TextBlock", "text": "x", "wrap": true, "maxLines": 2,
        "horizontalAlignment": "Center", "fontType": "Monospace"
    })) else {
        panic!("text")
    };
    assert!(text.wrap && text.monospace);
    assert_eq!(text.max_lines, Some(2));
    assert_eq!(text.alignment, Some(HorizontalAlignment::Center));
}

#[test]
fn heading_style_is_bold_and_larger_unless_sized() {
    let CardElement::Text(heading) =
        first_element(json!({"type": "TextBlock", "text": "h", "style": "heading"}))
    else {
        panic!("text")
    };
    assert!(heading.bold);
    assert_eq!(heading.size, TextSize::Large);
    let CardElement::Text(sized) = first_element(
        json!({"type": "TextBlock", "text": "h", "style": "heading", "size": "small"}),
    ) else {
        panic!("text")
    };
    assert_eq!(sized.size, TextSize::Small);
}

#[test]
fn text_runs_carry_highlight_color_size_and_monospace() {
    let CardElement::Text(text) = first_element(json!({"type": "RichTextBlock", "inlines": [
        {"type": "TextRun", "text": "mark", "highlight": true},
        {"type": "TextRun", "text": "good", "color": "good", "size": "large", "fontType": "monospace"},
        {"type": "TextRun", "text": "quiet", "isSubtle": true}
    ]})) else {
        panic!("text")
    };
    let Span::Colored {
        color, background, ..
    } = &text.spans[0]
    else {
        panic!("highlight is a colored span")
    };
    assert!(color.is_some() && background.is_some());
    let Span::Colored {
        color: Some(_),
        background: None,
        children,
    } = &text.spans[1]
    else {
        panic!("colored run")
    };
    assert_eq!(
        children[0],
        Span::Sized(FontSize::Pixels(18), vec![Span::Code("good".to_owned())])
    );
    assert!(matches!(
        &text.spans[2],
        Span::Colored {
            color: Some(_),
            background: None,
            ..
        }
    ));
}

#[test]
fn images_carry_alt_text_alignment_and_background() {
    let CardElement::Image(image) = first_element(json!({
        "type": "Image", "url": "http://img.example/a.png", "altText": "Logo",
        "horizontalAlignment": "right", "backgroundColor": "#80102030"
    })) else {
        panic!("image")
    };
    assert_eq!(image.url, "http://img.example/a.png");
    assert_eq!(image.alt_text.as_deref(), Some("Logo"));
    assert_eq!(image.alignment, HorizontalAlignment::Right);
    assert_eq!(image.background_color, Some(0x10203080));
    let CardElement::Image(opaque) = first_element(
        json!({"type": "Image", "url": "https://img.example/a.png", "backgroundColor": "#102030"}),
    ) else {
        panic!("image")
    };
    assert_eq!(opaque.background_color, Some(0x102030ff));
    assert_eq!(opaque.alignment, HorizontalAlignment::Left);
}

#[test]
fn containers_columns_and_the_card_keep_layout_properties() {
    let card = parse(json!({
        "minHeight": "120px",
        "verticalContentAlignment": "center",
        "backgroundImage": {"url": "https://img.example/bg.png", "fillMode": "repeat"},
        "body": [
            {"type": "Container", "style": "good", "bleed": true, "rtl": true, "minHeight": "80px",
             "verticalContentAlignment": "bottom", "backgroundImage": "https://img.example/c.png",
             "height": "stretch",
             "items": [{"type": "TextBlock", "text": "x"}]},
            {"type": "ColumnSet", "style": "emphasis", "columns": [
                {"type": "Column", "style": "accent", "minHeight": "40px", "items": [{"type": "TextBlock", "text": "y"}]}
            ]}
        ]
    }));
    assert_eq!(card.layout.min_height, Some(120.));
    assert_eq!(card.layout.vertical_alignment, VerticalAlignment::Center);
    assert_eq!(
        card.layout.background_image.as_deref(),
        Some("https://img.example/bg.png")
    );
    assert!(card.items[0].stretch);
    let CardElement::Container { layout, .. } = &card.items[0].element else {
        panic!("container")
    };
    assert_eq!(layout.style, ContainerStyle::Good);
    assert!(layout.bleed && layout.rtl);
    assert_eq!(layout.min_height, Some(80.));
    assert_eq!(layout.vertical_alignment, VerticalAlignment::Bottom);
    assert_eq!(
        layout.background_image.as_deref(),
        Some("https://img.example/c.png")
    );
    let CardElement::Columns { columns, layout } = &card.items[1].element else {
        panic!("columns")
    };
    assert_eq!(layout.style, ContainerStyle::Emphasis);
    assert_eq!(columns[0].layout.style, ContainerStyle::Accent);
    assert_eq!(columns[0].layout.min_height, Some(40.));
}

#[test]
fn unknown_elements_without_fallback_are_dropped() {
    let card = parse(json!({"body": [
        {"type": "Hologram", "text": "x"},
        {"type": "TextBlock", "text": "kept"}
    ]}));
    assert_eq!(card.items.len(), 1);
}

#[test]
fn fallback_elements_replace_unknown_or_unmet_elements() {
    let card = parse(json!({"body": [
        {"type": "Hologram", "fallback": {"type": "TextBlock", "text": "plain"}},
        {"type": "Hologram", "fallback": "drop", "fallbackText": "never"},
        {"type": "TextBlock", "text": "future", "requires": {"adaptiveCards": "1.9"},
         "fallback": {"type": "TextBlock", "text": "older"}},
        {"type": "TextBlock", "text": "custom", "requires": {"acTest": "1.0"}, "fallback": "drop"},
        {"type": "Hologram", "fallback": {"type": "Hologram", "fallback": {"type": "TextBlock", "text": "deep"}}}
    ]}));
    let texts: Vec<String> = card
        .items
        .iter()
        .map(|item| match &item.element {
            CardElement::Text(text) => match &text.spans[0] {
                Span::Text(text) => text.clone(),
                other => panic!("unexpected span {other:?}"),
            },
            other => panic!("unexpected element {other:?}"),
        })
        .collect();
    assert_eq!(texts, ["plain", "older", "deep"]);
}

#[test]
fn requirements_up_to_version_1_6_are_met() {
    let card = parse(json!({"body": [
        {"type": "TextBlock", "text": "a", "requires": {"adaptiveCards": "1.x"}},
        {"type": "TextBlock", "text": "b", "requires": {"adaptiveCards": "1.6"}},
        {"type": "TextBlock", "text": "c", "requires": {"adaptiveCards": "*"}},
        {"type": "TextBlock", "text": "d", "requires": {"adaptiveCards": "1.7"}}
    ]}));
    assert_eq!(card.items.len(), 3);
}

#[test]
fn the_whole_card_keeps_its_fallback_text() {
    let card = parse(json!({"body": [{"type": "Hologram", "fallbackText": "Open elsewhere"}]}));
    let CardElement::Text(text) = &card.items[0].element else {
        panic!("text")
    };
    assert_eq!(text.spans, vec![Span::Text("Open elsewhere".to_owned())]);
}

#[test]
fn width_classes_follow_the_breakpoints() {
    assert_eq!(WidthClass::from_pixels(180.), WidthClass::VeryNarrow);
    assert_eq!(WidthClass::from_pixels(300.), WidthClass::Narrow);
    assert_eq!(WidthClass::from_pixels(480.), WidthClass::Standard);
    assert_eq!(WidthClass::from_pixels(720.), WidthClass::Wide);
}

#[test]
fn target_width_matches_exact_and_ranged_classes() {
    let exact = TargetWidth::parse("narrow").unwrap();
    assert!(exact.matches(WidthClass::Narrow));
    assert!(!exact.matches(WidthClass::Standard));
    let at_least = TargetWidth::parse("atLeast:standard").unwrap();
    assert!(!at_least.matches(WidthClass::Narrow));
    assert!(at_least.matches(WidthClass::Standard) && at_least.matches(WidthClass::Wide));
    let at_most = TargetWidth::parse("atMost:narrow").unwrap();
    assert!(at_most.matches(WidthClass::VeryNarrow) && at_most.matches(WidthClass::Narrow));
    assert!(!at_most.matches(WidthClass::Standard));
    assert_eq!(TargetWidth::parse("gigantic"), None);
}

#[test]
fn target_width_is_kept_on_items_and_columns() {
    let card = parse(json!({"body": [
        {"type": "TextBlock", "text": "wide only", "targetWidth": "atLeast:wide"},
        {"type": "ColumnSet", "columns": [
            {"type": "Column", "targetWidth": "veryNarrow", "items": [{"type": "TextBlock", "text": "x"}]}
        ]}
    ]}));
    assert!(
        card.items[0]
            .target_width
            .unwrap()
            .matches(WidthClass::Wide)
    );
    let CardElement::Columns { columns, .. } = &card.items[1].element else {
        panic!("columns")
    };
    assert!(
        columns[0]
            .target_width
            .unwrap()
            .matches(WidthClass::VeryNarrow)
    );
}

#[test]
fn tables_resolve_widths_header_and_alignment() {
    let CardElement::Table(table) = first_element(json!({
        "type": "Table",
        "columns": [{"width": 2}, {"width": "80px", "horizontalCellContentAlignment": "right"}],
        "firstRowAsHeader": false, "showGridLines": false, "gridStyle": "accent",
        "horizontalCellContentAlignment": "center", "verticalCellContentAlignment": "bottom",
        "rows": [
            {"type": "TableRow", "style": "emphasis", "cells": [
                {"type": "TableCell", "items": [{"type": "TextBlock", "text": "a"}]},
                {"type": "TableCell", "verticalContentAlignment": "top", "items": [{"type": "TextBlock", "text": "b"}]}
            ]},
            {"type": "TableRow", "cells": [{"type": "TableCell", "items": [{"type": "TextBlock", "text": "c"}]}]}
        ]
    })) else {
        panic!("table")
    };
    assert_eq!(table.columns[0].width, ColumnWidth::Weighted(2.));
    assert_eq!(table.columns[1].width, ColumnWidth::Pixels(80.));
    assert!(!table.first_row_as_header && !table.show_grid_lines);
    assert_eq!(table.grid_style, ContainerStyle::Accent);
    assert_eq!(table.rows[0].style, ContainerStyle::Emphasis);
    let row = &table.rows[0];
    assert_eq!(
        table.cell_horizontal_alignment(row, 0),
        HorizontalAlignment::Center
    );
    assert_eq!(
        table.cell_horizontal_alignment(row, 1),
        HorizontalAlignment::Right
    );
    assert_eq!(
        table.cell_vertical_alignment(row, &row.cells[0], 0),
        VerticalAlignment::Bottom
    );
    assert_eq!(
        table.cell_vertical_alignment(row, &row.cells[1], 1),
        VerticalAlignment::Top
    );
}

#[test]
fn tables_pad_missing_columns_and_expose_cell_text() {
    let card = parse(json!({"body": [{
        "type": "Table",
        "rows": [
            {"cells": [{"items": [{"type": "TextBlock", "text": "a"}]}, {"items": [{"type": "TextBlock", "text": "b"}]}]},
            {"cells": [{"items": [{"type": "TextBlock", "text": "c"}]}]}
        ]
    }]}));
    let CardElement::Table(table) = &card.items[0].element else {
        panic!("table")
    };
    assert_eq!(table.columns.len(), 2);
    assert!(table.first_row_as_header && table.show_grid_lines);
    assert_eq!(card.plain_text().unwrap(), "a\nb\nc");
}

#[test]
fn code_blocks_keep_snippet_language_and_start_line() {
    let CardElement::CodeBlock(block) = first_element(json!({
        "type": "CodeBlock", "codeSnippet": "let a = 1;\nlet b = 2;\n", "language": "Rust", "startLineNumber": 7
    })) else {
        panic!("code block")
    };
    assert_eq!(block.code, "let a = 1;\nlet b = 2;");
    assert_eq!(block.language.as_deref(), Some("Rust"));
    assert_eq!(block.start_line, 7);
    let CardElement::CodeBlock(plain) =
        first_element(json!({"type": "CodeBlock", "codeSnippet": "x"}))
    else {
        panic!("code block")
    };
    assert_eq!(plain.start_line, 1);
}

#[test]
fn badges_parse_style_shape_and_icon() {
    let CardElement::Badge(badge) = first_element(json!({
        "type": "Badge", "text": "Done", "icon": "CheckmarkCircle,Filled", "style": "Good",
        "appearance": "tint", "shape": "rounded", "size": "large", "iconPosition": "After",
        "tooltip": "All good"
    })) else {
        panic!("badge")
    };
    assert_eq!(badge.style, BadgeStyle::Good);
    assert_eq!(badge.appearance, BadgeAppearance::Tint);
    assert_eq!(badge.shape, BadgeShape::Rounded);
    assert_eq!(badge.icon.as_deref(), Some("CheckmarkCircle"));
    assert_eq!(badge.icon_position, IconPosition::After);
    assert_eq!(badge.tooltip.as_deref(), Some("All good"));
    let CardElement::Badge(plain) = first_element(json!({"type": "Badge", "text": "New"})) else {
        panic!("badge")
    };
    assert_eq!(plain.appearance, BadgeAppearance::Filled);
    assert_eq!(plain.shape, BadgeShape::Circular);
    assert!(AdaptiveCard::parse(&json!({"body": [{"type": "Badge"}]}).to_string()).is_none());
}

#[test]
fn progress_elements_parse_value_color_and_label() {
    let CardElement::ProgressBar(bar) =
        first_element(json!({"type": "ProgressBar", "value": 140, "color": "good"}))
    else {
        panic!("bar")
    };
    assert_eq!(bar.value, Some(100.));
    assert_eq!(bar.color, TextColor::Good);
    let CardElement::ProgressBar(indeterminate) = first_element(json!({"type": "ProgressBar"}))
    else {
        panic!("bar")
    };
    assert_eq!(indeterminate.value, None);
    let CardElement::ProgressRing(ring) = first_element(
        json!({"type": "ProgressRing", "label": "Loading", "labelPosition": "Below", "size": "large"}),
    ) else {
        panic!("ring")
    };
    assert_eq!(ring.label.as_deref(), Some("Loading"));
    assert_eq!(ring.label_position, LabelPosition::Below);
}

#[test]
fn compound_buttons_need_a_title() {
    let CardElement::CompoundButton(button) = first_element(json!({
        "type": "CompoundButton", "title": "Open", "description": "Opens the page", "icon": "Open",
        "badge": "New", "selectAction": {"type": "Action.OpenUrl", "url": "https://a.example"}
    })) else {
        panic!("button")
    };
    assert_eq!(button.title, "Open");
    assert_eq!(button.badge.as_deref(), Some("New"));
    assert!(button.select_action.is_some());
    assert!(
        AdaptiveCard::parse(
            &json!({"body": [{"type": "CompoundButton", "description": "x"}]}).to_string()
        )
        .is_none()
    );
}

#[test]
fn carousels_hold_pages_and_ignore_the_timer() {
    let card = parse(json!({"body": [{
        "type": "Carousel", "timer": 5000, "initialPageIndex": 1,
        "pages": [
            {"type": "CarouselPage", "items": [{"type": "TextBlock", "text": "one"}]},
            {"type": "CarouselPage", "items": [{"type": "TextBlock", "text": "two"}]},
            {"type": "CarouselPage", "items": []}
        ]
    }]}));
    let CardElement::Carousel(carousel) = &card.items[0].element else {
        panic!("carousel")
    };
    assert_eq!(carousel.pages.len(), 2);
    assert_eq!(carousel.initial_page, 1);
    assert_eq!(card.plain_text().unwrap(), "one\ntwo");
}

#[test]
fn action_mode_tooltip_and_style_are_parsed() {
    let card = parse(
        json!({"body": [{"type": "TextBlock", "text": "x"}], "actions": [
            {"type": "Action.OpenUrl", "title": "A", "url": "https://a.example", "style": "destructive", "tooltip": "Open A"},
            {"type": "Action.OpenUrl", "title": "B", "url": "https://b.example", "mode": "secondary"}
        ]}),
    );
    assert!(!card.actions[0].secondary);
    assert_eq!(card.actions[0].tooltip.as_deref(), Some("Open A"));
    assert!(card.actions[1].secondary);
    assert!(card.actions[1].is_clickable());
}

fn numbered_actions(count: usize, secondary: &[usize]) -> Vec<teams_core::CardAction> {
    let actions: Vec<_> = (0..count)
        .map(|index| {
            let mut action = json!({"type": "Action.OpenUrl", "title": format!("A{index}"), "url": "https://a.example"});
            if secondary.contains(&index) {
                action["mode"] = json!("secondary");
            }
            action
        })
        .collect();
    parse(json!({"body": [{"type": "TextBlock", "text": "x"}], "actions": actions})).actions
}

#[test]
fn secondary_actions_go_to_the_overflow() {
    let actions = numbered_actions(4, &[1]);
    let split = split_overflow(&actions, |_| true);
    assert_eq!(split.primary, [0, 2, 3]);
    assert_eq!(split.overflow, [1]);
}

#[test]
fn primary_actions_beyond_six_go_to_the_overflow_in_order() {
    let actions = numbered_actions(8, &[3]);
    let split = split_overflow(&actions, |_| true);
    assert_eq!(split.primary, [0, 1, 2, 4, 5, 6]);
    assert_eq!(split.overflow, [3, 7]);
}

#[test]
fn hidden_actions_are_left_out_of_the_split() {
    let actions = numbered_actions(3, &[]);
    let split = split_overflow(&actions, |action| action.title != "A1");
    assert_eq!(split.primary, [0, 2]);
    assert!(split.overflow.is_empty());
}

fn local(text: &str) -> String {
    format_card_dates_in(text, &FixedOffset::east_opt(2 * 3600).unwrap())
}

#[test]
fn dates_format_in_the_given_zone() {
    let iso = "2026-10-09T12:05:00Z";
    assert_eq!(
        local(&format!("{{{{DATE({iso}, SHORT)}}}}")),
        "Fri, Oct 9, 2026"
    );
    assert_eq!(
        local(&format!("{{{{DATE({iso},LONG)}}}}")),
        "Friday, October 9, 2026"
    );
    assert_eq!(
        local(&format!("{{{{DATE({iso}, COMPACT)}}}}")),
        "2026-10-09"
    );
    assert_eq!(local(&format!("{{{{DATE({iso})}}}}")), "2026-10-09");
    assert_eq!(local(&format!("{{{{TIME({iso})}}}}")), "14:05");
}

#[test]
fn the_zone_can_move_the_date() {
    assert_eq!(
        local("{{DATE(2026-10-09T23:30:00Z, SHORT)}}"),
        "Sat, Oct 10, 2026"
    );
}

#[test]
fn dates_work_without_braces_inside_longer_text() {
    assert_eq!(
        local("Due DATE(2026-10-09T12:05:00Z, SHORT) at TIME(2026-10-09T12:05:00Z)."),
        "Due Fri, Oct 9, 2026 at 14:05."
    );
}

#[test]
fn malformed_dates_stay_literal() {
    for text in [
        "{{DATE(yesterday, SHORT)}}",
        "{{DATE(2026-10-09, SHORT)}}",
        "{{TIME()}}",
        "{{DATE(2026-10-09T12:05:00Z, WEEKLY)}}",
    ] {
        assert_eq!(local(text), text);
    }
}

#[test]
fn text_blocks_and_facts_format_dates() {
    let card = parse(json!({"body": [
        {"type": "TextBlock", "text": "On {{DATE(2026-10-09T12:05:00Z, LONG)}}"},
        {"type": "RichTextBlock", "inlines": [{"type": "TextRun", "text": "{{TIME(2026-10-09T12:05:00Z)}}"}]},
        {"type": "FactSet", "facts": [{"title": "Due", "value": "{{DATE(2026-10-09T12:05:00Z, COMPACT)}}"}]}
    ]}));
    let text = card.plain_text().unwrap();
    assert!(!text.contains("{{"), "{text}");
    assert!(text.contains("2026"));
}
