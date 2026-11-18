//! Cards without an Adaptive Card `body`: Office 365 connector `MessageCard`s (incoming
//! webhooks, legacy workflows) and hero or thumbnail cards. Teams draws them with title,
//! sections, facts and buttons; they are rewritten into an Adaptive Card here.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

/// Set on a converted `TextBlock` or fact whose text is HTML, which connector cards allow
/// besides markdown. The card parser then reads it as HTML instead of escaping the tags.
pub(crate) const HTML_TEXT: &str = "x-legacyHtml";

static HTML_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?[a-zA-Z][a-zA-Z0-9]*(\s[^<>]*)?/?>").unwrap());

const OPEN_URI: &str = "OpenUri";
const VIEW_ACTION: &str = "ViewAction";
const OPEN_URL_BUTTON: &str = "openUrl";
const DEFAULT_OS: &str = "default";

/// The Adaptive Card that shows `root`, or `None` when it is no legacy card.
pub(crate) fn as_adaptive_card(root: &Value) -> Option<Value> {
    let object = root.as_object()?;
    if object.contains_key("body") || text(root, "type") == Some("AdaptiveCard") {
        return None;
    }
    let connector = text(root, "@type") == Some("MessageCard")
        || ["sections", "potentialAction"]
            .iter()
            .any(|key| root.get(key).is_some_and(Value::is_array));
    let hero = ["buttons", "images"]
        .iter()
        .any(|key| root.get(key).is_some_and(Value::is_array));
    if !connector && !hero {
        return None;
    }
    let mut body = Vec::new();
    if let Some(title) = text(root, "title") {
        body.push(json!({"type": "TextBlock", "text": title, "weight": "Bolder", "size": "Medium", "wrap": true}));
    }
    if let Some(subtitle) = text(root, "subtitle") {
        body.push(json!({"type": "TextBlock", "text": subtitle, "isSubtle": true, "wrap": true}));
    }
    if let Some(text) = text(root, "text") {
        body.push(json!({"type": "TextBlock", "text": text, "wrap": true}));
    }
    for image in root
        .get("images")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(url) = text(image, "url").or_else(|| text(image, "image")) {
            body.push(json!({"type": "Image", "url": url}));
        }
    }
    for section in root
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        body.extend(section_items(section));
    }
    if body.is_empty() {
        if let Some(summary) = text(root, "summary") {
            body.push(json!({"type": "TextBlock", "text": summary, "wrap": true}));
        }
    }
    let actions: Vec<Value> = root
        .get("potentialAction")
        .or_else(|| root.get("buttons"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(open_url_action)
        .collect();
    if body.is_empty() && actions.is_empty() {
        return None;
    }
    let mut card =
        json!({"type": "AdaptiveCard", "version": "1.4", "body": body, "actions": actions});
    mark_html_text(&mut card);
    Some(card)
}

fn mark_html_text(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(mark_html_text),
        Value::Object(object) => {
            let html_in = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|text| HTML_TAG.is_match(text))
            };
            if html_in("text") || html_in("value") {
                object.insert(HTML_TEXT.to_owned(), Value::Bool(true));
            }
            object.values_mut().for_each(mark_html_text);
        }
        _ => {}
    }
}

fn section_items(section: &Value) -> Vec<Value> {
    let mut items = Vec::new();
    let separator = section
        .get("startGroup")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(title) = text(section, "title") {
        items.push(json!({"type": "TextBlock", "text": title, "weight": "Bolder", "wrap": true}));
    }
    let mut activity = Vec::new();
    if let Some(title) = text(section, "activityTitle") {
        activity
            .push(json!({"type": "TextBlock", "text": title, "weight": "Bolder", "wrap": true}));
    }
    if let Some(subtitle) = text(section, "activitySubtitle") {
        activity.push(json!({"type": "TextBlock", "text": subtitle, "isSubtle": true, "spacing": "None", "wrap": true}));
    }
    if let Some(body) = text(section, "activityText") {
        activity.push(json!({"type": "TextBlock", "text": body, "wrap": true}));
    }
    match (text(section, "activityImage"), activity.is_empty()) {
        (Some(image), false) => items.push(json!({
            "type": "ColumnSet",
            "columns": [
                {"type": "Column", "width": "auto", "items": [{"type": "Image", "url": image, "size": "Small", "style": "Person"}]},
                {"type": "Column", "width": "stretch", "items": activity},
            ],
        })),
        _ => items.extend(activity),
    }
    if let Some(body) = text(section, "text") {
        items.push(json!({"type": "TextBlock", "text": body, "wrap": true}));
    }
    let facts: Vec<Value> = section
        .get("facts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|fact| {
            Some(json!({"title": text(fact, "name").unwrap_or_default(), "value": text(fact, "value")?}))
        })
        .collect();
    if !facts.is_empty() {
        items.push(json!({"type": "FactSet", "facts": facts}));
    }
    for image in section
        .get("images")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(url) = text(image, "image").or_else(|| text(image, "url")) {
            items.push(json!({"type": "Image", "url": url}));
        }
    }
    if separator && let Some(first) = items.first_mut() {
        first["separator"] = json!(true);
    }
    items
}

fn open_url_action(action: &Value) -> Option<Value> {
    let title = text(action, "name").or_else(|| text(action, "title"))?;
    let kind = text(action, "@type").or_else(|| text(action, "type"))?;
    let url = match kind {
        OPEN_URI => {
            let targets = action.get("targets")?.as_array()?;
            targets
                .iter()
                .find(|target| text(target, "os") == Some(DEFAULT_OS))
                .or_else(|| targets.first())
                .and_then(|target| text(target, "uri"))?
        }
        VIEW_ACTION => action.get("target")?.as_array()?.first()?.as_str()?,
        OPEN_URL_BUTTON => text(action, "value")?,
        _ => return None,
    };
    Some(json!({"type": "Action.OpenUrl", "title": title, "url": url}))
}

fn text<'value>(value: &'value Value, key: &str) -> Option<&'value str> {
    value
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::adaptive_card::AdaptiveCard;

    const WEBHOOK: &str = r#"{
        "@type": "MessageCard",
        "summary": "Build failed",
        "title": "Pipeline main",
        "sections": [{
            "activityTitle": "Build 128 failed",
            "activitySubtitle": "on agent 3",
            "facts": [{"name": "Branch", "value": "main"}, {"name": "Duration", "value": "4 min"}],
            "text": "See the log for details."
        }],
        "potentialAction": [
            {"@type": "OpenUri", "name": "Open log", "targets": [{"os": "default", "uri": "https://ci.example/128"}]},
            {"@type": "HttpPOST", "name": "Retry", "target": "https://ci.example/retry"}
        ]
    }"#;

    #[test]
    fn a_connector_message_card_shows_title_sections_facts_and_links() {
        let card = AdaptiveCard::parse(WEBHOOK).expect("connector card is shown");
        let text = card.plain_text().unwrap_or_default();
        for expected in [
            "Pipeline main",
            "Build 128 failed",
            "on agent 3",
            "See the log",
            "Branch",
            "main",
        ] {
            assert!(text.contains(expected), "{expected} missing in {text:?}");
        }
        let titles: Vec<_> = card
            .actions
            .iter()
            .map(|action| action.title.as_str())
            .collect();
        assert_eq!(titles, ["Open log"]);
    }

    #[test]
    fn a_hero_card_shows_its_text_and_buttons() {
        let hero = r#"{"title": "Release 1.2", "text": "Ready", "buttons": [{"type": "openUrl", "title": "Notes", "value": "https://example.com/notes"}]}"#;
        let card = AdaptiveCard::parse(hero).expect("hero card is shown");
        assert!(
            card.plain_text()
                .unwrap_or_default()
                .contains("Release 1.2")
        );
        assert_eq!(card.actions.len(), 1);
    }

    #[test]
    fn html_in_connector_text_and_facts_is_read_as_html() {
        let html_card = r#"{"@type": "MessageCard", "text": "Deploy <b>done</b><br>see <a href=\"https://example.com/run\">run</a>", "sections": [{"facts": [{"name": "Status", "value": "<i>green</i>"}]}]}"#;
        let text = AdaptiveCard::parse(html_card)
            .expect("connector card is shown")
            .plain_text()
            .unwrap_or_default();
        assert!(text.contains("done") && text.contains("green"));
        assert!(!text.contains('<'), "tags leaked: {text}");
    }

    #[test]
    fn an_object_without_card_shape_stays_no_card() {
        assert!(AdaptiveCard::parse("{}").is_none());
        assert!(AdaptiveCard::parse(r#"{"title": "t", "value": "v"}"#).is_none());
    }
}
