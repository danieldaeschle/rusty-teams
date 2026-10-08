use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::card_inputs::{CardInput, merge_input_data, parse_input};
use crate::markdown::markdown_to_html;
use crate::spans::{Span, html_to_spans};
use crate::stored::push_plain;

const SUPPORTED_URL_SCHEMES: [&str; 2] = ["https://", "http://"];
const IMAGE_URL_SCHEME: &str = "https://";
const ACTION_TYPE_PREFIX: &str = "Action.";
const INPUT_TYPE_PREFIX: &str = "Input.";
const OPEN_URL_ACTION: &str = "Action.OpenUrl";
const SUBMIT_ACTION: &str = "Action.Submit";
const EXECUTE_ACTION: &str = "Action.Execute";
const SHOW_CARD_ACTION: &str = "Action.ShowCard";
const TOGGLE_VISIBILITY_ACTION: &str = "Action.ToggleVisibility";
const TEAMS_SETTINGS_KEY: &str = "msteams";
pub const CARD_THEME: &str = "dark";
const MESSAGE_BACK_NAME: &str = "messageback";
const TASK_FETCH_NAME: &str = "task/fetch";
const EXECUTE_NAME: &str = "adaptiveCard/action";
const PIXEL_SUFFIX: &str = "px";

#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveCard {
    pub full_width: bool,
    pub items: Vec<CardItem>,
    pub actions: Vec<CardAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardItem {
    pub id: Option<String>,
    pub visible: bool,
    pub separator: bool,
    pub spacing: CardSpacing,
    pub element: CardElement,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CardElement {
    Text(CardText),
    Image(CardImage),
    ImageSet(Vec<CardImage>),
    Columns(Vec<CardColumn>),
    Container {
        style: ContainerStyle,
        items: Vec<CardItem>,
    },
    Facts(Vec<CardFact>),
    Actions(Vec<CardAction>),
    Input(CardInput),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardSpacing {
    None,
    Small,
    Default,
    Medium,
    Large,
    ExtraLarge,
    Padding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSize {
    Small,
    Default,
    Medium,
    Large,
    ExtraLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextColor {
    Default,
    Dark,
    Light,
    Accent,
    Good,
    Warning,
    Attention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSize {
    Auto,
    Stretch,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerStyle {
    Default,
    Emphasis,
    Accent,
    Good,
    Warning,
    Attention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnWidth {
    Auto,
    Stretch,
    Weighted(f32),
    Pixels(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardText {
    pub spans: Vec<Span>,
    pub size: TextSize,
    pub bold: bool,
    pub color: TextColor,
    pub subtle: bool,
    pub wrap: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardImage {
    pub url: String,
    pub size: ImageSize,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub person: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardColumn {
    pub width: ColumnWidth,
    pub vertical_alignment: VerticalAlignment,
    pub items: Vec<CardItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardFact {
    pub title: String,
    pub value: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardAction {
    pub id: Option<String>,
    pub title: String,
    pub kind: CardActionKind,
    pub enabled: bool,
    pub visible: bool,
    pub collects_inputs: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CardActionKind {
    OpenUrl(String),
    Submit(SubmitAction),
    Execute(ExecuteAction),
    ShowCard(Box<AdaptiveCard>),
    ToggleVisibility(Vec<ToggleTarget>),
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitAction {
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteAction {
    pub id: Option<String>,
    pub verb: String,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleTarget {
    pub element_id: String,
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvokePayload {
    pub name: &'static str,
    pub value: Value,
}

impl SubmitAction {
    pub fn with_inputs(&self, values: &Map<String, Value>) -> SubmitAction {
        let mut data = merge_input_data(&self.data, values);
        if !values.is_empty() && self.is_message_back() {
            let mut sent = data.as_object().cloned().unwrap_or_default();
            sent.remove(TEAMS_SETTINGS_KEY);
            if let Some(settings) = data
                .get_mut(TEAMS_SETTINGS_KEY)
                .and_then(Value::as_object_mut)
            {
                settings.insert("value".to_owned(), Value::Object(sent));
            }
        }
        SubmitAction { data }
    }

    fn is_message_back(&self) -> bool {
        self.teams_settings()
            .and_then(|settings| string_field(settings, "type"))
            .is_some_and(|kind| kind.eq_ignore_ascii_case("messageBack"))
    }

    fn teams_settings(&self) -> Option<&Value> {
        self.data.get(TEAMS_SETTINGS_KEY)
    }

    pub fn is_task_fetch(&self) -> bool {
        self.teams_settings()
            .and_then(|settings| string_field(settings, "type"))
            .is_some_and(|kind| kind == TASK_FETCH_NAME)
    }

    pub fn payload(&self) -> InvokePayload {
        if self.is_task_fetch() {
            let mut data = self.data.as_object().cloned().unwrap_or_default();
            data.remove(TEAMS_SETTINGS_KEY);
            data.insert("type".to_owned(), json!(TASK_FETCH_NAME));
            return InvokePayload {
                name: TASK_FETCH_NAME,
                value: task_value(Value::Object(data)),
            };
        }
        let message_back = self
            .teams_settings()
            .filter(|settings| {
                string_field(settings, "type").is_some_and(|kind| {
                    kind.eq_ignore_ascii_case("messageBack") || kind.eq_ignore_ascii_case("imBack")
                })
            })
            .and_then(|settings| settings.get("value"));
        InvokePayload {
            name: MESSAGE_BACK_NAME,
            value: message_back.unwrap_or(&self.data).clone(),
        }
    }
}

impl ExecuteAction {
    pub fn payload(&self) -> InvokePayload {
        let mut action = Map::new();
        action.insert("type".to_owned(), json!(EXECUTE_ACTION));
        if let Some(id) = &self.id {
            action.insert("id".to_owned(), json!(id));
        }
        action.insert("verb".to_owned(), json!(self.verb));
        action.insert("data".to_owned(), self.data.clone());
        InvokePayload {
            name: EXECUTE_NAME,
            value: json!({"action": action, "trigger": "manual"}),
        }
    }
}

pub fn task_value(data: Value) -> Value {
    json!({"data": data, "context": {"theme": CARD_THEME}})
}

impl CardAction {
    pub fn with_inputs(&self, values: &Map<String, Value>) -> CardAction {
        let kind = match &self.kind {
            CardActionKind::Submit(submit) => CardActionKind::Submit(submit.with_inputs(values)),
            CardActionKind::Execute(execute) => {
                CardActionKind::Execute(ExecuteAction {
                    data: merge_input_data(&execute.data, values),
                    ..execute.clone()
                })
            }
            other => other.clone(),
        };
        CardAction {
            kind,
            ..self.clone()
        }
    }

    pub fn invoke_payload(&self) -> Option<InvokePayload> {
        match &self.kind {
            CardActionKind::Submit(submit) => Some(submit.payload()),
            CardActionKind::Execute(execute) => Some(execute.payload()),
            _ => None,
        }
    }

    pub fn is_clickable(&self) -> bool {
        self.enabled && !matches!(self.kind, CardActionKind::Unsupported)
    }
}

impl AdaptiveCard {
    pub fn parse(content: &str) -> Option<AdaptiveCard> {
        Self::from_value(&serde_json::from_str(content).ok()?)
    }

    fn from_value(root: &Value) -> Option<AdaptiveCard> {
        let object = root.as_object()?;
        let mut items = parse_items(object.get("body"));
        let actions = parse_actions(object.get("actions"));
        if items.is_empty()
            && let Some(item) = fallback_item(root)
        {
            items.push(item);
        }
        if items.is_empty() && actions.is_empty() {
            return None;
        }
        let full_width = object
            .get("msTeams")
            .and_then(|settings| string_field(settings, "width"))
            .is_some_and(|width| width.eq_ignore_ascii_case("full"));
        Some(AdaptiveCard {
            full_width,
            items,
            actions,
        })
    }

    /// Initial visibility of every element and action that has an id, nested cards included.
    pub fn element_visibility(&self) -> HashMap<String, bool> {
        let mut visibility = HashMap::new();
        collect_visibility(&self.items, &self.actions, &mut visibility);
        visibility
    }

    pub fn own_inputs(&self) -> Vec<CardInput> {
        let mut inputs = Vec::new();
        collect_inputs(&self.items, &mut inputs);
        inputs
    }

    pub fn all_inputs(&self) -> Vec<CardInput> {
        let mut inputs = self.own_inputs();
        for action in self.all_actions() {
            if let CardActionKind::ShowCard(card) = &action.kind {
                inputs.extend(card.all_inputs());
            }
        }
        inputs
    }

    fn all_actions(&self) -> Vec<&CardAction> {
        let mut actions: Vec<&CardAction> = self.actions.iter().collect();
        collect_item_actions(&self.items, &mut actions);
        actions
    }

    pub fn plain_text(&self) -> Option<String> {
        let mut lines = Vec::new();
        collect_lines(&self.items, &mut lines);
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

fn collect_visibility(
    items: &[CardItem],
    actions: &[CardAction],
    visibility: &mut HashMap<String, bool>,
) {
    for item in items {
        if let Some(id) = &item.id {
            visibility.insert(id.clone(), item.visible);
        }
        match &item.element {
            CardElement::Columns(columns) => columns
                .iter()
                .for_each(|column| collect_visibility(&column.items, &[], visibility)),
            CardElement::Container { items, .. } => collect_visibility(items, &[], visibility),
            CardElement::Actions(actions) => collect_visibility(&[], actions, visibility),
            _ => {}
        }
    }
    for action in actions {
        if let Some(id) = &action.id {
            visibility.insert(id.clone(), action.visible);
        }
        if let CardActionKind::ShowCard(card) = &action.kind {
            collect_visibility(&card.items, &card.actions, visibility);
        }
    }
}

fn collect_inputs(items: &[CardItem], inputs: &mut Vec<CardInput>) {
    for item in items {
        match &item.element {
            CardElement::Input(input) => inputs.push(input.clone()),
            CardElement::Columns(columns) => columns
                .iter()
                .for_each(|column| collect_inputs(&column.items, inputs)),
            CardElement::Container { items, .. } => collect_inputs(items, inputs),
            _ => {}
        }
    }
}

fn collect_item_actions<'card>(items: &'card [CardItem], actions: &mut Vec<&'card CardAction>) {
    for item in items {
        match &item.element {
            CardElement::Actions(set) => actions.extend(set),
            CardElement::Columns(columns) => columns
                .iter()
                .for_each(|column| collect_item_actions(&column.items, actions)),
            CardElement::Container { items, .. } => collect_item_actions(items, actions),
            _ => {}
        }
    }
}

pub fn card_content_text(content: &str) -> Option<String> {
    AdaptiveCard::parse(content)?.plain_text()
}

fn collect_lines(items: &[CardItem], lines: &mut Vec<String>) {
    for item in items.iter().filter(|item| item.visible) {
        match &item.element {
            CardElement::Text(text) => push_line(lines, &text.spans),
            CardElement::Columns(columns) => columns
                .iter()
                .for_each(|column| collect_lines(&column.items, lines)),
            CardElement::Container { items, .. } => collect_lines(items, lines),
            CardElement::Facts(facts) => {
                for fact in facts {
                    let mut line = format!("{}: ", fact.title);
                    push_plain(&mut line, &fact.value);
                    lines.push(line.trim().to_owned());
                }
            }
            CardElement::Image(_)
            | CardElement::ImageSet(_)
            | CardElement::Actions(_)
            | CardElement::Input(_) => {}
        }
    }
}

fn push_line(lines: &mut Vec<String>, spans: &[Span]) {
    let mut line = String::new();
    push_plain(&mut line, spans);
    let line = line.trim();
    if !line.is_empty() {
        lines.push(line.to_owned());
    }
}

fn parse_items(value: Option<&Value>) -> Vec<CardItem> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_item)
        .collect()
}

fn parse_item(value: &Value) -> Option<CardItem> {
    let id = string_field(value, "id").map(str::to_owned);
    let visible = is_visible(value);
    if !visible && id.is_none() {
        return None;
    }
    let element = match string_field(value, "type")? {
        "TextBlock" => parse_text_block(value),
        "RichTextBlock" => parse_rich_text_block(value),
        "Image" => parse_image(value).map(CardElement::Image),
        "ImageSet" => parse_image_set(value),
        "ColumnSet" => parse_column_set(value),
        "Container" => parse_container(value),
        "FactSet" => parse_fact_set(value),
        "ActionSet" => parse_action_set(value),
        input_type if input_type.starts_with(INPUT_TYPE_PREFIX) => {
            parse_input(value, input_type).map(CardElement::Input)
        }
        _ => None,
    };
    let element = element.or_else(|| fallback_item(value).map(|item| item.element))?;
    Some(CardItem {
        id,
        visible,
        separator: value
            .get("separator")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        spacing: parse_spacing(string_field(value, "spacing")),
        element,
    })
}

fn fallback_item(value: &Value) -> Option<CardItem> {
    let text = string_field(value, "fallbackText").filter(|text| !text.trim().is_empty())?;
    Some(CardItem {
        id: None,
        visible: true,
        separator: false,
        spacing: CardSpacing::Default,
        element: CardElement::Text(plain_text_block(vec![Span::Text(text.to_owned())])),
    })
}

fn plain_text_block(spans: Vec<Span>) -> CardText {
    CardText {
        spans,
        size: TextSize::Default,
        bold: false,
        color: TextColor::Default,
        subtle: false,
        wrap: true,
    }
}

fn parse_text_block(value: &Value) -> Option<CardElement> {
    let text = string_field(value, "text").filter(|text| !text.trim().is_empty())?;
    Some(CardElement::Text(CardText {
        spans: html_to_spans(&markdown_to_html(text)),
        size: parse_text_size(string_field(value, "size")),
        bold: string_field(value, "weight").is_some_and(is_bold_weight),
        color: parse_text_color(string_field(value, "color")),
        subtle: bool_field(value, "isSubtle"),
        wrap: bool_field(value, "wrap"),
    }))
}

fn parse_rich_text_block(value: &Value) -> Option<CardElement> {
    let spans: Vec<Span> = value
        .get("inlines")?
        .as_array()?
        .iter()
        .filter_map(parse_text_run)
        .collect();
    (!spans.is_empty()).then(|| CardElement::Text(plain_text_block(spans)))
}

fn parse_text_run(value: &Value) -> Option<Span> {
    let text = value
        .as_str()
        .or_else(|| string_field(value, "text"))
        .filter(|text| !text.is_empty())?;
    let mut span = Span::Text(text.to_owned());
    if string_field(value, "weight").is_some_and(is_bold_weight) {
        span = Span::Bold(vec![span]);
    }
    if bool_field(value, "italic") {
        span = Span::Italic(vec![span]);
    }
    if bool_field(value, "strikethrough") {
        span = Span::Strike(vec![span]);
    }
    if bool_field(value, "underline") {
        span = Span::Underline(vec![span]);
    }
    let url = value
        .get("selectAction")
        .filter(|action| string_field(action, "type") == Some(OPEN_URL_ACTION))
        .and_then(|action| string_field(action, "url"))
        .filter(|url| is_supported_url(url));
    Some(match url {
        Some(url) => Span::Link {
            url: url.to_owned(),
            children: vec![span],
        },
        None => span,
    })
}

fn parse_image(value: &Value) -> Option<CardImage> {
    let url = string_field(value, "url").filter(|url| url.starts_with(IMAGE_URL_SCHEME))?;
    Some(CardImage {
        url: url.to_owned(),
        size: parse_image_size(string_field(value, "size")),
        width: pixel_field(value, "width"),
        height: pixel_field(value, "height"),
        person: string_field(value, "style")
            .is_some_and(|style| style.eq_ignore_ascii_case("person")),
    })
}

fn parse_image_set(value: &Value) -> Option<CardElement> {
    let size = parse_image_size(string_field(value, "imageSize"));
    let images: Vec<CardImage> = value
        .get("images")?
        .as_array()?
        .iter()
        .filter(|image| is_visible(image))
        .filter_map(parse_image)
        .map(|image| match image.size {
            ImageSize::Auto => CardImage { size, ..image },
            _ => image,
        })
        .collect();
    (!images.is_empty()).then_some(CardElement::ImageSet(images))
}

fn parse_column_set(value: &Value) -> Option<CardElement> {
    let columns: Vec<CardColumn> = value
        .get("columns")?
        .as_array()?
        .iter()
        .filter(|column| is_visible(column))
        .filter_map(|column| {
            let items = parse_items(column.get("items"));
            (!items.is_empty()).then(|| CardColumn {
                width: parse_column_width(column.get("width")),
                vertical_alignment: parse_vertical_alignment(string_field(
                    column,
                    "verticalContentAlignment",
                )),
                items,
            })
        })
        .collect();
    (!columns.is_empty()).then_some(CardElement::Columns(columns))
}

fn parse_container(value: &Value) -> Option<CardElement> {
    let items = parse_items(value.get("items"));
    (!items.is_empty()).then(|| CardElement::Container {
        style: parse_container_style(string_field(value, "style")),
        items,
    })
}

fn parse_fact_set(value: &Value) -> Option<CardElement> {
    let facts: Vec<CardFact> = value
        .get("facts")?
        .as_array()?
        .iter()
        .filter_map(|fact| {
            let title = string_field(fact, "title")?;
            let text = string_field(fact, "value").unwrap_or_default();
            Some(CardFact {
                title: title.to_owned(),
                value: html_to_spans(&markdown_to_html(text)),
            })
        })
        .collect();
    (!facts.is_empty()).then_some(CardElement::Facts(facts))
}

fn parse_action_set(value: &Value) -> Option<CardElement> {
    let actions = parse_actions(value.get("actions"));
    (!actions.is_empty()).then_some(CardElement::Actions(actions))
}

fn parse_actions(value: Option<&Value>) -> Vec<CardAction> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_action)
        .collect()
}

fn parse_action(value: &Value) -> Option<CardAction> {
    let id = string_field(value, "id").map(str::to_owned);
    let visible = is_visible(value);
    if !visible && id.is_none() {
        return None;
    }
    let action_type = string_field(value, "type")?;
    if !action_type.starts_with(ACTION_TYPE_PREFIX) {
        return None;
    }
    let title = string_field(value, "title")
        .map(str::trim)
        .filter(|title| !title.is_empty())?;
    let data = || value.get("data").cloned().unwrap_or(Value::Null);
    let kind = match action_type {
        OPEN_URL_ACTION => string_field(value, "url")
            .filter(|url| is_supported_url(url))
            .map_or(CardActionKind::Unsupported, |url| {
                CardActionKind::OpenUrl(url.to_owned())
            }),
        SUBMIT_ACTION => CardActionKind::Submit(SubmitAction { data: data() }),
        EXECUTE_ACTION => string_field(value, "verb").map_or(CardActionKind::Unsupported, |verb| {
            CardActionKind::Execute(ExecuteAction {
                id: id.clone(),
                verb: verb.to_owned(),
                data: data(),
            })
        }),
        SHOW_CARD_ACTION => value
            .get("card")
            .and_then(AdaptiveCard::from_value)
            .map_or(CardActionKind::Unsupported, |card| {
                CardActionKind::ShowCard(Box::new(card))
            }),
        TOGGLE_VISIBILITY_ACTION => {
            let targets = parse_toggle_targets(value.get("targetElements"));
            if targets.is_empty() {
                CardActionKind::Unsupported
            } else {
                CardActionKind::ToggleVisibility(targets)
            }
        }
        _ => CardActionKind::Unsupported,
    };
    Some(CardAction {
        id,
        title: title.to_owned(),
        kind,
        enabled: value
            .get("isEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        visible,
        collects_inputs: !string_field(value, "associatedInputs")
            .is_some_and(|mode| mode.eq_ignore_ascii_case("none")),
    })
}

fn parse_toggle_targets(value: Option<&Value>) -> Vec<ToggleTarget> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|target| match target.as_str() {
            Some(element_id) => Some(ToggleTarget {
                element_id: element_id.to_owned(),
                visible: None,
            }),
            None => Some(ToggleTarget {
                element_id: string_field(target, "elementId")?.to_owned(),
                visible: target.get("isVisible").and_then(Value::as_bool),
            }),
        })
        .collect()
}

fn is_bold_weight(weight: &str) -> bool {
    weight.eq_ignore_ascii_case("bolder") || weight.eq_ignore_ascii_case("bold")
}

fn is_supported_url(url: &str) -> bool {
    SUPPORTED_URL_SCHEMES
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

fn is_visible(value: &Value) -> bool {
    value
        .get("isVisible")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn string_field<'value>(value: &'value Value, key: &str) -> Option<&'value str> {
    value.get(key)?.as_str()
}

fn pixel_field(value: &Value, key: &str) -> Option<f32> {
    let field = value.get(key)?;
    let pixels = match field.as_str() {
        Some(text) => text
            .trim()
            .strip_suffix(PIXEL_SUFFIX)?
            .trim()
            .parse()
            .ok()?,
        None => field.as_f64()? as f32,
    };
    (pixels > 0.).then_some(pixels)
}

fn parse_column_width(value: Option<&Value>) -> ColumnWidth {
    let Some(value) = value else {
        return ColumnWidth::Stretch;
    };
    if let Some(weight) = value.as_f64() {
        return ColumnWidth::Weighted(weight as f32);
    }
    let Some(text) = value.as_str().map(str::trim) else {
        return ColumnWidth::Stretch;
    };
    if text.eq_ignore_ascii_case("auto") {
        return ColumnWidth::Auto;
    }
    if let Some(pixels) = text
        .strip_suffix(PIXEL_SUFFIX)
        .and_then(|number| number.trim().parse().ok())
    {
        return ColumnWidth::Pixels(pixels);
    }
    text.parse()
        .map(ColumnWidth::Weighted)
        .unwrap_or(ColumnWidth::Stretch)
}

fn parse_spacing(value: Option<&str>) -> CardSpacing {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("none") => CardSpacing::None,
        Some("small") => CardSpacing::Small,
        Some("medium") => CardSpacing::Medium,
        Some("large") => CardSpacing::Large,
        Some("extralarge") => CardSpacing::ExtraLarge,
        Some("padding") => CardSpacing::Padding,
        _ => CardSpacing::Default,
    }
}

fn parse_text_size(value: Option<&str>) -> TextSize {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("small") => TextSize::Small,
        Some("medium") => TextSize::Medium,
        Some("large") => TextSize::Large,
        Some("extralarge") => TextSize::ExtraLarge,
        _ => TextSize::Default,
    }
}

fn parse_text_color(value: Option<&str>) -> TextColor {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("dark") => TextColor::Dark,
        Some("light") => TextColor::Light,
        Some("accent") => TextColor::Accent,
        Some("good") => TextColor::Good,
        Some("warning") => TextColor::Warning,
        Some("attention") => TextColor::Attention,
        _ => TextColor::Default,
    }
}

fn parse_image_size(value: Option<&str>) -> ImageSize {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("stretch") => ImageSize::Stretch,
        Some("small") => ImageSize::Small,
        Some("medium") => ImageSize::Medium,
        Some("large") => ImageSize::Large,
        _ => ImageSize::Auto,
    }
}

fn parse_container_style(value: Option<&str>) -> ContainerStyle {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("emphasis") => ContainerStyle::Emphasis,
        Some("accent") => ContainerStyle::Accent,
        Some("good") => ContainerStyle::Good,
        Some("warning") => ContainerStyle::Warning,
        Some("attention") => ContainerStyle::Attention,
        _ => ContainerStyle::Default,
    }
}

fn parse_vertical_alignment(value: Option<&str>) -> VerticalAlignment {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("center") => VerticalAlignment::Center,
        Some("bottom") => VerticalAlignment::Bottom,
        _ => VerticalAlignment::Top,
    }
}
