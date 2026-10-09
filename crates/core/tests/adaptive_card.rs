use serde_json::json;
use teams_core::{
    AdaptiveCard, CardAction, CardActionIcon, CardActionKind, CardElement, ColumnWidth,
    ExecuteTrigger, IconSize, ImageSize, Span, TextColor, TextSize, ToggleTarget,
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

    let CardElement::Columns { columns, .. } = &card.items[0].element else {
        panic!("first item is a column set");
    };
    assert_eq!(columns[0].width, ColumnWidth::Auto);
    assert_eq!(columns[0].layout.vertical_alignment, VerticalAlignment::Center);
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
    let CardElement::Columns { columns, .. } = &card.items[1].element else {
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
    let CardActionKind::Submit(submit) = &actions[2].kind else {
        panic!("watch is a submit action");
    };
    assert_eq!(submit.data, json!({"action": "watch"}));
    assert!(actions[2].is_clickable());
}

fn card_actions(actions: serde_json::Value) -> Vec<CardAction> {
    let card = json!({"body": [{"type": "TextBlock", "text": "x"}], "actions": actions});
    AdaptiveCard::parse(&card.to_string()).unwrap().actions
}

#[test]
fn plain_submit_sends_its_data_as_a_message_back() {
    let actions = card_actions(json!([
        {"type": "Action.Submit", "title": "Unwatch", "data": {"type": "unwatch", "page": "p1"}}
    ]));
    let payload = actions[0].invoke_payload().unwrap();
    assert_eq!(payload.name, "messageback");
    assert_eq!(payload.value, json!({"type": "unwatch", "page": "p1"}));
}

#[test]
fn task_fetch_submit_wraps_data_without_the_teams_settings() {
    let actions = card_actions(json!([
        {"type": "Action.Submit", "title": "Bell", "data": {"cardType": "n", "msteams": {"type": "task/fetch"}}}
    ]));
    let payload = actions[0].invoke_payload().unwrap();
    assert_eq!(payload.name, "task/fetch");
    assert_eq!(
        payload.value,
        json!({"data": {"cardType": "n", "type": "task/fetch"}, "context": {"theme": "dark"}})
    );
}

#[test]
fn message_back_and_im_back_send_the_teams_value() {
    let actions = card_actions(json!([
        {"type": "Action.Submit", "title": "A", "data": {"msteams": {"type": "messageBack", "value": {"k": 1}}}},
        {"type": "Action.Submit", "title": "B", "data": {"msteams": {"type": "imBack"}, "k": 2}}
    ]));
    let first = actions[0].invoke_payload().unwrap();
    assert_eq!(
        (first.name.as_str(), first.value),
        ("messageback", json!({"k": 1}))
    );
    let second = actions[1].invoke_payload().unwrap();
    assert_eq!(second.value, json!({"msteams": {"type": "imBack"}, "k": 2}));
}

#[test]
fn execute_action_carries_verb_id_and_data() {
    let actions = card_actions(json!([
        {"type": "Action.Execute", "id": "a1", "title": "Approve", "verb": "approve", "data": {"n": 3}},
        {"type": "Action.Execute", "title": "No verb"}
    ]));
    let payload = actions[0].invoke_payload().unwrap();
    assert_eq!(payload.name, "adaptiveCard/action");
    assert_eq!(
        payload.value,
        json!({
            "action": {"type": "Action.Execute", "id": "a1", "verb": "approve", "data": {"n": 3}},
            "trigger": "manual"
        })
    );
    assert_eq!(actions[1].kind, CardActionKind::Unsupported);
}

#[test]
fn show_card_and_toggle_actions_are_parsed_locally() {
    let actions = card_actions(json!([
        {"type": "Action.ShowCard", "title": "More", "card": {"body": [{"type": "TextBlock", "text": "nested"}]}},
        {"type": "Action.ToggleVisibility", "title": "Toggle", "targetElements": ["a", {"elementId": "b", "isVisible": true}]},
        {"type": "Action.ToggleVisibility", "title": "Empty", "targetElements": []}
    ]));
    let CardActionKind::ShowCard(nested) = &actions[0].kind else {
        panic!("show card expected");
    };
    assert_eq!(nested.plain_text().as_deref(), Some("nested"));
    assert!(actions[0].invoke_payload().is_none());
    assert_eq!(
        actions[1].kind,
        CardActionKind::ToggleVisibility(vec![
            ToggleTarget {
                element_id: "a".to_owned(),
                visible: None
            },
            ToggleTarget {
                element_id: "b".to_owned(),
                visible: Some(true)
            },
        ])
    );
    assert_eq!(actions[2].kind, CardActionKind::Unsupported);
}

#[test]
fn disabled_actions_stay_disabled_and_hidden_ones_need_an_id() {
    let actions = card_actions(json!([
        {"type": "Action.Submit", "title": "Off", "isEnabled": false, "data": {}},
        {"type": "Action.Submit", "title": "Gone", "isVisible": false},
        {"type": "Action.Submit", "title": "Later", "id": "later", "isVisible": false}
    ]));
    assert_eq!(actions.len(), 2);
    assert!(!actions[0].is_clickable());
    assert!(!actions[1].visible);
    assert_eq!(actions[1].id.as_deref(), Some("later"));
}

#[test]
fn toggled_items_keep_their_id_and_stay_out_of_the_text() {
    let card = json!({"body": [
        {"type": "TextBlock", "text": "shown"},
        {"type": "TextBlock", "id": "details", "isVisible": false, "text": "secret"}
    ]})
    .to_string();
    let parsed = AdaptiveCard::parse(&card).unwrap();
    assert_eq!(parsed.items.len(), 2);
    assert_eq!(parsed.items[1].id.as_deref(), Some("details"));
    assert!(!parsed.items[1].visible);
    assert_eq!(parsed.plain_text().as_deref(), Some("shown"));
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

#[test]
fn element_visibility_covers_items_actions_and_nested_cards() {
    let card = json!({
        "body": [
            {"type": "TextBlock", "id": "title", "text": "t"},
            {"type": "Container", "items": [
                {"type": "TextBlock", "id": "details", "isVisible": false, "text": "d"}
            ]},
            {"type": "ActionSet", "actions": [
                {"type": "Action.Submit", "id": "later", "isVisible": false, "title": "Later"},
                {"type": "Action.ShowCard", "title": "More", "card": {"body": [
                    {"type": "TextBlock", "id": "nested", "isVisible": false, "text": "n"}
                ]}}
            ]}
        ]
    })
    .to_string();
    let visibility = AdaptiveCard::parse(&card).unwrap().element_visibility();
    assert_eq!(visibility.len(), 4);
    assert!(visibility["title"]);
    assert!(!visibility["details"]);
    assert!(!visibility["later"]);
    assert!(!visibility["nested"]);
}

fn parse(card: serde_json::Value) -> AdaptiveCard {
    AdaptiveCard::parse(&card.to_string()).unwrap()
}

fn refresh_card(user_ids: Option<serde_json::Value>) -> AdaptiveCard {
    let mut refresh =
        json!({"action": {"type": "Action.Execute", "verb": "refreshView", "data": {"k": 1}}});
    if let Some(user_ids) = user_ids {
        refresh["userIds"] = user_ids;
    }
    parse(json!({"body": [{"type": "TextBlock", "text": "x"}], "refresh": refresh}))
}

#[test]
fn refresh_parses_the_execute_action_and_user_ids() {
    let card = refresh_card(Some(json!(["8:orgid:ABC"])));
    let refresh = card.refresh.unwrap();
    assert_eq!(refresh.action.verb, "refreshView");
    assert_eq!(refresh.user_ids, Some(vec!["8:orgid:ABC".to_owned()]));
    assert!(parse(json!({"body": [{"type": "TextBlock", "text": "x"}], "refresh": {"action": {"type": "Action.OpenUrl", "url": "https://a.example"}}})).refresh.is_none());
}

#[test]
fn refresh_runs_for_small_chats_without_user_ids() {
    let refresh = refresh_card(None).refresh.unwrap();
    assert!(refresh.runs_automatically(Some(2), None));
    assert!(refresh.runs_automatically(Some(60), Some("8:orgid:me")));
    assert!(!refresh.runs_automatically(Some(61), Some("8:orgid:me")));
    assert!(!refresh.runs_automatically(None, Some("8:orgid:me")));
}

#[test]
fn refresh_with_user_ids_runs_only_for_listed_users() {
    let refresh = refresh_card(Some(json!(["8:orgid:ABC-1", "8:orgid:other"])))
        .refresh
        .unwrap();
    assert!(refresh.runs_automatically(Some(500), Some("abc-1")));
    assert!(refresh.runs_automatically(Some(500), Some("8:orgid:abc-1")));
    assert!(!refresh.runs_automatically(Some(2), Some("8:orgid:nobody")));
    assert!(!refresh.runs_automatically(Some(2), None));
}

#[test]
fn execute_payload_carries_the_trigger() {
    let refresh = refresh_card(None).refresh.unwrap();
    assert_eq!(refresh.action.payload().value["trigger"], "manual");
    let automatic = refresh.action.payload_for(ExecuteTrigger::Automatic);
    assert_eq!(automatic.name, "adaptiveCard/action");
    assert_eq!(automatic.value["trigger"], "automatic");
}

#[test]
fn tab_info_and_sign_in_submits_open_a_url() {
    let actions = card_actions(json!([
        {"type": "Action.Submit", "title": "Tab", "data": {"msteams": {"type": "invoke", "value": {"type": "tab/tabInfoAction", "tabInfo": {"contentUrl": "https://content.example/c", "websiteUrl": "https://site.example/w", "name": "n", "entityId": "e"}}}}},
        {"type": "Action.Submit", "title": "Tab content", "data": {"msteams": {"type": "invoke", "value": {"type": "tab/tabInfoAction", "tabInfo": {"contentUrl": "https://content.example/c"}}}}},
        {"type": "Action.Submit", "title": "Tab bad", "data": {"msteams": {"type": "invoke", "value": {"type": "tab/tabInfoAction", "tabInfo": {"websiteUrl": "javascript:x"}}}}},
        {"type": "Action.Submit", "title": "Sign in", "data": {"msteams": {"type": "signin", "value": "https://login.example/start"}}}
    ]));
    assert_eq!(
        actions[0].kind,
        CardActionKind::OpenUrl("https://site.example/w".into())
    );
    assert_eq!(
        actions[1].kind,
        CardActionKind::OpenUrl("https://content.example/c".into())
    );
    assert_eq!(actions[2].kind, CardActionKind::Unsupported);
    assert_eq!(
        actions[3].kind,
        CardActionKind::OpenUrl("https://login.example/start".into())
    );
}

#[test]
fn other_invoke_submits_send_the_value_named_by_its_type() {
    let actions = card_actions(json!([
        {"type": "Action.Submit", "title": "Go", "data": {"msteams": {"type": "invoke", "value": {"type": "custom/thing", "payload": 1}}}}
    ]));
    let payload = actions[0].invoke_payload().unwrap();
    assert_eq!(payload.name, "custom/thing");
    assert_eq!(payload.value, json!({"type": "custom/thing", "payload": 1}));
}

#[test]
fn add_app_submits_stay_as_unavailable_buttons() {
    let actions = card_actions(json!([
        {"type": "Action.Submit", "title": "Add", "data": {"msteams": {"type": "invoke", "value": {"type": "appInstallToConversation"}}}},
        {"type": "Action.Submit", "title": "Add 2", "data": {"msteams": {"type": "addAppToConversation"}}}
    ]));
    for action in &actions {
        assert_eq!(
            action.kind,
            CardActionKind::Unavailable("Adding apps is not supported")
        );
        assert!(!action.is_clickable());
    }
}

#[test]
fn select_actions_are_parsed_on_card_image_container_and_column() {
    let select = json!({"type": "Action.OpenUrl", "url": "https://a.example"});
    let card = parse(json!({
        "selectAction": select,
        "body": [
            {"type": "Image", "url": "https://img.example/a.png", "selectAction": {"type": "Action.Submit", "data": {"a": 1}}},
            {"type": "Container", "selectAction": {"type": "Action.ToggleVisibility", "targetElements": ["x"]}, "items": [{"type": "TextBlock", "text": "t"}]},
            {"type": "ColumnSet", "columns": [{"type": "Column", "selectAction": {"type": "Action.Execute", "verb": "v"}, "items": [{"type": "TextBlock", "text": "t"}]}]},
            {"type": "Image", "url": "https://img.example/b.png", "selectAction": {"type": "Action.ShowCard", "card": {"body": [{"type": "TextBlock", "text": "t"}]}}}
        ]
    }));
    assert!(matches!(
        card.select_action.unwrap().kind,
        CardActionKind::OpenUrl(_)
    ));
    let CardElement::Image(image) = &card.items[0].element else {
        panic!("image")
    };
    assert!(matches!(
        image.select_action.as_ref().unwrap().kind,
        CardActionKind::Submit(_)
    ));
    let CardElement::Container { select_action, .. } = &card.items[1].element else {
        panic!("container")
    };
    assert!(matches!(
        select_action.as_ref().unwrap().kind,
        CardActionKind::ToggleVisibility(_)
    ));
    let CardElement::Columns { columns, .. } = &card.items[2].element else {
        panic!("columns")
    };
    assert!(matches!(
        columns[0].select_action.as_ref().unwrap().kind,
        CardActionKind::Execute(_)
    ));
    let CardElement::Image(image) = &card.items[3].element else {
        panic!("image")
    };
    assert!(image.select_action.is_none());
}

#[test]
fn media_takes_the_first_web_source_and_poster() {
    let card = parse(json!({"body": [
        {"type": "Media", "poster": "https://img.example/p.png", "altText": "Intro", "sources": [
            {"mimeType": "video/mp4", "url": "ftp://x"}, {"mimeType": "video/mp4", "url": "https://v.example/a.mp4"}, {"url": "https://v.example/b.mp4"}
        ]},
        {"type": "Media", "sources": []},
        {"type": "TextBlock", "text": "after"}
    ]}));
    assert_eq!(card.items.len(), 2);
    let CardElement::Media(media) = &card.items[0].element else {
        panic!("media")
    };
    assert_eq!(media.source_url, "https://v.example/a.mp4");
    assert_eq!(
        media.poster_url.as_deref(),
        Some("https://img.example/p.png")
    );
    assert_eq!(media.alt_text.as_deref(), Some("Intro"));
}

#[test]
fn icon_elements_and_action_icons_are_parsed() {
    let card = parse(json!({
        "body": [
            {"type": "Icon", "name": "Calendar", "size": "xxLarge", "color": "accent", "style": "Filled", "selectAction": {"type": "Action.OpenUrl", "url": "https://a.example"}},
            {"type": "Icon", "name": "Mail"}
        ],
        "actions": [
            {"type": "Action.OpenUrl", "title": "A", "url": "https://a.example", "iconUrl": "icon:Send,Filled"},
            {"type": "Action.OpenUrl", "title": "B", "url": "https://a.example", "iconUrl": "https://img.example/i.png"},
            {"type": "Action.OpenUrl", "title": "C", "url": "https://a.example", "iconUrl": "data:image/png;base64,AA"}
        ]
    }));
    let CardElement::Icon(icon) = &card.items[0].element else {
        panic!("icon")
    };
    assert_eq!(
        (icon.name.as_str(), icon.size, icon.color, icon.filled),
        (
            "Calendar",
            IconSize::ExtraExtraLarge,
            TextColor::Accent,
            true
        )
    );
    assert!(icon.select_action.is_some());
    let CardElement::Icon(plain) = &card.items[1].element else {
        panic!("icon")
    };
    assert_eq!((plain.size, plain.filled), (IconSize::Standard, false));
    assert_eq!(
        card.actions[0].icon,
        Some(CardActionIcon::Named("Send".into()))
    );
    assert_eq!(
        card.actions[1].icon,
        Some(CardActionIcon::Url("https://img.example/i.png".into()))
    );
    assert_eq!(card.actions[2].icon, None);
}

#[test]
fn mentions_and_italic_survive_in_text_fact_and_rich_text() {
    let card = parse(json!({"body": [
        {"type": "TextBlock", "text": "Hi <at>Ada</at>, this is _soft_ and snake_case_x"},
        {"type": "FactSet", "facts": [{"title": "Owner", "value": "<at>Bob</at>"}]},
        {"type": "RichTextBlock", "inlines": [{"type": "TextRun", "text": "cc <at>Cy</at>", "weight": "bolder"}]}
    ]}));
    let CardElement::Text(text) = &card.items[0].element else {
        panic!("text")
    };
    assert!(text.spans.contains(&Span::Mention {
        name: "Ada".into(),
        id: None
    }));
    assert!(
        text.spans
            .contains(&Span::Italic(vec![Span::Text("soft".into())]))
    );
    let CardElement::Facts(facts) = &card.items[1].element else {
        panic!("facts")
    };
    assert_eq!(
        facts[0].value,
        vec![Span::Mention {
            name: "Bob".into(),
            id: None
        }]
    );
    let CardElement::Text(rich) = &card.items[2].element else {
        panic!("rich")
    };
    assert_eq!(
        rich.spans,
        vec![Span::Bold(vec![
            Span::Text("cc ".into()),
            Span::Mention {
                name: "Cy".into(),
                id: None
            }
        ])]
    );
}
