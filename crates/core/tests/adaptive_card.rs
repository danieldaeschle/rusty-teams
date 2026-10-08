use serde_json::json;
use teams_core::{
    AdaptiveCard, CardActionKind, CardElement, ColumnWidth, ImageSize, Span, TextColor, TextSize,
    VerticalAlignment, card_content_text,
};

fn digest_card() -> String {
    json!({
        "type": "AdaptiveCard",
        "msTeams": {"width": "Full"},
        "body": [
            {"type": "ColumnSet", "columns": [
                {"type": "Column", "width": "auto", "verticalContentAlignment": "center", "items": [
                    {"type": "Image", "url": "https://img.example.com/person.png", "size": "small", "style": "person", "height": "32px"}
                ]},
                {"type": "Column", "width": "stretch", "items": [
                    {"type": "TextBlock", "size": "medium", "weight": "bolder", "wrap": true, "text": "Ada Example edited your page"}
                ]}
            ]},
            {"type": "ColumnSet", "separator": true, "columns": [
                {"type": "Column", "width": "40px", "items": [
                    {"type": "Image", "url": "https://img.example.com/page.png", "width": "24px", "height": "24px"}
                ]},
                {"type": "Column", "width": 2, "items": [
                    {"type": "TextBlock", "weight": "bolder", "wrap": true, "color": "accent", "text": "[**Quarterly Plan**](https://example.com/wiki/plan) in [**Product Space**](https://example.com/wiki/space)"},
                    {"type": "TextBlock", "spacing": "small", "isSubtle": true, "text": "Owned by: Ada Example"}
                ]}
            ]},
            {"type": "ActionSet", "actions": [
                {"type": "Action.OpenUrl", "title": "View page", "url": "https://example.com/wiki/plan"},
                {"type": "Action.OpenUrl", "title": "Unsafe", "url": "javascript:alert(1)"},
                {"type": "Action.Submit", "title": "Watch", "data": {"action": "watch"}},
                {"type": "Action.Submit", "title": "Hidden", "isVisible": false}
            ]}
        ]
    })
    .to_string()
}

#[test]
fn digest_card_keeps_layout_text_and_actions() {
    let card = AdaptiveCard::parse(&digest_card()).unwrap();
    assert!(card.full_width);
    assert_eq!(card.items.len(), 3);

    let CardElement::Columns(columns) = &card.items[0].element else {
        panic!("first item is a column set");
    };
    assert_eq!(columns[0].width, ColumnWidth::Auto);
    assert_eq!(columns[0].vertical_alignment, VerticalAlignment::Center);
    let CardElement::Image(image) = &columns[0].items[0].element else {
        panic!("avatar column holds an image");
    };
    assert!(image.person);
    assert_eq!(image.size, ImageSize::Small);
    assert_eq!(image.height, Some(32.));
    let CardElement::Text(title) = &columns[1].items[0].element else {
        panic!("title column holds text");
    };
    assert!(title.bold && title.wrap);
    assert_eq!(title.size, TextSize::Medium);

    assert!(card.items[1].separator);
    let CardElement::Columns(columns) = &card.items[1].element else {
        panic!("second item is a column set");
    };
    assert_eq!(columns[0].width, ColumnWidth::Pixels(40.));
    assert_eq!(columns[1].width, ColumnWidth::Weighted(2.));
    let CardElement::Text(line) = &columns[1].items[0].element else {
        panic!("text expected");
    };
    assert_eq!(line.color, TextColor::Accent);
    assert_eq!(
        line.spans[0],
        Span::Link {
            url: "https://example.com/wiki/plan".to_owned(),
            children: vec![Span::Bold(vec![Span::Text("Quarterly Plan".to_owned())])],
        }
    );
    let CardElement::Text(owner) = &columns[1].items[1].element else {
        panic!("text expected");
    };
    assert!(owner.subtle);

    let CardElement::Actions(actions) = &card.items[2].element else {
        panic!("third item is an action set");
    };
    assert_eq!(actions.len(), 3);
    assert_eq!(
        actions[0].kind,
        CardActionKind::OpenUrl("https://example.com/wiki/plan".to_owned())
    );
    assert_eq!(actions[1].kind, CardActionKind::Unsupported);
    assert_eq!(actions[2].title, "Watch");
    assert_eq!(actions[2].kind, CardActionKind::Unsupported);
}

#[test]
fn plain_text_drops_urls_images_and_buttons() {
    assert_eq!(
        card_content_text(&digest_card()).unwrap(),
        "Ada Example edited your page\nQuarterly Plan in Product Space\nOwned by: Ada Example"
    );
}

#[test]
fn hidden_and_unknown_elements_are_skipped_but_fallback_text_stays() {
    let card = json!({
        "body": [
            {"type": "TextBlock", "text": "hidden", "isVisible": false},
            {"type": "Carousel", "fallbackText": "Open the carousel elsewhere"},
            {"type": "Carousel"},
            {"type": "Image", "url": "http://insecure.example.com/a.png"},
            {"type": "TextBlock", "text": "shown"}
        ]
    })
    .to_string();
    assert_eq!(
        card_content_text(&card).unwrap(),
        "Open the carousel elsewhere\nshown"
    );
}

#[test]
fn containers_fact_sets_and_rich_text_are_parsed() {
    let card = json!({
        "body": [
            {"type": "Container", "style": "emphasis", "items": [
                {"type": "RichTextBlock", "inlines": [
                    "plain ",
                    {"type": "TextRun", "text": "bold", "weight": "bolder"},
                    {"type": "TextRun", "text": " link", "selectAction": {"type": "Action.OpenUrl", "url": "https://example.com/x"}}
                ]},
                {"type": "FactSet", "facts": [{"title": "Env", "value": "prod"}]},
                {"type": "ImageSet", "imageSize": "medium", "images": [
                    {"type": "Image", "url": "https://img.example.com/a.png"}
                ]}
            ]}
        ]
    })
    .to_string();
    assert_eq!(
        card_content_text(&card).unwrap(),
        "plain bold link\nEnv: prod"
    );
}

#[test]
fn cards_without_content_are_none() {
    assert!(AdaptiveCard::parse("not json").is_none());
    assert!(AdaptiveCard::parse(r#"{"body": []}"#).is_none());
    assert!(AdaptiveCard::parse("[1]").is_none());
}
