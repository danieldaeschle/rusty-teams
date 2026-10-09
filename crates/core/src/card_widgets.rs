use serde_json::Value;

use crate::adaptive_card::{
    CardAction, CardElement, CardItem, TextColor, parse_items, parse_select_action,
    parse_text_color, string_field,
};
use crate::card_layout::ContainerLayout;

const ICON_NAME_SEPARATOR: char = ',';
const MAX_PROGRESS: f32 = 100.;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCodeBlock {
    pub code: String,
    pub language: Option<String>,
    pub start_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeStyle {
    Default,
    Accent,
    Good,
    Attention,
    Warning,
    Subtle,
    Informative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeAppearance {
    Filled,
    Tint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeShape {
    Square,
    Rounded,
    Circular,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSize {
    Medium,
    Large,
    ExtraLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconPosition {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardBadge {
    pub text: String,
    pub icon: Option<String>,
    pub style: BadgeStyle,
    pub appearance: BadgeAppearance,
    pub shape: BadgeShape,
    pub size: BadgeSize,
    pub icon_position: IconPosition,
    pub tooltip: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardProgressBar {
    pub value: Option<f32>,
    pub color: TextColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingSize {
    Tiny,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPosition {
    Before,
    After,
    Above,
    Below,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardProgressRing {
    pub label: Option<String>,
    pub label_position: LabelPosition,
    pub size: RingSize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardCompoundButton {
    pub icon: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub badge: Option<String>,
    pub select_action: Option<CardAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardCarousel {
    pub pages: Vec<CardCarouselPage>,
    pub initial_page: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardCarouselPage {
    pub items: Vec<CardItem>,
    pub layout: ContainerLayout,
    pub select_action: Option<CardAction>,
}

fn non_blank(value: &Value, key: &str) -> Option<String> {
    string_field(value, key)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn icon_name(value: &Value, key: &str) -> Option<String> {
    let name = string_field(value, key)?
        .split(ICON_NAME_SEPARATOR)
        .next()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

pub(crate) fn parse_code_block(value: &Value) -> Option<CardElement> {
    let code = string_field(value, "codeSnippet").filter(|code| !code.trim().is_empty())?;
    Some(CardElement::CodeBlock(CardCodeBlock {
        code: code.trim_end_matches('\n').to_owned(),
        language: non_blank(value, "language"),
        start_line: value
            .get("startLineNumber")
            .and_then(Value::as_u64)
            .map_or(1, |line| line as usize),
    }))
}

pub(crate) fn parse_badge(value: &Value) -> Option<CardElement> {
    let text = non_blank(value, "text");
    let icon = icon_name(value, "icon");
    if text.is_none() && icon.is_none() {
        return None;
    }
    let lowered = |key: &str| string_field(value, key).map(str::to_ascii_lowercase);
    Some(CardElement::Badge(CardBadge {
        text: text.unwrap_or_default(),
        icon,
        style: match lowered("style").as_deref() {
            Some("accent") => BadgeStyle::Accent,
            Some("good") => BadgeStyle::Good,
            Some("attention") => BadgeStyle::Attention,
            Some("warning") => BadgeStyle::Warning,
            Some("subtle") => BadgeStyle::Subtle,
            Some("informative") => BadgeStyle::Informative,
            _ => BadgeStyle::Default,
        },
        appearance: match lowered("appearance").as_deref() {
            Some("tint") => BadgeAppearance::Tint,
            _ => BadgeAppearance::Filled,
        },
        shape: match lowered("shape").as_deref() {
            Some("square") => BadgeShape::Square,
            Some("rounded") => BadgeShape::Rounded,
            _ => BadgeShape::Circular,
        },
        size: match lowered("size").as_deref() {
            Some("large") => BadgeSize::Large,
            Some("extralarge") => BadgeSize::ExtraLarge,
            _ => BadgeSize::Medium,
        },
        icon_position: match lowered("iconPosition").as_deref() {
            Some("after") => IconPosition::After,
            _ => IconPosition::Before,
        },
        tooltip: non_blank(value, "tooltip"),
    }))
}

pub(crate) fn parse_progress_bar(value: &Value) -> Option<CardElement> {
    Some(CardElement::ProgressBar(CardProgressBar {
        value: value
            .get("value")
            .and_then(Value::as_f64)
            .map(|progress| (progress as f32).clamp(0., MAX_PROGRESS)),
        color: parse_text_color(string_field(value, "color")),
    }))
}

pub(crate) fn parse_progress_ring(value: &Value) -> Option<CardElement> {
    let lowered = |key: &str| string_field(value, key).map(str::to_ascii_lowercase);
    Some(CardElement::ProgressRing(CardProgressRing {
        label: non_blank(value, "label"),
        label_position: match lowered("labelPosition").as_deref() {
            Some("before") => LabelPosition::Before,
            Some("above") => LabelPosition::Above,
            Some("below") => LabelPosition::Below,
            _ => LabelPosition::After,
        },
        size: match lowered("size").as_deref() {
            Some("tiny") => RingSize::Tiny,
            Some("small") => RingSize::Small,
            Some("large") => RingSize::Large,
            _ => RingSize::Medium,
        },
    }))
}

pub(crate) fn parse_compound_button(value: &Value) -> Option<CardElement> {
    Some(CardElement::CompoundButton(CardCompoundButton {
        icon: icon_name(value, "icon"),
        title: non_blank(value, "title")?,
        description: non_blank(value, "description"),
        badge: non_blank(value, "badge"),
        select_action: parse_select_action(value),
    }))
}

pub(crate) fn parse_carousel(value: &Value) -> Option<CardElement> {
    let pages: Vec<CardCarouselPage> = value
        .get("pages")?
        .as_array()?
        .iter()
        .filter_map(|page| {
            let items = parse_items(page.get("items"));
            (!items.is_empty()).then(|| CardCarouselPage {
                items,
                layout: ContainerLayout::parse(page),
                select_action: parse_select_action(page),
            })
        })
        .collect();
    let initial_page = value
        .get("initialPageIndex")
        .and_then(Value::as_u64)
        .map_or(0, |index| index as usize)
        .min(pages.len().saturating_sub(1));
    (!pages.is_empty()).then_some(CardElement::Carousel(CardCarousel {
        pages,
        initial_page,
    }))
}
