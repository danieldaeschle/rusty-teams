use std::f32::consts::{FRAC_PI_2, TAU};

use serde_json::Value;

const CHART_TYPE_PREFIX: &str = "Chart.";
const DEFAULT_GAUGE_MAX: f64 = 100.;

#[derive(Debug, Clone, PartialEq)]
pub struct CardChart {
    pub title: Option<String>,
    pub x_axis_title: Option<String>,
    pub y_axis_title: Option<String>,
    pub show_legend: bool,
    pub show_values: bool,
    pub kind: ChartKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChartKind {
    Donut(Vec<ChartPoint>),
    Pie(Vec<ChartPoint>),
    Gauge(ChartGauge),
    Line(Vec<ChartSeries>),
    VerticalBar(Vec<ChartPoint>),
    GroupedBar {
        series: Vec<ChartSeries>,
        stacked: bool,
    },
    HorizontalBar {
        bars: Vec<ChartPoint>,
        mode: BarDisplayMode,
    },
    StackedHorizontalBar(Vec<ChartSeries>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarDisplayMode {
    AbsoluteWithAxis,
    AbsoluteNoAxis,
    PartToWhole,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartPoint {
    pub label: String,
    pub value: f64,
    pub color: ChartColor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    pub legend: String,
    pub color: ChartColor,
    pub points: Vec<ChartPoint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartGauge {
    pub minimum: f64,
    pub maximum: f64,
    pub value: f64,
    pub segments: Vec<GaugeSegment>,
    pub sub_label: Option<String>,
    pub format: GaugeValueFormat,
    pub show_min_max: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GaugeSegment {
    pub legend: String,
    pub size: f64,
    pub color: ChartColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaugeValueFormat {
    Percentage,
    Fraction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartColorSet {
    Categorical,
    Sequential,
    Diverging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartColor {
    Good,
    Warning,
    Attention,
    Neutral,
    CategoricalRed,
    CategoricalPurple,
    CategoricalLavender,
    CategoricalBlue,
    CategoricalLightBlue,
    CategoricalTeal,
    CategoricalGreen,
    CategoricalLime,
    CategoricalMarigold,
    Sequential1,
    Sequential2,
    Sequential3,
    Sequential4,
    Sequential5,
    Sequential6,
    Sequential7,
    Sequential8,
    DivergingBlue,
    DivergingLightBlue,
    DivergingCyan,
    DivergingTeal,
    DivergingYellow,
    DivergingPeach,
    DivergingLightRed,
    DivergingRed,
    DivergingMaroon,
    DivergingGray,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisScale {
    pub maximum: f64,
    pub step: f64,
}

const NAMED_COLORS: [(&str, ChartColor); 31] = [
    ("good", ChartColor::Good),
    ("warning", ChartColor::Warning),
    ("attention", ChartColor::Attention),
    ("neutral", ChartColor::Neutral),
    ("categoricalred", ChartColor::CategoricalRed),
    ("categoricalpurple", ChartColor::CategoricalPurple),
    ("categoricallavender", ChartColor::CategoricalLavender),
    ("categoricalblue", ChartColor::CategoricalBlue),
    ("categoricallightblue", ChartColor::CategoricalLightBlue),
    ("categoricalteal", ChartColor::CategoricalTeal),
    ("categoricalgreen", ChartColor::CategoricalGreen),
    ("categoricallime", ChartColor::CategoricalLime),
    ("categoricalmarigold", ChartColor::CategoricalMarigold),
    ("sequential1", ChartColor::Sequential1),
    ("sequential2", ChartColor::Sequential2),
    ("sequential3", ChartColor::Sequential3),
    ("sequential4", ChartColor::Sequential4),
    ("sequential5", ChartColor::Sequential5),
    ("sequential6", ChartColor::Sequential6),
    ("sequential7", ChartColor::Sequential7),
    ("sequential8", ChartColor::Sequential8),
    ("divergingblue", ChartColor::DivergingBlue),
    ("diverginglightblue", ChartColor::DivergingLightBlue),
    ("divergingcyan", ChartColor::DivergingCyan),
    ("divergingteal", ChartColor::DivergingTeal),
    ("divergingyellow", ChartColor::DivergingYellow),
    ("divergingpeach", ChartColor::DivergingPeach),
    ("diverginglightred", ChartColor::DivergingLightRed),
    ("divergingred", ChartColor::DivergingRed),
    ("divergingmaroon", ChartColor::DivergingMaroon),
    ("diverginggray", ChartColor::DivergingGray),
];

const CATEGORICAL_ORDER: [ChartColor; 9] = [
    ChartColor::CategoricalBlue,
    ChartColor::CategoricalTeal,
    ChartColor::CategoricalMarigold,
    ChartColor::CategoricalRed,
    ChartColor::CategoricalPurple,
    ChartColor::CategoricalGreen,
    ChartColor::CategoricalLavender,
    ChartColor::CategoricalLime,
    ChartColor::CategoricalLightBlue,
];

const SEQUENTIAL_ORDER: [ChartColor; 8] = [
    ChartColor::Sequential3,
    ChartColor::Sequential5,
    ChartColor::Sequential7,
    ChartColor::Sequential1,
    ChartColor::Sequential4,
    ChartColor::Sequential6,
    ChartColor::Sequential2,
    ChartColor::Sequential8,
];

const DIVERGING_ORDER: [ChartColor; 10] = [
    ChartColor::DivergingBlue,
    ChartColor::DivergingRed,
    ChartColor::DivergingTeal,
    ChartColor::DivergingPeach,
    ChartColor::DivergingLightBlue,
    ChartColor::DivergingMaroon,
    ChartColor::DivergingYellow,
    ChartColor::DivergingCyan,
    ChartColor::DivergingLightRed,
    ChartColor::DivergingGray,
];

impl ChartColor {
    pub fn from_name(name: &str) -> Option<ChartColor> {
        let lowered = name.trim().to_ascii_lowercase();
        NAMED_COLORS
            .iter()
            .find(|(candidate, _)| *candidate == lowered)
            .map(|(_, color)| *color)
    }
}

impl ChartColorSet {
    pub fn from_name(name: Option<&str>) -> ChartColorSet {
        let lowered = name.unwrap_or_default().to_ascii_lowercase();
        if lowered.starts_with("sequential") {
            ChartColorSet::Sequential
        } else if lowered.starts_with("diverging") {
            ChartColorSet::Diverging
        } else {
            ChartColorSet::Categorical
        }
    }

    pub fn color_at(self, index: usize) -> ChartColor {
        match self {
            ChartColorSet::Categorical => CATEGORICAL_ORDER[index % CATEGORICAL_ORDER.len()],
            ChartColorSet::Sequential => SEQUENTIAL_ORDER[index % SEQUENTIAL_ORDER.len()],
            ChartColorSet::Diverging => DIVERGING_ORDER[index % DIVERGING_ORDER.len()],
        }
    }
}

struct ColorPlan {
    color_set: ChartColorSet,
    uniform: Option<ChartColor>,
}

impl ColorPlan {
    fn new(value: &Value) -> ColorPlan {
        ColorPlan {
            color_set: ChartColorSet::from_name(string_field(value, "colorSet")),
            uniform: named_color(value),
        }
    }

    fn resolve(&self, entry: &Value, index: usize) -> ChartColor {
        named_color(entry)
            .or(self.uniform)
            .unwrap_or_else(|| self.color_set.color_at(index))
    }
}

pub fn is_chart_type(element_type: &str) -> bool {
    element_type.starts_with(CHART_TYPE_PREFIX)
}

pub fn parse_chart(value: &Value, element_type: &str) -> Option<CardChart> {
    let plan = ColorPlan::new(value);
    let kind = match element_type {
        "Chart.Donut" => ChartKind::Donut(parse_slices(value, &plan)),
        "Chart.Pie" => ChartKind::Pie(parse_slices(value, &plan)),
        "Chart.Gauge" => ChartKind::Gauge(parse_gauge(value, &plan)),
        "Chart.Line" => ChartKind::Line(parse_series(value, &plan)),
        "Chart.VerticalBar" => ChartKind::VerticalBar(parse_xy_points(value, &plan)),
        "Chart.VerticalBar.Grouped" => ChartKind::GroupedBar {
            series: parse_series(value, &plan),
            stacked: bool_field(value, "stacked", false),
        },
        "Chart.HorizontalBar" => ChartKind::HorizontalBar {
            bars: parse_xy_points(value, &plan),
            mode: parse_display_mode(string_field(value, "displayMode")),
        },
        "Chart.HorizontalBar.Stacked" => {
            ChartKind::StackedHorizontalBar(parse_stacked_rows(value, &plan))
        }
        _ => return None,
    };
    Some(CardChart {
        title: text_field(value, "title"),
        x_axis_title: text_field(value, "xAxisTitle"),
        y_axis_title: text_field(value, "yAxisTitle"),
        show_legend: bool_field(value, "showLegend", true),
        show_values: bool_field(value, "showBarValues", false),
        kind,
    })
}

impl CardChart {
    pub fn legend_entries(&self) -> Vec<(String, ChartColor)> {
        match &self.kind {
            ChartKind::Donut(points) | ChartKind::Pie(points) => {
                points.iter().map(point_legend).collect()
            }
            ChartKind::Gauge(gauge) => gauge
                .segments
                .iter()
                .filter(|segment| !segment.legend.is_empty())
                .map(|segment| (segment.legend.clone(), segment.color))
                .collect(),
            ChartKind::Line(series) | ChartKind::GroupedBar { series, .. } => series
                .iter()
                .map(|entry| (entry.legend.clone(), entry.color))
                .collect(),
            ChartKind::StackedHorizontalBar(rows) => {
                let mut entries: Vec<(String, ChartColor)> = Vec::new();
                for point in rows.iter().flat_map(|row| &row.points) {
                    if !entries.iter().any(|(legend, _)| *legend == point.label) {
                        entries.push(point_legend(point));
                    }
                }
                entries
            }
            ChartKind::VerticalBar(_) | ChartKind::HorizontalBar { .. } => Vec::new(),
        }
    }
}

fn point_legend(point: &ChartPoint) -> (String, ChartColor) {
    (point.label.clone(), point.color)
}

fn parse_slices(value: &Value, plan: &ColorPlan) -> Vec<ChartPoint> {
    array_field(value, "data")
        .iter()
        .enumerate()
        .map(|(index, entry)| ChartPoint {
            label: text_field(entry, "legend").unwrap_or_default(),
            value: number_field(entry, "value").unwrap_or(0.),
            color: plan.resolve(entry, index),
        })
        .collect()
}

fn parse_xy_points(value: &Value, plan: &ColorPlan) -> Vec<ChartPoint> {
    array_field(value, "data")
        .iter()
        .enumerate()
        .map(|(index, entry)| ChartPoint {
            label: label_field(entry, "x"),
            value: number_field(entry, "y").unwrap_or(0.),
            color: plan.resolve(entry, index),
        })
        .collect()
}

fn parse_series(value: &Value, plan: &ColorPlan) -> Vec<ChartSeries> {
    array_field(value, "data")
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let color = plan.resolve(entry, index);
            ChartSeries {
                legend: text_field(entry, "legend").unwrap_or_default(),
                color,
                points: array_field(entry, "values")
                    .iter()
                    .map(|point| ChartPoint {
                        label: label_field(point, "x"),
                        value: number_field(point, "y").unwrap_or(0.),
                        color,
                    })
                    .collect(),
            }
        })
        .collect()
}

fn parse_stacked_rows(value: &Value, plan: &ColorPlan) -> Vec<ChartSeries> {
    array_field(value, "data")
        .iter()
        .enumerate()
        .map(|(index, row)| ChartSeries {
            legend: text_field(row, "title").unwrap_or_default(),
            color: plan.resolve(row, index),
            points: parse_slices(row, plan),
        })
        .collect()
}

fn parse_gauge(value: &Value, plan: &ColorPlan) -> ChartGauge {
    let segments: Vec<GaugeSegment> = array_field(value, "segments")
        .iter()
        .enumerate()
        .map(|(index, entry)| GaugeSegment {
            legend: text_field(entry, "legend").unwrap_or_default(),
            size: number_field(entry, "size")
                .or_else(|| number_field(entry, "value"))
                .unwrap_or(0.)
                .max(0.),
            color: plan.resolve(entry, index),
        })
        .collect();
    let segment_total: f64 = segments.iter().map(|segment| segment.size).sum();
    let minimum = number_field(value, "min").unwrap_or(0.);
    let default_maximum = if segment_total > 0. {
        minimum + segment_total
    } else {
        DEFAULT_GAUGE_MAX
    };
    let format = match string_field(value, "valueFormat") {
        Some(format) if format.eq_ignore_ascii_case("fraction") => GaugeValueFormat::Fraction,
        _ => GaugeValueFormat::Percentage,
    };
    ChartGauge {
        minimum,
        maximum: number_field(value, "max").unwrap_or(default_maximum),
        value: number_field(value, "value").unwrap_or(0.),
        segments,
        sub_label: text_field(value, "subLabel"),
        format,
        show_min_max: bool_field(value, "showMinMax", true),
    }
}

fn parse_display_mode(mode: Option<&str>) -> BarDisplayMode {
    match mode.map(str::to_ascii_lowercase).as_deref() {
        Some("absolutenoaxis") => BarDisplayMode::AbsoluteNoAxis,
        Some("parttowhole") => BarDisplayMode::PartToWhole,
        _ => BarDisplayMode::AbsoluteWithAxis,
    }
}

fn array_field<'value>(value: &'value Value, key: &str) -> &'value [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn string_field<'value>(value: &'value Value, key: &str) -> Option<&'value str> {
    value.get(key)?.as_str()
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    string_field(value, key)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn bool_field(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn number_field(value: &Value, key: &str) -> Option<f64> {
    value.get(key)?.as_f64().filter(|number| number.is_finite())
}

fn label_field(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.as_f64().map(format_chart_value).unwrap_or_default(),
        _ => String::new(),
    }
}

fn named_color(value: &Value) -> Option<ChartColor> {
    ChartColor::from_name(string_field(value, "color")?)
}

pub fn format_chart_value(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        let formatted = format!("{value:.2}");
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

pub fn slice_angles(values: &[f64]) -> Vec<(f32, f32)> {
    let total: f64 = values.iter().map(|value| value.max(0.)).sum();
    let mut start = -FRAC_PI_2;
    values
        .iter()
        .map(|value| {
            let sweep = if total > 0. {
                (value.max(0.) / total) as f32 * TAU
            } else {
                0.
            };
            let angles = (start, start + sweep);
            start += sweep;
            angles
        })
        .collect()
}

pub fn unit_fraction(value: f64, minimum: f64, maximum: f64) -> f32 {
    let range = maximum - minimum;
    if range <= 0. {
        return 0.;
    }
    ((value - minimum) / range).clamp(0., 1.) as f32
}

pub fn nice_scale(maximum_value: f64, target_steps: usize) -> AxisScale {
    if maximum_value <= 0. || target_steps == 0 {
        return AxisScale {
            maximum: 1.,
            step: 1.,
        };
    }
    let rough_step = maximum_value / target_steps as f64;
    let magnitude = 10f64.powf(rough_step.log10().floor());
    let normalized = rough_step / magnitude;
    let factor = [1., 2., 2.5, 5., 10.]
        .into_iter()
        .find(|candidate| *candidate >= normalized - 1e-9)
        .unwrap_or(10.);
    let step = factor * magnitude;
    AxisScale {
        maximum: (maximum_value / step - 1e-9).ceil().max(1.) * step,
        step,
    }
}

impl AxisScale {
    pub fn ticks(&self) -> Vec<f64> {
        let count = (self.maximum / self.step).round() as usize;
        (0..=count).map(|index| index as f64 * self.step).collect()
    }
}

pub fn label_stride(count: usize, slot_width: f32, label_width: f32) -> usize {
    if count == 0 || slot_width <= 0. {
        return 1;
    }
    (label_width / slot_width).ceil().max(1.) as usize
}

pub fn series_categories(series: &[ChartSeries]) -> Vec<String> {
    let mut categories: Vec<String> = Vec::new();
    for point in series.iter().flat_map(|entry| &entry.points) {
        if !categories.contains(&point.label) {
            categories.push(point.label.clone());
        }
    }
    categories
}

pub fn series_value(series: &ChartSeries, category: &str) -> f64 {
    series
        .points
        .iter()
        .find(|point| point.label == category)
        .map_or(0., |point| point.value)
}

pub fn series_maximum(series: &[ChartSeries], stacked: bool) -> f64 {
    if stacked {
        return series_categories(series)
            .iter()
            .map(|category| {
                series
                    .iter()
                    .map(|entry| series_value(entry, category).max(0.))
                    .sum::<f64>()
            })
            .fold(0., f64::max);
    }
    series
        .iter()
        .flat_map(|entry| &entry.points)
        .map(|point| point.value)
        .fold(0., f64::max)
}
