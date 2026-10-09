use chrono::Local;
use serde_json::{Value, json};

const CARD_POSTER_URL: &str = "https://avatars.githubusercontent.com/u/9919?s=360";
const CARD_ICON_URL: &str = "https://avatars.githubusercontent.com/u/9919?s=32";
const VIDEO_URL: &str = "https://example.com/videos/release-tour.mp4";

pub fn behaviour_cards() -> Vec<Value> {
    vec![
        refresh_card("Build status: waiting for the automatic refresh..."),
        actions_card(),
        text_card(),
        select_action_card(),
        media_and_icon_card(),
        text_layout_card(),
        text_runs_card(),
        layout_card(),
        fallback_card(),
        target_width_card(),
        table_card(),
        code_block_card(),
        badge_card(),
        progress_card(),
        compound_button_card(),
        carousel_card(),
        action_overflow_card(),
        dates_card(),
    ]
}

pub fn refreshed_card() -> Value {
    refresh_card(&format!(
        "Build #482 passed (refreshed at {})",
        Local::now().format("%H:%M:%S")
    ))
}

fn refresh_card(status: &str) -> Value {
    json!({
        "type": "AdaptiveCard",
        "refresh": {"action": {"type": "Action.Execute", "verb": "refreshStatus", "data": {"build": 482}}},
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Refreshing card"},
            {"type": "TextBlock", "wrap": true, "text": status},
            {"type": "TextBlock", "wrap": true, "isSubtle": true, "size": "small", "text": "Updates itself once when it is first shown. Use the message menu to refresh again."}
        ]
    })
}

fn actions_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Submit variants"}
        ],
        "actions": [
            {"type": "Action.Submit", "title": "Open tab", "iconUrl": "icon:Open", "data": {"msteams": {"type": "invoke", "value": {"type": "tab/tabInfoAction", "tabInfo": {
                "contentUrl": "https://example.com/tabs/content", "websiteUrl": "https://example.com/tabs/site", "name": "Release tab", "entityId": "release"
            }}}}},
            {"type": "Action.Submit", "title": "Sign in", "iconUrl": "icon:LockClosed", "data": {"msteams": {"type": "signin", "value": "https://example.com/signin"}}},
            {"type": "Action.Submit", "title": "Custom invoke", "data": {"msteams": {"type": "invoke", "value": {"type": "demo/custom", "payload": 1}}}},
            {"type": "Action.Submit", "title": "Add app", "data": {"msteams": {"type": "invoke", "value": {"type": "appInstallToConversation"}}}}
        ]
    })
}

fn text_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "msteams": {"entities": [
            {"type": "mention", "text": "<at>Mara Lindqvist</at>", "mentioned": {"id": "29:mara", "name": "Mara Lindqvist"}}
        ]},
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Mentions and italics"},
            {"type": "TextBlock", "wrap": true, "text": "Hi <at>Mara Lindqvist</at>, this is _italic_, this is *also italic*, and **bold**. Names like snake_case_name and https://example.com/a_b_c stay as they are."},
            {"type": "FactSet", "facts": [{"title": "Reviewer", "value": "<at>Dana Demo</at>"}, {"title": "Note", "value": "_soft_ launch"}]},
            {"type": "RichTextBlock", "inlines": [{"type": "TextRun", "text": "cc "}, {"type": "TextRun", "text": "<at>Priya Nair</at>", "weight": "bolder"}]}
        ]
    })
}

fn select_action_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "selectAction": {"type": "Action.OpenUrl", "url": "https://example.com/card"},
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Clickable areas"},
            {"type": "TextBlock", "wrap": true, "isSubtle": true, "size": "small", "text": "The card, the container, the column and the image each open a page or toggle the note."},
            {"type": "Container", "style": "emphasis", "selectAction": {"type": "Action.ToggleVisibility", "targetElements": ["container-note"]}, "items": [
                {"type": "TextBlock", "wrap": true, "text": "Container: click to toggle the note"},
                {"type": "TextBlock", "id": "container-note", "isVisible": false, "isSubtle": true, "text": "Container note"}
            ]},
            {"type": "ColumnSet", "columns": [
                {"type": "Column", "width": "auto", "selectAction": {"type": "Action.OpenUrl", "url": "https://example.com/image"}, "items": [
                    {"type": "Image", "url": CARD_ICON_URL, "size": "small", "selectAction": {"type": "Action.OpenUrl", "url": "https://example.com/image"}}
                ]},
                {"type": "Column", "width": "stretch", "selectAction": {"type": "Action.Submit", "data": {"action": "watch"}}, "items": [
                    {"type": "TextBlock", "wrap": true, "text": "Column: click to submit"}
                ]}
            ]}
        ]
    })
}

fn media_and_icon_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Media and icons"},
            {"type": "Media", "poster": CARD_POSTER_URL, "altText": "Release tour video", "sources": [{"mimeType": "video/mp4", "url": VIDEO_URL}]},
            {"type": "ColumnSet", "columns": [
                {"type": "Column", "width": "auto", "items": [{"type": "Icon", "name": "Calendar", "size": "Large", "color": "accent"}]},
                {"type": "Column", "width": "auto", "items": [{"type": "Icon", "name": "Mail", "size": "Medium"}]},
                {"type": "Column", "width": "auto", "items": [{"type": "Icon", "name": "Warning", "size": "Standard", "color": "warning", "selectAction": {"type": "Action.OpenUrl", "url": "https://example.com/warning"}}]},
                {"type": "Column", "width": "auto", "items": [{"type": "Icon", "name": "Alert", "size": "Small", "color": "good"}]},
                {"type": "Column", "width": "auto", "items": [{"type": "Icon", "name": "NoSuchFluentName", "size": "Large"}]}
            ]}
        ],
        "actions": [
            {"type": "Action.OpenUrl", "title": "Send", "url": "https://example.com/send", "iconUrl": "icon:Send"},
            {"type": "Action.OpenUrl", "title": "Logo", "url": "https://example.com/logo", "iconUrl": CARD_ICON_URL}
        ]
    })
}

fn text_layout_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "style": "heading", "text": "Text layout"},
            {"type": "TextBlock", "text": "wrap is false by default, so this long line is cut off with an ellipsis at the card edge instead of overflowing"},
            {"type": "TextBlock", "wrap": true, "maxLines": 2, "text": "maxLines 2 with wrap: this text is long enough to need more than two lines, so the third line is cut off with an ellipsis. More words follow to fill the space and prove the clamp."},
            {"type": "TextBlock", "horizontalAlignment": "center", "text": "Centered"},
            {"type": "TextBlock", "horizontalAlignment": "right", "text": "Right aligned"},
            {"type": "TextBlock", "fontType": "monospace", "text": "monospace: let answer = 42;"},
            {"type": "TextBlock", "color": "dark", "text": "Dark colour stays readable"},
            {"type": "TextBlock", "color": "light", "text": "Light colour stays readable"}
        ]
    })
}

fn text_runs_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Text runs"},
            {"type": "RichTextBlock", "inlines": [
                {"type": "TextRun", "text": "highlighted ", "highlight": true},
                {"type": "TextRun", "text": "good ", "color": "good", "weight": "bolder"},
                {"type": "TextRun", "text": "attention ", "color": "attention"},
                {"type": "TextRun", "text": "subtle ", "isSubtle": true},
                {"type": "TextRun", "text": "large ", "size": "large"},
                {"type": "TextRun", "text": "small ", "size": "small"},
                {"type": "TextRun", "text": "mono", "fontType": "monospace"}
            ]},
            {"type": "RichTextBlock", "horizontalAlignment": "center", "inlines": [{"type": "TextRun", "text": "centered rich text"}]}
        ]
    })
}

fn layout_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "minHeight": "260px",
        "verticalContentAlignment": "center",
        "backgroundImage": {"url": CARD_POSTER_URL, "fillMode": "cover"},
        "body": [
            {"type": "Container", "style": "good", "bleed": true, "items": [
                {"type": "TextBlock", "weight": "bolder", "text": "Bleed container touches the card edges"}
            ]},
            {"type": "TextBlock", "weight": "bolder", "text": "Card with background image, minHeight and centered content"},
            {"type": "Container", "style": "emphasis", "minHeight": "70px", "verticalContentAlignment": "center", "items": [
                {"type": "TextBlock", "text": "minHeight 70 with centered text"}
            ]},
            {"type": "Container", "style": "accent", "rtl": true, "items": [
                {"type": "TextBlock", "text": "rtl container aligns text right"}
            ]},
            {"type": "Container", "style": "warning", "minHeight": "90px", "items": [
                {"type": "TextBlock", "text": "Top"},
                {"type": "Container", "height": "stretch", "style": "emphasis", "items": [{"type": "TextBlock", "text": "height: stretch fills the free space"}]},
                {"type": "TextBlock", "weight": "bolder", "text": "Bottom"}
            ]},
            {"type": "ColumnSet", "rtl": true, "style": "emphasis", "columns": [
                {"type": "Column", "width": "stretch", "style": "attention", "items": [{"type": "TextBlock", "text": "First column"}]},
                {"type": "Column", "width": "stretch", "style": "good", "items": [{"type": "TextBlock", "text": "Second column"}]}
            ]},
            {"type": "Image", "url": CARD_ICON_URL, "altText": "Hover for alt text", "size": "small", "horizontalAlignment": "center", "backgroundColor": "#FFB0B0B0"}
        ]
    })
}

fn fallback_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "fallbackText": "This card needs a newer client",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Fallback"},
            {"type": "Hologram", "fallback": {"type": "TextBlock", "wrap": true, "text": "Unknown element replaced by its fallback"}},
            {"type": "Hologram", "fallback": "drop"},
            {"type": "Hologram"},
            {"type": "TextBlock", "text": "never shown", "requires": {"adaptiveCards": "1.9"}, "fallback": {"type": "TextBlock", "wrap": true, "text": "Needs 1.9, fell back to this"}},
            {"type": "TextBlock", "wrap": true, "text": "Requires 1.x is met", "requires": {"adaptiveCards": "1.x"}},
            {"type": "TextBlock", "wrap": true, "text": "Hidden: unknown feature", "requires": {"fancyFeature": "1.0"}, "fallback": "drop"}
        ]
    })
}

fn target_width_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Target width (this card is standard)"},
            {"type": "TextBlock", "targetWidth": "standard", "text": "Visible: standard"},
            {"type": "TextBlock", "targetWidth": "atLeast:standard", "text": "Visible: atLeast:standard"},
            {"type": "TextBlock", "targetWidth": "atMost:standard", "text": "Visible: atMost:standard"},
            {"type": "TextBlock", "targetWidth": "wide", "text": "Hidden: wide"},
            {"type": "TextBlock", "targetWidth": "veryNarrow", "text": "Hidden: veryNarrow"},
            {"type": "TextBlock", "targetWidth": "atMost:narrow", "text": "Hidden: atMost:narrow"}
        ]
    })
}

fn table_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Table"},
            {"type": "Table", "gridStyle": "accent", "horizontalCellContentAlignment": "center", "verticalCellContentAlignment": "center",
             "columns": [{"width": 2, "horizontalCellContentAlignment": "left"}, {"width": 1}, {"width": "70px", "horizontalCellContentAlignment": "right"}],
             "rows": [
                {"type": "TableRow", "cells": [
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "Service"}]},
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "State"}]},
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "Ms"}]}
                ]},
                {"type": "TableRow", "cells": [
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "API gateway"}]},
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "color": "good", "text": "Healthy"}]},
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "42"}]}
                ]},
                {"type": "TableRow", "style": "attention", "cells": [
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "Search"}]},
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "color": "attention", "text": "Degraded"}]},
                    {"type": "TableCell", "selectAction": {"type": "Action.OpenUrl", "url": "https://example.com/search"}, "items": [{"type": "TextBlock", "wrap": true, "text": "1830"}]}
                ]}
            ]},
            {"type": "Table", "firstRowAsHeader": false, "showGridLines": false, "rows": [
                {"type": "TableRow", "cells": [
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "No header"}]},
                    {"type": "TableCell", "items": [{"type": "TextBlock", "wrap": true, "text": "No grid lines"}]}
                ]}
            ]}
        ]
    })
}

fn code_block_card() -> Value {
    let snippet: String = (1..=14)
        .map(|line| format!("    let value_{line} = compute({line});\n"))
        .collect();
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Code blocks"},
            {"type": "CodeBlock", "language": "Rust", "startLineNumber": 8, "codeSnippet": format!("fn main() {{\n{snippet}}}")},
            {"type": "CodeBlock", "language": "Json", "codeSnippet": "{\"short\": true}"}
        ]
    })
}

fn badge_card() -> Value {
    let styles = [
        "default",
        "accent",
        "good",
        "attention",
        "warning",
        "subtle",
        "informative",
    ];
    let filled: Vec<Value> = styles
        .iter()
        .map(|style| json!({"type": "Badge", "text": style, "style": style, "icon": "Checkmark"}))
        .collect();
    let tinted: Vec<Value> = styles
        .iter()
        .map(|style| json!({"type": "Badge", "text": style, "style": style, "appearance": "tint"}))
        .collect();
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Badges"},
            {"type": "Container", "items": filled},
            {"type": "Container", "spacing": "small", "items": tinted},
            {"type": "ColumnSet", "spacing": "small", "columns": [
                {"type": "Column", "width": "auto", "items": [{"type": "Badge", "text": "Square", "shape": "square", "style": "accent"}]},
                {"type": "Column", "width": "auto", "items": [{"type": "Badge", "text": "Rounded", "shape": "rounded", "style": "accent"}]},
                {"type": "Column", "width": "auto", "items": [{"type": "Badge", "text": "Large", "size": "large", "style": "good", "icon": "Star", "iconPosition": "After", "tooltip": "Large badge with trailing icon"}]},
                {"type": "Column", "width": "auto", "items": [{"type": "Badge", "text": "XL", "size": "extraLarge", "style": "warning"}]}
            ]}
        ]
    })
}

fn progress_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Progress"},
            {"type": "ProgressBar", "value": 65},
            {"type": "ProgressBar", "value": 30, "color": "good", "spacing": "small"},
            {"type": "ProgressBar", "color": "attention", "spacing": "small"},
            {"type": "ProgressRing", "label": "Loading data...", "spacing": "medium"},
            {"type": "ProgressRing", "label": "Above", "labelPosition": "above", "size": "large", "spacing": "medium"},
            {"type": "ProgressRing", "size": "tiny", "spacing": "medium"}
        ]
    })
}

fn compound_button_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Compound buttons"},
            {"type": "CompoundButton", "icon": "Calendar", "title": "Schedule a meeting", "description": "Find a time that works for everyone", "badge": "New", "selectAction": {"type": "Action.OpenUrl", "url": "https://example.com/schedule"}},
            {"type": "CompoundButton", "icon": "Document", "title": "Open the release notes", "description": "Opens in your browser", "spacing": "small", "selectAction": {"type": "Action.Submit", "data": {"action": "notes"}}}
        ]
    })
}

fn carousel_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Carousel"},
            {"type": "Carousel", "timer": 4000, "pages": [
                {"type": "CarouselPage", "style": "emphasis", "minHeight": "80px", "verticalContentAlignment": "center", "items": [
                    {"type": "TextBlock", "weight": "bolder", "text": "Page one"},
                    {"type": "TextBlock", "wrap": true, "text": "Use the arrows or the dots."}
                ]},
                {"type": "CarouselPage", "style": "good", "minHeight": "80px", "items": [
                    {"type": "TextBlock", "weight": "bolder", "text": "Page two"},
                    {"type": "Image", "url": CARD_ICON_URL, "size": "small"}
                ]},
                {"type": "CarouselPage", "style": "accent", "minHeight": "80px", "items": [
                    {"type": "TextBlock", "weight": "bolder", "text": "Page three"}
                ]}
            ]}
        ]
    })
}

fn action_overflow_card() -> Value {
    let url = "https://example.com/action";
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Action modes, tooltips and overflow"},
            {"type": "ActionSet", "actions": [
                {"type": "Action.OpenUrl", "title": "Primary", "url": url, "tooltip": "Tooltip on hover"},
                {"type": "Action.OpenUrl", "title": "Positive", "url": url, "style": "positive"},
                {"type": "Action.OpenUrl", "title": "Destructive", "url": url, "style": "destructive"},
                {"type": "Action.OpenUrl", "title": "Disabled", "url": url, "isEnabled": false, "tooltip": "Not available right now"},
                {"type": "Action.OpenUrl", "title": "Secondary A", "url": url, "mode": "secondary"},
                {"type": "Action.OpenUrl", "title": "Secondary B", "url": url, "mode": "secondary"}
            ]}
        ],
        "actions": (1..=8)
            .map(|number| json!({"type": "Action.OpenUrl", "title": format!("Action {number}"), "url": url}))
            .collect::<Vec<_>>()
    })
}

fn dates_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Dates and times"},
            {"type": "TextBlock", "wrap": true, "text": "Short {{DATE(2026-10-09T14:05:00Z, SHORT)}}"},
            {"type": "TextBlock", "wrap": true, "text": "Long {{DATE(2026-10-09T14:05:00Z, LONG)}}"},
            {"type": "TextBlock", "wrap": true, "text": "Compact {{DATE(2026-10-09T14:05:00Z, COMPACT)}} at {{TIME(2026-10-09T14:05:00Z)}}"},
            {"type": "TextBlock", "wrap": true, "text": "Malformed stays literal: {{DATE(tomorrow, SHORT)}}"},
            {"type": "RichTextBlock", "inlines": [{"type": "TextRun", "text": "Run: {{TIME(2026-10-09T09:30:00+02:00)}}"}]},
            {"type": "FactSet", "facts": [{"title": "Due", "value": "{{DATE(2026-10-12T08:00:00Z, SHORT)}}"}]}
        ]
    })
}
