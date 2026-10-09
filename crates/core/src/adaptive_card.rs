use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::card_date::format_card_dates;
use crate::card_chart::{CardChart, is_chart_type, parse_chart};
use crate::card_inputs::{CardInput, RatingDisplay, merge_input_data, parse_input, parse_rating};
use crate::card_layout::{
    ContainerLayout, HorizontalAlignment, parse_color_hex, parse_horizontal_alignment,
};
use crate::card_table::{CardTable, parse_table};
use crate::card_widgets::{
    CardBadge, CardCarousel, CardCodeBlock, CardCompoundButton, CardProgressBar, CardProgressRing,
    parse_badge, parse_carousel, parse_code_block, parse_compound_button, parse_progress_bar,
    parse_progress_ring,
};
use crate::card_width::TargetWidth;
use crate::markdown::{card_markdown_to_html, split_mentions};
use crate::spans::{FontSize, Span, html_to_spans};
use crate::stored::push_plain;

const SUPPORTED_URL_SCHEMES: [&str; 2] = ["https://", "http://"];
const IMAGE_URL_SCHEME: &str = "https://";
const SUPPORTED_CARD_VERSION: (u32, u32) = (1, 6);
const ADAPTIVE_CARDS_FEATURE: &str = "adaptiveCards";
const FALLBACK_DROP: &str = "drop";
const MAX_PRIMARY_ACTIONS: usize = 6;
const ACCENT_RUN_COLOR: u32 = 0xe08a5c;
const GOOD_RUN_COLOR: u32 = 0x22c55e;
const WARNING_RUN_COLOR: u32 = 0xfbbf24;
const ATTENTION_RUN_COLOR: u32 = 0xf87171;
const SUBTLE_RUN_COLOR: u32 = 0xa1a1a6;
const HIGHLIGHT_BACKGROUND: u32 = 0xfacc15;
const HIGHLIGHT_FOREGROUND: u32 = 0x18181b;
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
const INVOKE_TYPE: &str = "invoke";
const SIGN_IN_TYPE: &str = "signin";
const TAB_INFO_TYPE: &str = "tab/tabInfoAction";
const ADD_APP_TYPES: [&str; 2] = ["appInstallToConversation", "addAppToConversation"];
const ADD_APP_UNAVAILABLE: &str = "Adding apps is not supported";
const AUTO_REFRESH_MEMBER_LIMIT: usize = 60;
const ICON_URL_PREFIX: &str = "icon:";
const PIXEL_SUFFIX: &str = "px";

#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveCard {
    pub full_width: bool,
    pub items: Vec<CardItem>,
    pub actions: Vec<CardAction>,
    pub select_action: Option<CardAction>,
    pub refresh: Option<CardRefresh>,
    pub layout: ContainerLayout,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardRefresh {
    pub action: ExecuteAction,
    pub user_ids: Option<Vec<String>>,
}

impl CardRefresh {
    pub fn runs_automatically(
        &self,
        member_count: Option<usize>,
        my_user_id: Option<&str>,
    ) -> bool {
        match &self.user_ids {
            Some(user_ids) => my_user_id.is_some_and(|me| {
                user_ids
                    .iter()
                    .any(|user_id| user_guid(user_id).eq_ignore_ascii_case(user_guid(me)))
            }),
            None => member_count.is_some_and(|count| count <= AUTO_REFRESH_MEMBER_LIMIT),
        }
    }
}

fn user_guid(user_id: &str) -> &str {
    user_id.rsplit(':').next().unwrap_or(user_id)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardItem {
    pub id: Option<String>,
    pub visible: bool,
    pub separator: bool,
    pub spacing: CardSpacing,
    pub stretch: bool,
    pub target_width: Option<TargetWidth>,
    pub element: CardElement,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CardElement {
    Text(CardText),
    Image(CardImage),
    ImageSet(Vec<CardImage>),
    Columns {
        columns: Vec<CardColumn>,
        layout: ContainerLayout,
    },
    Container {
        layout: ContainerLayout,
        items: Vec<CardItem>,
        select_action: Option<CardAction>,
    },
    Facts(Vec<CardFact>),
    Media(CardMedia),
    Icon(CardIcon),
    Table(CardTable),
    CodeBlock(CardCodeBlock),
    Badge(CardBadge),
    ProgressBar(CardProgressBar),
    ProgressRing(CardProgressRing),
    CompoundButton(CardCompoundButton),
    Carousel(CardCarousel),
    Actions(Vec<CardAction>),
    Input(CardInput),
    Rating(RatingDisplay),
    Chart(CardChart),
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
pub enum IconSize {
    ExtraExtraSmall,
    ExtraSmall,
    Small,
    Standard,
    Medium,
    Large,
    ExtraLarge,
    ExtraExtraLarge,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContainerStyle {
    #[default]
    Default,
    Emphasis,
    Accent,
    Good,
    Warning,
    Attention,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlignment {
    #[default]
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
    pub max_lines: Option<usize>,
    pub alignment: Option<HorizontalAlignment>,
    pub monospace: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardImage {
    pub url: String,
    pub size: ImageSize,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub person: bool,
    pub alt_text: Option<String>,
    pub alignment: HorizontalAlignment,
    pub background_color: Option<u32>,
    pub select_action: Option<CardAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardMedia {
    pub poster_url: Option<String>,
    pub source_url: String,
    pub alt_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardIcon {
    pub name: String,
    pub size: IconSize,
    pub color: TextColor,
    pub filled: bool,
    pub select_action: Option<CardAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardColumn {
    pub width: ColumnWidth,
    pub layout: ContainerLayout,
    pub target_width: Option<TargetWidth>,
    pub items: Vec<CardItem>,
    pub select_action: Option<CardAction>,
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
    pub icon: Option<CardActionIcon>,
    pub secondary: bool,
    pub tooltip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardActionIcon {
    Named(String),
    Url(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CardActionKind {
    OpenUrl(String),
    Submit(SubmitAction),
    Execute(ExecuteAction),
    ShowCard(Box<AdaptiveCard>),
    ToggleVisibility(Vec<ToggleTarget>),
    Unavailable(&'static str),
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
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecuteTrigger {
    Manual,
    Automatic,
}

impl ExecuteTrigger {
    fn as_str(self) -> &'static str {
        match self {
            ExecuteTrigger::Manual => "manual",
            ExecuteTrigger::Automatic => "automatic",
        }
    }
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

    fn invoke_value(&self) -> Option<&Value> {
        self.teams_settings()
            .filter(|settings| {
                string_field(settings, "type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case(INVOKE_TYPE))
            })
            .and_then(|settings| settings.get("value"))
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
                name: TASK_FETCH_NAME.to_owned(),
                value: task_value(Value::Object(data)),
            };
        }
        if let Some((name, value)) = self.invoke_value().and_then(|value| {
            let name = string_field(value, "type").filter(|name| !name.is_empty())?;
            Some((name, value))
        }) {
            return InvokePayload {
                name: name.to_owned(),
                value: value.clone(),
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
            name: MESSAGE_BACK_NAME.to_owned(),
            value: message_back.unwrap_or(&self.data).clone(),
        }
    }
}

impl ExecuteAction {
    pub fn payload(&self) -> InvokePayload {
        self.payload_for(ExecuteTrigger::Manual)
    }

    pub fn payload_for(&self, trigger: ExecuteTrigger) -> InvokePayload {
        let mut action = Map::new();
        action.insert("type".to_owned(), json!(EXECUTE_ACTION));
        if let Some(id) = &self.id {
            action.insert("id".to_owned(), json!(id));
        }
        action.insert("verb".to_owned(), json!(self.verb));
        action.insert("data".to_owned(), self.data.clone());
        InvokePayload {
            name: EXECUTE_NAME.to_owned(),
            value: json!({"action": action, "trigger": trigger.as_str()}),
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
            CardActionKind::Execute(execute) => CardActionKind::Execute(ExecuteAction {
                data: merge_input_data(&execute.data, values),
                ..execute.clone()
            }),
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
        self.enabled
            && !matches!(
                self.kind,
                CardActionKind::Unsupported | CardActionKind::Unavailable(_)
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSplit {
    pub primary: Vec<usize>,
    pub overflow: Vec<usize>,
}

pub fn split_overflow(
    actions: &[CardAction],
    is_visible: impl Fn(&CardAction) -> bool,
) -> ActionSplit {
    let mut split = ActionSplit {
        primary: Vec::new(),
        overflow: Vec::new(),
    };
    for (index, action) in actions
        .iter()
        .enumerate()
        .filter(|(_, action)| is_visible(action))
    {
        if action.secondary || split.primary.len() >= MAX_PRIMARY_ACTIONS {
            split.overflow.push(index);
        } else {
            split.primary.push(index);
        }
    }
    split
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
            select_action: parse_select_action(root),
            refresh: parse_refresh(root),
            layout: ContainerLayout::parse(root),
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
        for nested in item.element.nested_items() {
            collect_visibility(nested, &[], visibility);
        }
        if let CardElement::Actions(actions) = &item.element {
            collect_visibility(&[], actions, visibility);
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
        if let CardElement::Input(input) = &item.element {
            inputs.push(input.clone());
        }
        for nested in item.element.nested_items() {
            collect_inputs(nested, inputs);
        }
    }
}

fn collect_item_actions<'card>(items: &'card [CardItem], actions: &mut Vec<&'card CardAction>) {
    for item in items {
        if let CardElement::Actions(set) = &item.element {
            actions.extend(set);
        }
        for nested in item.element.nested_items() {
            collect_item_actions(nested, actions);
        }
    }
}

impl CardElement {
    fn nested_items(&self) -> Vec<&[CardItem]> {
        match self {
            CardElement::Columns { columns, .. } => columns
                .iter()
                .map(|column| column.items.as_slice())
                .collect(),
            CardElement::Container { items, .. } => vec![items.as_slice()],
            CardElement::Table(table) => table
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .map(|cell| cell.items.as_slice())
                .collect(),
            CardElement::Carousel(carousel) => carousel
                .pages
                .iter()
                .map(|page| page.items.as_slice())
                .collect(),
            _ => Vec::new(),
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
            CardElement::Facts(facts) => {
                for fact in facts {
                    let mut line = format!("{}: ", fact.title);
                    push_plain(&mut line, &fact.value);
                    lines.push(line.trim().to_owned());
                }
            }
            CardElement::CodeBlock(block) => lines.push(block.code.clone()),
            CardElement::Badge(badge) if !badge.text.is_empty() => lines.push(badge.text.clone()),
            CardElement::CompoundButton(button) => {
                lines.push(button.title.clone());
                lines.extend(button.description.clone());
            }
            CardElement::ProgressRing(ring) => lines.extend(ring.label.clone()),
            _ => {}
        }
        for nested in item.element.nested_items() {
            collect_lines(nested, lines);
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

pub(crate) fn parse_items(value: Option<&Value>) -> Vec<CardItem> {
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
    let element = parse_element(value).or_else(|| fallback_element(value))?;
    Some(CardItem {
        id,
        visible,
        separator: value
            .get("separator")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        spacing: parse_spacing(string_field(value, "spacing")),
        stretch: string_field(value, "height")
            .is_some_and(|height| height.eq_ignore_ascii_case("stretch")),
        target_width: parse_target_width(value),
        element,
    })
}

fn parse_target_width(value: &Value) -> Option<TargetWidth> {
    string_field(value, "targetWidth").and_then(TargetWidth::parse)
}

fn parse_element(value: &Value) -> Option<CardElement> {
    if !requirements_met(value) {
        return None;
    }
    match string_field(value, "type")? {
        "TextBlock" => parse_text_block(value),
        "RichTextBlock" => parse_rich_text_block(value),
        "Image" => parse_image(value).map(CardElement::Image),
        "ImageSet" => parse_image_set(value),
        "ColumnSet" => parse_column_set(value),
        "Container" => parse_container(value),
        "FactSet" => parse_fact_set(value),
        "Media" => parse_media(value).map(CardElement::Media),
        "Icon" => parse_icon(value).map(CardElement::Icon),
        "Table" => parse_table(value),
        "CodeBlock" => parse_code_block(value),
        "Badge" => parse_badge(value),
        "ProgressBar" => parse_progress_bar(value),
        "ProgressRing" => parse_progress_ring(value),
        "CompoundButton" => parse_compound_button(value),
        "Carousel" => parse_carousel(value),
        "ActionSet" => parse_action_set(value),
        "Rating" => Some(CardElement::Rating(parse_rating(value))),
        chart_type if is_chart_type(chart_type) => {
            parse_chart(value, chart_type).map(CardElement::Chart)
        }
        input_type if input_type.starts_with(INPUT_TYPE_PREFIX) => {
            parse_input(value, input_type).map(CardElement::Input)
        }
        _ => None,
    }
}

fn requirements_met(value: &Value) -> bool {
    value
        .get("requires")
        .and_then(Value::as_object)
        .is_none_or(|requires| {
            requires.iter().all(|(feature, version)| {
                feature.eq_ignore_ascii_case(ADAPTIVE_CARDS_FEATURE)
                    && version_supported(version.as_str().unwrap_or_default())
            })
        })
}

fn version_supported(version: &str) -> bool {
    let version = version.trim();
    if version == "*" {
        return true;
    }
    let mut parts = version
        .split('.')
        .map(|part| part.trim().parse::<u32>().unwrap_or(0));
    let required = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    required <= SUPPORTED_CARD_VERSION
}

fn fallback_element(value: &Value) -> Option<CardElement> {
    match value.get("fallback") {
        Some(Value::String(mode)) if mode.eq_ignore_ascii_case(FALLBACK_DROP) => return None,
        Some(fallback @ Value::Object(_)) => {
            if let Some(item) = parse_item(fallback) {
                return Some(item.element);
            }
        }
        _ => {}
    }
    fallback_item(value).map(|item| item.element)
}

fn fallback_item(value: &Value) -> Option<CardItem> {
    let text = string_field(value, "fallbackText").filter(|text| !text.trim().is_empty())?;
    Some(CardItem {
        id: None,
        visible: true,
        separator: false,
        spacing: CardSpacing::Default,
        stretch: false,
        target_width: None,
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
        max_lines: None,
        alignment: None,
        monospace: false,
    }
}

fn parse_text_block(value: &Value) -> Option<CardElement> {
    let text = string_field(value, "text").filter(|text| !text.trim().is_empty())?;
    let text = format_card_dates(text);
    let heading =
        string_field(value, "style").is_some_and(|style| style.eq_ignore_ascii_case("heading"));
    let size = parse_text_size(string_field(value, "size"));
    Some(CardElement::Text(CardText {
        spans: html_to_spans(&card_markdown_to_html(&text)),
        size: match size {
            TextSize::Default if heading => TextSize::Large,
            other => other,
        },
        bold: heading || string_field(value, "weight").is_some_and(is_bold_weight),
        color: parse_text_color(string_field(value, "color")),
        subtle: bool_field(value, "isSubtle"),
        wrap: bool_field(value, "wrap"),
        max_lines: value
            .get("maxLines")
            .and_then(Value::as_u64)
            .filter(|lines| *lines > 0)
            .map(|lines| lines as usize),
        alignment: parse_horizontal_alignment(string_field(value, "horizontalAlignment")),
        monospace: is_monospace(value),
    }))
}

fn is_monospace(value: &Value) -> bool {
    string_field(value, "fontType").is_some_and(|font| font.eq_ignore_ascii_case("monospace"))
}

fn parse_rich_text_block(value: &Value) -> Option<CardElement> {
    let spans: Vec<Span> = value
        .get("inlines")?
        .as_array()?
        .iter()
        .flat_map(parse_text_run)
        .collect();
    (!spans.is_empty()).then(|| {
        CardElement::Text(CardText {
            alignment: parse_horizontal_alignment(string_field(value, "horizontalAlignment")),
            ..plain_text_block(spans)
        })
    })
}

fn parse_text_run(value: &Value) -> Vec<Span> {
    let Some(text) = value
        .as_str()
        .or_else(|| string_field(value, "text"))
        .filter(|text| !text.is_empty())
    else {
        return Vec::new();
    };
    let mut spans = split_mentions(&format_card_dates(text));
    if is_monospace(value) {
        spans = spans
            .into_iter()
            .map(|span| match span {
                Span::Text(text) => Span::Code(text),
                other => other,
            })
            .collect();
    }
    if string_field(value, "weight").is_some_and(is_bold_weight) {
        spans = vec![Span::Bold(spans)];
    }
    if bool_field(value, "italic") {
        spans = vec![Span::Italic(spans)];
    }
    if bool_field(value, "strikethrough") {
        spans = vec![Span::Strike(spans)];
    }
    if bool_field(value, "underline") {
        spans = vec![Span::Underline(spans)];
    }
    if let Some(pixels) = run_font_pixels(string_field(value, "size")) {
        spans = vec![Span::Sized(FontSize::Pixels(pixels), spans)];
    }
    let highlight = bool_field(value, "highlight");
    let color = run_color(string_field(value, "color"), bool_field(value, "isSubtle"))
        .or(highlight.then_some(HIGHLIGHT_FOREGROUND));
    if color.is_some() || highlight {
        spans = vec![Span::Colored {
            color,
            background: highlight.then_some(HIGHLIGHT_BACKGROUND),
            children: spans,
        }];
    }
    let url = value
        .get("selectAction")
        .filter(|action| string_field(action, "type") == Some(OPEN_URL_ACTION))
        .and_then(|action| string_field(action, "url"))
        .filter(|url| is_supported_url(url));
    match url {
        Some(url) => vec![Span::Link {
            url: url.to_owned(),
            children: spans,
        }],
        None => spans,
    }
}

fn run_font_pixels(size: Option<&str>) -> Option<u16> {
    match parse_text_size(size) {
        TextSize::Small => Some(12),
        TextSize::Default if size.is_none() => None,
        TextSize::Default => Some(14),
        TextSize::Medium => Some(15),
        TextSize::Large => Some(18),
        TextSize::ExtraLarge => Some(21),
    }
}

fn run_color(color: Option<&str>, subtle: bool) -> Option<u32> {
    match parse_text_color(color) {
        TextColor::Accent => Some(ACCENT_RUN_COLOR),
        TextColor::Good => Some(GOOD_RUN_COLOR),
        TextColor::Warning => Some(WARNING_RUN_COLOR),
        TextColor::Attention => Some(ATTENTION_RUN_COLOR),
        TextColor::Default | TextColor::Dark | TextColor::Light => {
            subtle.then_some(SUBTLE_RUN_COLOR)
        }
    }
}

fn parse_image(value: &Value) -> Option<CardImage> {
    let url = string_field(value, "url").filter(|url| is_supported_url(url))?;
    Some(CardImage {
        url: url.to_owned(),
        size: parse_image_size(string_field(value, "size")),
        width: pixel_field(value, "width"),
        height: pixel_field(value, "height"),
        person: string_field(value, "style")
            .is_some_and(|style| style.eq_ignore_ascii_case("person")),
        alt_text: string_field(value, "altText")
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned),
        alignment: parse_horizontal_alignment(string_field(value, "horizontalAlignment"))
            .unwrap_or_default(),
        background_color: string_field(value, "backgroundColor").and_then(parse_color_hex),
        select_action: parse_select_action(value),
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
                layout: ContainerLayout::parse(column),
                target_width: parse_target_width(column),
                items,
                select_action: parse_select_action(column),
            })
        })
        .collect();
    (!columns.is_empty()).then(|| CardElement::Columns {
        columns,
        layout: ContainerLayout::parse(value),
    })
}

fn parse_container(value: &Value) -> Option<CardElement> {
    let items = parse_items(value.get("items"));
    (!items.is_empty()).then(|| CardElement::Container {
        layout: ContainerLayout::parse(value),
        items,
        select_action: parse_select_action(value),
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
                title: format_card_dates(title),
                value: html_to_spans(&card_markdown_to_html(&format_card_dates(text))),
            })
        })
        .collect();
    (!facts.is_empty()).then_some(CardElement::Facts(facts))
}

fn parse_media(value: &Value) -> Option<CardMedia> {
    let source_url = value
        .get("sources")?
        .as_array()?
        .iter()
        .filter_map(|source| string_field(source, "url"))
        .find(|url| is_supported_url(url))?;
    Some(CardMedia {
        poster_url: string_field(value, "poster")
            .filter(|url| url.starts_with(IMAGE_URL_SCHEME))
            .map(str::to_owned),
        source_url: source_url.to_owned(),
        alt_text: string_field(value, "altText")
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned),
    })
}

fn parse_icon(value: &Value) -> Option<CardIcon> {
    let name = string_field(value, "name").filter(|name| !name.trim().is_empty())?;
    Some(CardIcon {
        name: name.trim().to_owned(),
        size: parse_icon_size(string_field(value, "size")),
        color: parse_text_color(string_field(value, "color")),
        filled: string_field(value, "style")
            .is_some_and(|style| style.eq_ignore_ascii_case("filled")),
        select_action: parse_select_action(value),
    })
}

pub(crate) fn parse_select_action(value: &Value) -> Option<CardAction> {
    let action = parse_action_titled(value.get("selectAction")?, false)?;
    matches!(
        action.kind,
        CardActionKind::OpenUrl(_)
            | CardActionKind::Submit(_)
            | CardActionKind::Execute(_)
            | CardActionKind::ToggleVisibility(_)
    )
    .then_some(action)
}

fn parse_refresh(value: &Value) -> Option<CardRefresh> {
    let refresh = value.get("refresh")?;
    let action = parse_action_titled(refresh.get("action")?, false)?;
    let CardActionKind::Execute(execute) = action.kind else {
        return None;
    };
    Some(CardRefresh {
        action: execute,
        user_ids: refresh
            .get("userIds")
            .and_then(Value::as_array)
            .map(|user_ids| {
                user_ids
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            }),
    })
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

pub(crate) fn parse_action(value: &Value) -> Option<CardAction> {
    parse_action_titled(value, true)
}

fn parse_action_titled(value: &Value, title_required: bool) -> Option<CardAction> {
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
        .filter(|title| !title.is_empty());
    let title = match title {
        Some(title) => title,
        None if title_required => return None,
        None => "",
    };
    let data = || value.get("data").cloned().unwrap_or(Value::Null);
    let kind = match action_type {
        OPEN_URL_ACTION => string_field(value, "url")
            .filter(|url| is_supported_url(url))
            .map_or(CardActionKind::Unsupported, |url| {
                CardActionKind::OpenUrl(url.to_owned())
            }),
        SUBMIT_ACTION => submit_kind(data()),
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
        icon: string_field(value, "iconUrl").and_then(parse_action_icon),
        secondary: string_field(value, "mode")
            .is_some_and(|mode| mode.eq_ignore_ascii_case("secondary")),
        tooltip: string_field(value, "tooltip")
            .map(str::trim)
            .filter(|tooltip| !tooltip.is_empty())
            .map(str::to_owned),
    })
}

fn parse_action_icon(icon_url: &str) -> Option<CardActionIcon> {
    match icon_url.strip_prefix(ICON_URL_PREFIX) {
        Some(name) => Some(name.split(',').next()?.trim())
            .filter(|name| !name.is_empty())
            .map(|name| CardActionIcon::Named(name.to_owned())),
        None => icon_url
            .starts_with(IMAGE_URL_SCHEME)
            .then(|| CardActionIcon::Url(icon_url.to_owned())),
    }
}

fn submit_kind(data: Value) -> CardActionKind {
    let submit = SubmitAction { data };
    let settings_type = submit
        .teams_settings()
        .and_then(|settings| string_field(settings, "type"));
    let invoke_type = submit
        .invoke_value()
        .and_then(|value| string_field(value, "type"));
    let names_add_app = [settings_type, invoke_type]
        .into_iter()
        .flatten()
        .any(|name| {
            ADD_APP_TYPES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name))
        });
    if names_add_app {
        return CardActionKind::Unavailable(ADD_APP_UNAVAILABLE);
    }
    let open_url = |url: Option<&str>| {
        url.filter(|url| is_supported_url(url))
            .map_or(CardActionKind::Unsupported, |url| {
                CardActionKind::OpenUrl(url.to_owned())
            })
    };
    if settings_type.is_some_and(|name| name.eq_ignore_ascii_case(SIGN_IN_TYPE)) {
        let url = submit
            .teams_settings()
            .and_then(|settings| string_field(settings, "value"));
        return open_url(url);
    }
    if invoke_type == Some(TAB_INFO_TYPE) {
        let tab_info = submit.invoke_value().and_then(|value| value.get("tabInfo"));
        let url = tab_info
            .and_then(|info| string_field(info, "websiteUrl"))
            .filter(|url| is_supported_url(url))
            .or_else(|| tab_info.and_then(|info| string_field(info, "contentUrl")));
        return open_url(url);
    }
    CardActionKind::Submit(submit)
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

pub(crate) fn is_supported_url(url: &str) -> bool {
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

pub(crate) fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn string_field<'value>(value: &'value Value, key: &str) -> Option<&'value str> {
    value.get(key)?.as_str()
}

pub(crate) fn pixel_field(value: &Value, key: &str) -> Option<f32> {
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

pub(crate) fn parse_column_width(value: Option<&Value>) -> ColumnWidth {
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

pub(crate) fn parse_text_color(value: Option<&str>) -> TextColor {
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

fn parse_icon_size(value: Option<&str>) -> IconSize {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("xxsmall") => IconSize::ExtraExtraSmall,
        Some("xsmall") => IconSize::ExtraSmall,
        Some("small") => IconSize::Small,
        Some("medium") => IconSize::Medium,
        Some("large") => IconSize::Large,
        Some("xlarge") => IconSize::ExtraLarge,
        Some("xxlarge") => IconSize::ExtraExtraLarge,
        _ => IconSize::Standard,
    }
}

pub(crate) fn parse_container_style(value: Option<&str>) -> ContainerStyle {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("emphasis") => ContainerStyle::Emphasis,
        Some("accent") => ContainerStyle::Accent,
        Some("good") => ContainerStyle::Good,
        Some("warning") => ContainerStyle::Warning,
        Some("attention") => ContainerStyle::Attention,
        _ => ContainerStyle::Default,
    }
}

pub(crate) fn parse_vertical_alignment(value: Option<&str>) -> VerticalAlignment {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("center") => VerticalAlignment::Center,
        Some("bottom") => VerticalAlignment::Bottom,
        _ => VerticalAlignment::Top,
    }
}
