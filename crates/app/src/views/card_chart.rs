use std::collections::HashSet;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{
    BarDisplayMode, CardChart, ChartColor, ChartGauge, ChartKind, ChartPoint, ChartSeries,
    GaugeValueFormat, format_chart_value, label_stride, nice_scale, series_categories,
    series_maximum, series_value, slice_angles, unit_fraction,
};

use crate::theme;

const CHART_HEIGHT: f32 = 200.;
const GAUGE_HEIGHT: f32 = 136.;
const GAUGE_MAX_RADIUS: f32 = 100.;
const GAUGE_RING_RATIO: f32 = 0.72;
const GAUGE_VALUE_SIZE: f32 = 22.;
const GAUGE_MARKER_RADIUS: f32 = 6.;
const DONUT_HOLE_RATIO: f32 = 0.58;
const SLICE_GAP: f32 = 2.;
const SLICE_LABEL_MIN_SWEEP: f32 = 0.32;
const ARC_STEP: f32 = 0.05;
const TITLE_SIZE: f32 = 13.5;
const LABEL_SIZE: f32 = 11.;
const LABEL_LINE_HEIGHT: f32 = 14.;
const LEGEND_SIZE: f32 = 12.;
const LEGEND_DOT: f32 = 8.;
const LEGEND_GAP: f32 = 12.;
const SECTION_GAP: f32 = 8.;
const AXIS_TICK_TARGET: usize = 4;
const AXIS_LABEL_GAP: f32 = 6.;
const AXIS_TITLE_HEIGHT: f32 = 16.;
const CATEGORY_LABEL_HEIGHT: f32 = 20.;
const FRAME_PADDING: f32 = 8.;
const VALUE_GUTTER: f32 = 40.;
const LABEL_COLUMN_MAX_RATIO: f32 = 0.4;
const BAR_SLOT_RATIO: f32 = 0.64;
const BAR_MAX_WIDTH: f32 = 56.;
const BAR_RADIUS: f32 = 2.;
const GROUP_GAP: f32 = 2.;
const HORIZONTAL_ROW_HEIGHT: f32 = 30.;
const HORIZONTAL_BAR_HEIGHT: f32 = 18.;
const WHOLE_ROW_HEIGHT: f32 = 46.;
const WHOLE_BAR_HEIGHT: f32 = 10.;
const SEGMENT_LABEL_MIN_WIDTH: f32 = 26.;
const LINE_WIDTH: f32 = 2.;
const MARKER_RADIUS: f32 = 3.5;
const GRID_WIDTH: f32 = 1.;
const ELLIPSIS: &str = "\u{2026}";
const LIGHT_FILL_LUMINANCE: f32 = 0.62;
const DARK_LABEL: u32 = 0x161617;

#[derive(Clone, Copy)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Rect {
    fn from_bounds(bounds: Bounds<Pixels>) -> Rect {
        Rect {
            left: f32::from(bounds.origin.x),
            top: f32::from(bounds.origin.y),
            right: f32::from(bounds.origin.x + bounds.size.width),
            bottom: f32::from(bounds.origin.y + bounds.size.height),
        }
    }

    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.bottom - self.top
    }

    fn center_x(&self) -> f32 {
        (self.left + self.right) / 2.
    }

    fn center_y(&self) -> f32 {
        (self.top + self.bottom) / 2.
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.left), px(self.top)),
            size(px(self.width().max(0.)), px(self.height().max(0.))),
        )
    }
}

#[derive(Clone, Copy)]
enum LabelAlign {
    Left,
    Center,
    Right,
}

struct Pen<'a> {
    window: &'a mut Window,
    cx: &'a mut App,
}

impl Pen<'_> {
    fn shape(&self, text: &str, size_pixels: f32, color: Hsla, weight: FontWeight) -> ShapedLine {
        let mut font = self.window.text_style().font();
        font.weight = weight;
        let run = TextRun {
            len: text.len(),
            font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        self.window
            .text_system()
            .shape_line(text.to_owned().into(), px(size_pixels), &[run], None)
    }

    fn text_width(&self, text: &str) -> f32 {
        f32::from(
            self.shape(text, LABEL_SIZE, theme::text_muted(), FontWeight::NORMAL)
                .width,
        )
    }

    fn text(&mut self, text: &str, x: f32, y: f32, align: LabelAlign, color: Hsla) {
        self.sized_text(text, (x, y), align, color, LABEL_SIZE, FontWeight::NORMAL);
    }

    fn sized_text(
        &mut self,
        text: &str,
        (x, y): (f32, f32),
        align: LabelAlign,
        color: Hsla,
        size_pixels: f32,
        weight: FontWeight,
    ) {
        if text.is_empty() {
            return;
        }
        let line = self.shape(text, size_pixels, color, weight);
        let width = f32::from(line.width);
        let left = match align {
            LabelAlign::Left => x,
            LabelAlign::Center => x - width / 2.,
            LabelAlign::Right => x - width,
        };
        let _ = line.paint(
            point(px(left), px(y)),
            px(size_pixels * 1.25),
            TextAlign::Left,
            None,
            self.window,
            self.cx,
        );
    }

    fn fitted(&self, text: &str, max_width: f32) -> String {
        if self.text_width(text) <= max_width {
            return text.to_owned();
        }
        let characters: Vec<char> = text.chars().collect();
        (1..characters.len())
            .rev()
            .map(|length| {
                format!(
                    "{}{ELLIPSIS}",
                    characters[..length].iter().collect::<String>()
                )
            })
            .find(|candidate| self.text_width(candidate) <= max_width)
            .unwrap_or_else(|| ELLIPSIS.to_owned())
    }

    fn rect(&mut self, rect: Rect, color: Hsla, radius: f32) {
        if rect.width() <= 0. || rect.height() <= 0. {
            return;
        }
        self.window.paint_quad(quad(
            rect.bounds(),
            px(radius),
            color,
            px(0.),
            transparent_black(),
            BorderStyle::default(),
        ));
    }

    fn circle(&mut self, x: f32, y: f32, radius: f32, color: Hsla) {
        self.rect(
            Rect {
                left: x - radius,
                top: y - radius,
                right: x + radius,
                bottom: y + radius,
            },
            color,
            radius,
        );
    }

    fn horizontal_line(&mut self, left: f32, right: f32, y: f32, color: Hsla) {
        self.rect(
            Rect {
                left,
                top: y - GRID_WIDTH / 2.,
                right,
                bottom: y + GRID_WIDTH / 2.,
            },
            color,
            0.,
        );
    }

    fn vertical_line(&mut self, x: f32, top: f32, bottom: f32, color: Hsla) {
        self.rect(
            Rect {
                left: x - GRID_WIDTH / 2.,
                top,
                right: x + GRID_WIDTH / 2.,
                bottom,
            },
            color,
            0.,
        );
    }

    fn polyline(&mut self, points: &[(f32, f32)], color: Hsla) {
        let Some(((first_x, first_y), rest)) = points.split_first() else {
            return;
        };
        let mut builder = PathBuilder::stroke(px(LINE_WIDTH));
        builder.move_to(point(px(*first_x), px(*first_y)));
        for (x, y) in rest {
            builder.line_to(point(px(*x), px(*y)));
        }
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, color);
        }
    }

    fn sector(&mut self, center: (f32, f32), radii: (f32, f32), angles: (f32, f32), color: Hsla) {
        let (inner, outer) = radii;
        let (start, end) = angles;
        let steps = ((end - start).abs() / ARC_STEP).ceil().max(1.) as usize;
        let arc_point = |radius: f32, step: usize| {
            let angle = start + (end - start) * step as f32 / steps as f32;
            point(
                px(center.0 + radius * angle.cos()),
                px(center.1 + radius * angle.sin()),
            )
        };
        let mut builder = PathBuilder::fill();
        builder.move_to(arc_point(outer, 0));
        for step in 1..=steps {
            builder.line_to(arc_point(outer, step));
        }
        if inner > 0. {
            for step in (0..=steps).rev() {
                builder.line_to(arc_point(inner, step));
            }
        } else {
            builder.line_to(point(px(center.0), px(center.1)));
        }
        builder.close();
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, color);
        }
    }
}

pub fn chart_view(chart: &CardChart) -> AnyElement {
    let legend_entries = if chart.show_legend {
        chart.legend_entries()
    } else {
        Vec::new()
    };
    let height = chart_height(chart);
    let drawing_chart = chart.clone();
    let drawing = canvas(
        |_, _, _| (),
        move |bounds, (), window, cx| {
            paint_chart(&drawing_chart, bounds, &mut Pen { window, cx });
        },
    )
    .h(px(height));
    let side_legend = matches!(chart.kind, ChartKind::Donut(_) | ChartKind::Pie(_));
    let body = if side_legend {
        h_flex()
            .w_full()
            .items_center()
            .gap(px(LEGEND_GAP))
            .child(drawing.flex_1().min_w(px(0.)))
            .child(legend_view(&legend_entries, true))
            .into_any_element()
    } else {
        v_flex()
            .w_full()
            .gap(px(SECTION_GAP))
            .child(drawing.w_full())
            .child(legend_view(&legend_entries, false))
            .into_any_element()
    };
    v_flex()
        .w_full()
        .gap(px(SECTION_GAP))
        .when_some(chart.title.clone(), |column, title| {
            column.child(
                div()
                    .text_size(px(TITLE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_strong())
                    .child(title),
            )
        })
        .child(body)
        .into_any_element()
}

fn legend_view(entries: &[(String, ChartColor)], vertical: bool) -> Div {
    let container = if vertical {
        v_flex().flex_none().gap(px(6.))
    } else {
        h_flex().flex_wrap().gap_x(px(LEGEND_GAP)).gap_y(px(4.))
    };
    container.children(entries.iter().map(|(label, color)| {
        h_flex()
            .gap(px(6.))
            .items_center()
            .text_size(px(LEGEND_SIZE))
            .text_color(theme::text_muted())
            .child(
                div()
                    .flex_none()
                    .size(px(LEGEND_DOT))
                    .rounded(px(LEGEND_DOT / 2.))
                    .bg(palette(*color)),
            )
            .child(label.clone())
    }))
}

fn chart_height(chart: &CardChart) -> f32 {
    let title_rows = |has_axis_title: bool| {
        if has_axis_title {
            AXIS_TITLE_HEIGHT
        } else {
            0.
        }
    };
    match &chart.kind {
        ChartKind::Gauge(_) => GAUGE_HEIGHT,
        ChartKind::HorizontalBar { bars, mode } => match mode {
            BarDisplayMode::AbsoluteWithAxis => {
                bars.len() as f32 * HORIZONTAL_ROW_HEIGHT
                    + CATEGORY_LABEL_HEIGHT
                    + title_rows(chart.y_axis_title.is_some())
                    + title_rows(chart.x_axis_title.is_some())
                    + FRAME_PADDING
            }
            BarDisplayMode::AbsoluteNoAxis => {
                bars.len() as f32 * HORIZONTAL_ROW_HEIGHT
                    + title_rows(chart.x_axis_title.is_some())
                    + FRAME_PADDING
            }
            BarDisplayMode::PartToWhole => bars.len() as f32 * WHOLE_ROW_HEIGHT,
        },
        ChartKind::StackedHorizontalBar(rows) => {
            rows.len() as f32 * HORIZONTAL_ROW_HEIGHT
                + CATEGORY_LABEL_HEIGHT
                + title_rows(chart.y_axis_title.is_some())
                + title_rows(chart.x_axis_title.is_some())
                + FRAME_PADDING
        }
        _ => CHART_HEIGHT,
    }
}

fn paint_chart(chart: &CardChart, bounds: Bounds<Pixels>, pen: &mut Pen) {
    let area = Rect::from_bounds(bounds);
    match &chart.kind {
        ChartKind::Donut(slices) => paint_slices(pen, slices, area, DONUT_HOLE_RATIO),
        ChartKind::Pie(slices) => paint_slices(pen, slices, area, 0.),
        ChartKind::Gauge(gauge) => paint_gauge(pen, gauge, area),
        ChartKind::Line(series) => paint_lines(pen, chart, series, area),
        ChartKind::VerticalBar(bars) => paint_vertical_bars(pen, chart, bars, area),
        ChartKind::GroupedBar { series, stacked } => {
            paint_grouped_bars(pen, chart, series, *stacked, area)
        }
        ChartKind::HorizontalBar { bars, mode } => match mode {
            BarDisplayMode::PartToWhole => paint_part_to_whole(pen, bars, area),
            _ => paint_horizontal_bars(pen, chart, bars, *mode, area),
        },
        ChartKind::StackedHorizontalBar(rows) => paint_stacked_rows(pen, chart, rows, area),
    }
}

fn paint_slices(pen: &mut Pen, slices: &[ChartPoint], area: Rect, hole_ratio: f32) {
    let radius = area.width().min(area.height()) / 2. - FRAME_PADDING / 2.;
    if radius <= 0. {
        return;
    }
    let center = (area.center_x(), area.center_y());
    let inner = radius * hole_ratio;
    let values: Vec<f64> = slices.iter().map(|slice| slice.value).collect();
    let visible = values.iter().filter(|value| **value > 0.).count();
    let gap = if visible > 1 { SLICE_GAP / radius } else { 0. };
    for (slice, (start, end)) in slices.iter().zip(slice_angles(&values)) {
        let sweep = end - start;
        if sweep <= 0. {
            continue;
        }
        let fill = palette(slice.color);
        let trim = gap.min(sweep / 4.);
        pen.sector(
            center,
            (inner, radius),
            (start + trim / 2., end - trim / 2.),
            fill,
        );
        if sweep >= SLICE_LABEL_MIN_SWEEP {
            let middle = (start + end) / 2.;
            let label_radius = (inner + radius) / 2.;
            pen.sized_text(
                &format_chart_value(slice.value),
                (
                    center.0 + label_radius * middle.cos(),
                    center.1 + label_radius * middle.sin() - LABEL_LINE_HEIGHT / 2.,
                ),
                LabelAlign::Center,
                label_color_on(fill),
                LABEL_SIZE,
                FontWeight::SEMIBOLD,
            );
        }
    }
}

fn paint_gauge(pen: &mut Pen, gauge: &ChartGauge, area: Rect) {
    let radius = (area.width() / 2. - FRAME_PADDING * 1.5).min(GAUGE_MAX_RADIUS);
    if radius <= 0. {
        return;
    }
    let center = (area.center_x(), area.top + FRAME_PADDING / 2. + radius);
    let inner = radius * GAUGE_RING_RATIO;
    let segment_total: f64 = gauge.segments.iter().map(|segment| segment.size).sum();
    let fraction = unit_fraction(gauge.value, gauge.minimum, gauge.maximum);
    if segment_total > 0. {
        let sizes: Vec<f64> = gauge.segments.iter().map(|segment| segment.size).collect();
        let gap = SLICE_GAP / radius;
        for (segment, (start, end)) in gauge.segments.iter().zip(slice_angles(&sizes)) {
            let (start, end) = (half_arc_angle(start), half_arc_angle(end));
            let trim = gap.min((end - start) / 4.);
            pen.sector(
                center,
                (inner, radius),
                (start + trim / 2., end - trim / 2.),
                palette(segment.color),
            );
        }
    } else {
        pen.sector(
            center,
            (inner, radius),
            (PI, TAU),
            palette(ChartColor::Neutral),
        );
        pen.sector(
            center,
            (inner, radius),
            (PI, PI + PI * fraction),
            palette(ChartColor::CategoricalBlue),
        );
    }
    let marker_angle = PI + PI * fraction;
    let marker_radius = (inner + radius) / 2.;
    let marker = (
        center.0 + marker_radius * marker_angle.cos(),
        center.1 + marker_radius * marker_angle.sin(),
    );
    pen.circle(
        marker.0,
        marker.1,
        GAUGE_MARKER_RADIUS,
        theme::text_strong(),
    );
    pen.circle(
        marker.0,
        marker.1,
        GAUGE_MARKER_RADIUS - 2.,
        theme::surface(),
    );
    let value_text = match gauge.format {
        GaugeValueFormat::Percentage => {
            format!("{}%", format_chart_value((fraction * 100.).round() as f64))
        }
        GaugeValueFormat::Fraction => format!(
            "{}/{}",
            format_chart_value(gauge.value),
            format_chart_value(gauge.maximum)
        ),
    };
    pen.sized_text(
        &value_text,
        (center.0, center.1 - GAUGE_VALUE_SIZE * 1.1),
        LabelAlign::Center,
        theme::text_strong(),
        GAUGE_VALUE_SIZE,
        FontWeight::SEMIBOLD,
    );
    if let Some(sub_label) = &gauge.sub_label {
        pen.text(
            sub_label,
            center.0,
            center.1 + 4.,
            LabelAlign::Center,
            theme::text_muted(),
        );
    }
    if gauge.show_min_max {
        let end_offset = marker_radius;
        let y = center.1 + 4.;
        pen.text(
            &format_chart_value(gauge.minimum),
            center.0 - end_offset,
            y,
            LabelAlign::Center,
            theme::text_muted(),
        );
        pen.text(
            &format_chart_value(gauge.maximum),
            center.0 + end_offset,
            y,
            LabelAlign::Center,
            theme::text_muted(),
        );
    }
}

struct VerticalFrame {
    plot: Rect,
    slot_width: f32,
    maximum: f64,
}

impl VerticalFrame {
    fn slot_center(&self, index: usize) -> f32 {
        self.plot.left + self.slot_width * (index as f32 + 0.5)
    }

    fn y(&self, value: f64) -> f32 {
        self.plot.bottom - self.plot.height() * unit_fraction(value, 0., self.maximum)
    }
}

fn vertical_frame(
    pen: &mut Pen,
    chart: &CardChart,
    area: Rect,
    categories: &[String],
    maximum_value: f64,
) -> VerticalFrame {
    let scale = nice_scale(maximum_value, AXIS_TICK_TARGET);
    let ticks = scale.ticks();
    let tick_labels: Vec<String> = ticks.iter().map(|tick| format_chart_value(*tick)).collect();
    let label_width = tick_labels
        .iter()
        .map(|label| pen.text_width(label))
        .fold(0., f32::max);
    let top_padding = if chart.y_axis_title.is_some() {
        AXIS_TITLE_HEIGHT + FRAME_PADDING
    } else {
        FRAME_PADDING
    };
    let bottom_padding = CATEGORY_LABEL_HEIGHT
        + if chart.x_axis_title.is_some() {
            AXIS_TITLE_HEIGHT
        } else {
            0.
        };
    let plot = Rect {
        left: area.left + label_width + AXIS_LABEL_GAP + FRAME_PADDING / 2.,
        top: area.top + top_padding,
        right: area.right - FRAME_PADDING,
        bottom: area.bottom - bottom_padding,
    };
    let frame = VerticalFrame {
        plot,
        slot_width: plot.width() / categories.len().max(1) as f32,
        maximum: scale.maximum,
    };
    if let Some(title) = &chart.y_axis_title {
        pen.text(
            title,
            area.left,
            area.top,
            LabelAlign::Left,
            theme::text_muted(),
        );
    }
    for (tick, label) in ticks.iter().zip(&tick_labels) {
        let y = frame.y(*tick);
        let line_color = if *tick == 0. {
            theme::border_strong()
        } else {
            theme::border()
        };
        pen.horizontal_line(plot.left, plot.right, y, line_color);
        pen.text(
            label,
            plot.left - AXIS_LABEL_GAP,
            y - LABEL_LINE_HEIGHT / 2.,
            LabelAlign::Right,
            theme::text_muted(),
        );
    }
    let widest_category = categories
        .iter()
        .map(|category| pen.text_width(category))
        .fold(0., f32::max);
    let stride = label_stride(
        categories.len(),
        frame.slot_width,
        widest_category + AXIS_LABEL_GAP,
    );
    let fitted: Vec<String> = categories
        .iter()
        .map(|category| pen.fitted(category, frame.slot_width - AXIS_LABEL_GAP))
        .collect();
    let fitted_are_unique = fitted.iter().collect::<HashSet<_>>().len() == categories.len();
    let (labels, stride) = if stride > 1 && fitted_are_unique {
        (fitted.as_slice(), 1)
    } else {
        (categories, stride)
    };
    for (index, label) in labels.iter().enumerate().step_by(stride) {
        pen.text(
            label,
            frame.slot_center(index),
            plot.bottom + 4.,
            LabelAlign::Center,
            theme::text_muted(),
        );
    }
    if let Some(title) = &chart.x_axis_title {
        pen.text(
            title,
            plot.center_x(),
            area.bottom - AXIS_TITLE_HEIGHT + 2.,
            LabelAlign::Center,
            theme::text_muted(),
        );
    }
    frame
}

fn paint_bar_value(pen: &mut Pen, value: f64, x: f32, top: f32) {
    pen.text(
        &format_chart_value(value),
        x,
        top - LABEL_LINE_HEIGHT - 1.,
        LabelAlign::Center,
        theme::text_strong(),
    );
}

fn paint_vertical_bars(pen: &mut Pen, chart: &CardChart, bars: &[ChartPoint], area: Rect) {
    let categories: Vec<String> = bars.iter().map(|bar| bar.label.clone()).collect();
    let maximum = bars.iter().map(|bar| bar.value).fold(0., f64::max);
    let frame = vertical_frame(pen, chart, area, &categories, maximum);
    let bar_width = (frame.slot_width * BAR_SLOT_RATIO).min(BAR_MAX_WIDTH);
    for (index, bar) in bars.iter().enumerate() {
        let center = frame.slot_center(index);
        let top = frame.y(bar.value);
        pen.rect(
            Rect {
                left: center - bar_width / 2.,
                top,
                right: center + bar_width / 2.,
                bottom: frame.plot.bottom,
            },
            palette(bar.color),
            BAR_RADIUS,
        );
        if chart.show_values {
            paint_bar_value(pen, bar.value, center, top);
        }
    }
}

fn paint_grouped_bars(
    pen: &mut Pen,
    chart: &CardChart,
    series: &[ChartSeries],
    stacked: bool,
    area: Rect,
) {
    let categories = series_categories(series);
    let frame = vertical_frame(
        pen,
        chart,
        area,
        &categories,
        series_maximum(series, stacked),
    );
    let group_width =
        (frame.slot_width * BAR_SLOT_RATIO).min(BAR_MAX_WIDTH * series.len().max(1) as f32);
    let bar_width = if stacked {
        group_width.min(BAR_MAX_WIDTH)
    } else {
        group_width / series.len().max(1) as f32
    };
    for (index, category) in categories.iter().enumerate() {
        let center = frame.slot_center(index);
        let mut stack_total = 0.;
        for (series_index, entry) in series.iter().enumerate() {
            let value = series_value(entry, category).max(0.);
            let (left, bottom_value, top_value) = if stacked {
                let base = stack_total;
                stack_total += value;
                (center - bar_width / 2., base, stack_total)
            } else {
                let left = center - group_width / 2. + bar_width * series_index as f32;
                (left, 0., value)
            };
            let gap = if stacked { 0. } else { GROUP_GAP / 2. };
            pen.rect(
                Rect {
                    left: left + gap,
                    top: frame.y(top_value),
                    right: left + bar_width - gap,
                    bottom: frame.y(bottom_value),
                },
                palette(entry.color),
                if stacked { 0. } else { BAR_RADIUS },
            );
            if chart.show_values && !stacked {
                paint_bar_value(pen, value, left + bar_width / 2., frame.y(value));
            }
        }
    }
}

fn paint_lines(pen: &mut Pen, chart: &CardChart, series: &[ChartSeries], area: Rect) {
    let categories = series_categories(series);
    let frame = vertical_frame(pen, chart, area, &categories, series_maximum(series, false));
    for entry in series {
        let color = palette(entry.color);
        let points: Vec<(f32, f32)> = categories
            .iter()
            .enumerate()
            .filter(|(_, category)| entry.points.iter().any(|point| point.label == **category))
            .map(|(index, category)| {
                (
                    frame.slot_center(index),
                    frame.y(series_value(entry, category)),
                )
            })
            .collect();
        pen.polyline(&points, color);
        for (x, y) in &points {
            pen.circle(*x, *y, MARKER_RADIUS + 1.5, theme::surface());
            pen.circle(*x, *y, MARKER_RADIUS, color);
        }
    }
}

struct HorizontalFrame {
    plot: Rect,
    row_height: f32,
    maximum: f64,
}

impl HorizontalFrame {
    fn row_center(&self, index: usize) -> f32 {
        self.plot.top + self.row_height * (index as f32 + 0.5)
    }

    fn x(&self, value: f64) -> f32 {
        self.plot.left + self.plot.width() * unit_fraction(value, 0., self.maximum)
    }
}

fn horizontal_frame(
    pen: &mut Pen,
    chart: &CardChart,
    area: Rect,
    labels: &[String],
    maximum_value: f64,
    with_axis: bool,
) -> HorizontalFrame {
    let scale = nice_scale(maximum_value, AXIS_TICK_TARGET);
    let widest_label = labels
        .iter()
        .map(|label| pen.text_width(label))
        .fold(0., f32::max);
    let label_width = widest_label.min(area.width() * LABEL_COLUMN_MAX_RATIO);
    let top_padding = if chart.x_axis_title.is_some() {
        AXIS_TITLE_HEIGHT
    } else {
        FRAME_PADDING / 2.
    };
    let bottom_padding = if with_axis {
        CATEGORY_LABEL_HEIGHT
            + if chart.y_axis_title.is_some() {
                AXIS_TITLE_HEIGHT
            } else {
                0.
            }
    } else {
        FRAME_PADDING / 2.
    };
    let plot = Rect {
        left: area.left + label_width + AXIS_LABEL_GAP * 2.,
        top: area.top + top_padding,
        right: area.right - VALUE_GUTTER,
        bottom: area.bottom - bottom_padding,
    };
    let frame = HorizontalFrame {
        plot,
        row_height: plot.height() / labels.len().max(1) as f32,
        maximum: scale.maximum,
    };
    if let Some(title) = &chart.x_axis_title {
        pen.text(
            title,
            area.left,
            area.top,
            LabelAlign::Left,
            theme::text_muted(),
        );
    }
    if with_axis {
        for tick in scale.ticks() {
            let x = frame.x(tick);
            let line_color = if tick == 0. {
                theme::border_strong()
            } else {
                theme::border()
            };
            pen.vertical_line(x, plot.top, plot.bottom, line_color);
            pen.text(
                &format_chart_value(tick),
                x,
                plot.bottom + 4.,
                LabelAlign::Center,
                theme::text_muted(),
            );
        }
        if let Some(title) = &chart.y_axis_title {
            pen.text(
                title,
                plot.center_x(),
                area.bottom - AXIS_TITLE_HEIGHT + 2.,
                LabelAlign::Center,
                theme::text_muted(),
            );
        }
    }
    for (index, label) in labels.iter().enumerate() {
        let fitted = pen.fitted(label, label_width);
        pen.text(
            &fitted,
            plot.left - AXIS_LABEL_GAP,
            frame.row_center(index) - LABEL_LINE_HEIGHT / 2.,
            LabelAlign::Right,
            theme::text_soft(),
        );
    }
    frame
}

fn paint_horizontal_bars(
    pen: &mut Pen,
    chart: &CardChart,
    bars: &[ChartPoint],
    mode: BarDisplayMode,
    area: Rect,
) {
    let labels: Vec<String> = bars.iter().map(|bar| bar.label.clone()).collect();
    let maximum = bars.iter().map(|bar| bar.value).fold(0., f64::max);
    let with_axis = mode == BarDisplayMode::AbsoluteWithAxis;
    let frame = horizontal_frame(pen, chart, area, &labels, maximum, with_axis);
    let bar_height = HORIZONTAL_BAR_HEIGHT.min(frame.row_height * 0.7);
    for (index, bar) in bars.iter().enumerate() {
        let center = frame.row_center(index);
        let right = frame.x(bar.value);
        pen.rect(
            Rect {
                left: frame.plot.left,
                top: center - bar_height / 2.,
                right,
                bottom: center + bar_height / 2.,
            },
            palette(bar.color),
            BAR_RADIUS,
        );
        if chart.show_values || !with_axis {
            pen.text(
                &format_chart_value(bar.value),
                right + 6.,
                center - LABEL_LINE_HEIGHT / 2.,
                LabelAlign::Left,
                theme::text_strong(),
            );
        }
    }
}

fn paint_part_to_whole(pen: &mut Pen, bars: &[ChartPoint], area: Rect) {
    let total: f64 = bars.iter().map(|bar| bar.value.max(0.)).sum();
    for (index, bar) in bars.iter().enumerate() {
        let top = area.top + WHOLE_ROW_HEIGHT * index as f32;
        let value_text = format_chart_value(bar.value);
        let value_width = pen.text_width(&value_text);
        let label = pen.fitted(&bar.label, area.width() - value_width - LEGEND_GAP);
        pen.text(&label, area.left, top, LabelAlign::Left, theme::text_soft());
        pen.text(
            &value_text,
            area.right,
            top,
            LabelAlign::Right,
            theme::text_strong(),
        );
        let track = Rect {
            left: area.left,
            top: top + LABEL_LINE_HEIGHT + 6.,
            right: area.right,
            bottom: top + LABEL_LINE_HEIGHT + 6. + WHOLE_BAR_HEIGHT,
        };
        pen.rect(track, theme::surface_raised(), WHOLE_BAR_HEIGHT / 2.);
        let share = unit_fraction(bar.value, 0., total);
        pen.rect(
            Rect {
                right: track.left + track.width() * share,
                ..track
            },
            palette(bar.color),
            WHOLE_BAR_HEIGHT / 2.,
        );
    }
}

fn paint_stacked_rows(pen: &mut Pen, chart: &CardChart, rows: &[ChartSeries], area: Rect) {
    let labels: Vec<String> = rows.iter().map(|row| row.legend.clone()).collect();
    let maximum = rows
        .iter()
        .map(|row| {
            row.points
                .iter()
                .map(|point| point.value.max(0.))
                .sum::<f64>()
        })
        .fold(0., f64::max);
    let frame = horizontal_frame(pen, chart, area, &labels, maximum, true);
    let bar_height = HORIZONTAL_BAR_HEIGHT.min(frame.row_height * 0.7);
    for (index, row) in rows.iter().enumerate() {
        let center = frame.row_center(index);
        let mut consumed = 0.;
        for point in &row.points {
            let start = frame.x(consumed);
            consumed += point.value.max(0.);
            let end = frame.x(consumed);
            let fill = palette(point.color);
            pen.rect(
                Rect {
                    left: start,
                    top: center - bar_height / 2.,
                    right: (end - 1.).max(start),
                    bottom: center + bar_height / 2.,
                },
                fill,
                0.,
            );
            if chart.show_values && end - start >= SEGMENT_LABEL_MIN_WIDTH {
                pen.text(
                    &format_chart_value(point.value),
                    (start + end) / 2.,
                    center - LABEL_LINE_HEIGHT / 2.,
                    LabelAlign::Center,
                    label_color_on(fill),
                );
            }
        }
    }
}

fn half_arc_angle(full_circle_angle: f32) -> f32 {
    PI + (full_circle_angle + FRAC_PI_2) / 2.
}

fn label_color_on(fill: Hsla) -> Hsla {
    if fill.l > LIGHT_FILL_LUMINANCE {
        theme::color(DARK_LABEL)
    } else {
        theme::white()
    }
}

fn palette(color: ChartColor) -> Hsla {
    theme::color(match color {
        ChartColor::Good => 0x3fb56f,
        ChartColor::Warning => 0xf2b33d,
        ChartColor::Attention => 0xe5484d,
        ChartColor::Neutral => 0x4a4a50,
        ChartColor::CategoricalRed => 0xe5484d,
        ChartColor::CategoricalPurple => 0x9b6bdc,
        ChartColor::CategoricalLavender => 0xb4a7f5,
        ChartColor::CategoricalBlue => 0x4f8cff,
        ChartColor::CategoricalLightBlue => 0x7cc4f7,
        ChartColor::CategoricalTeal => 0x2fb8a6,
        ChartColor::CategoricalGreen => 0x4cb86a,
        ChartColor::CategoricalLime => 0xa5d24a,
        ChartColor::CategoricalMarigold => 0xf0a830,
        ChartColor::Sequential1 => 0xc7dbff,
        ChartColor::Sequential2 => 0xa3c4fb,
        ChartColor::Sequential3 => 0x7eabf6,
        ChartColor::Sequential4 => 0x5b92ee,
        ChartColor::Sequential5 => 0x3f79e0,
        ChartColor::Sequential6 => 0x2e61c8,
        ChartColor::Sequential7 => 0x2a4fa3,
        ChartColor::Sequential8 => 0x253f80,
        ChartColor::DivergingBlue => 0x3b6fd6,
        ChartColor::DivergingLightBlue => 0x7fb2f0,
        ChartColor::DivergingCyan => 0x4cc3d9,
        ChartColor::DivergingTeal => 0x2fa89a,
        ChartColor::DivergingYellow => 0xe8d44d,
        ChartColor::DivergingPeach => 0xf2a679,
        ChartColor::DivergingLightRed => 0xea7a72,
        ChartColor::DivergingRed => 0xd64545,
        ChartColor::DivergingMaroon => 0x962b3a,
        ChartColor::DivergingGray => 0x8a8a92,
    })
}
