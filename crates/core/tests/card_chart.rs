use serde_json::{Value, json};
use teams_core::{
    AdaptiveCard, BarDisplayMode, CardChart, CardElement, ChartColor, ChartKind, GaugeValueFormat,
    format_chart_value, label_stride, nice_scale, series_categories, series_maximum, slice_angles,
    unit_fraction,
};

fn parse_chart(element: Value) -> CardChart {
    let card = json!({"type": "AdaptiveCard", "version": "1.5", "body": [element]});
    let card = AdaptiveCard::parse(&card.to_string()).expect("card parses");
    match &card.items[0].element {
        CardElement::Chart(chart) => chart.clone(),
        other => panic!("expected chart, got {other:?}"),
    }
}

fn fruit_slices() -> Value {
    json!([
        {"legend": "Pear", "value": 59},
        {"legend": "Banana", "value": 292},
        {"legend": "Apple", "value": 143, "color": "good"}
    ])
}

fn days_series() -> Value {
    json!([
        {"legend": "Outlook", "values": [
            {"x": "2023-05-01", "y": 24}, {"x": "2023-05-02", "y": 27}, {"x": "2023-05-03", "y": 18}
        ]},
        {"legend": "Teams", "values": [
            {"x": "2023-05-01", "y": 9}, {"x": "2023-05-02", "y": 100}, {"x": "2023-05-03", "y": 22}
        ]}
    ])
}

#[test]
fn donut_parses_slices_and_explicit_color() {
    let chart =
        parse_chart(json!({"type": "Chart.Donut", "title": "Fruit", "data": fruit_slices()}));
    assert_eq!(chart.title.as_deref(), Some("Fruit"));
    let ChartKind::Donut(slices) = &chart.kind else {
        panic!("expected donut");
    };
    assert_eq!(slices.len(), 3);
    assert_eq!(slices[1].label, "Banana");
    assert_eq!(slices[1].value, 292.);
    assert_eq!(slices[2].color, ChartColor::Good);
    assert_ne!(slices[0].color, slices[1].color);
    assert!(chart.show_legend);
}

#[test]
fn pie_parses_slices() {
    let chart = parse_chart(
        json!({"type": "Chart.Pie", "colorSet": "categorical", "data": fruit_slices()}),
    );
    let ChartKind::Pie(slices) = &chart.kind else {
        panic!("expected pie");
    };
    assert_eq!(slices.len(), 3);
    assert_eq!(chart.legend_entries().len(), 3);
}

#[test]
fn gauge_parses_segments_and_defaults() {
    let chart = parse_chart(json!({
        "type": "Chart.Gauge", "value": 50, "subLabel": "CPU",
        "segments": [
            {"legend": "Low risk", "size": 33, "color": "good"},
            {"legend": "Medium risk", "size": 34, "color": "warning"},
            {"legend": "High risk", "size": 33, "color": "attention"}
        ]
    }));
    let ChartKind::Gauge(gauge) = &chart.kind else {
        panic!("expected gauge");
    };
    assert_eq!(gauge.value, 50.);
    assert_eq!(gauge.minimum, 0.);
    assert_eq!(gauge.maximum, 100.);
    assert_eq!(gauge.format, GaugeValueFormat::Percentage);
    assert_eq!(gauge.sub_label.as_deref(), Some("CPU"));
    assert!(gauge.show_min_max);
    assert_eq!(gauge.segments[2].color, ChartColor::Attention);
}

#[test]
fn gauge_reads_fraction_format_and_value_key() {
    let chart = parse_chart(json!({
        "type": "Chart.Gauge", "value": 35, "valueFormat": "fraction", "min": 10, "showLegend": false,
        "segments": [{"legend": "Used", "value": 35}, {"legend": "Unused", "value": 65, "color": "neutral"}]
    }));
    let ChartKind::Gauge(gauge) = &chart.kind else {
        panic!("expected gauge");
    };
    assert_eq!(gauge.format, GaugeValueFormat::Fraction);
    assert_eq!(gauge.maximum, 110.);
    assert_eq!(gauge.segments[0].size, 35.);
    assert_eq!(gauge.segments[1].color, ChartColor::Neutral);
    assert!(!chart.show_legend);
}

#[test]
fn line_parses_series_with_axis_titles() {
    let chart = parse_chart(json!({
        "type": "Chart.Line", "title": "Sample", "xAxisTitle": "Days", "yAxisTitle": "Sales",
        "colorSet": "categorical", "data": days_series()
    }));
    assert_eq!(chart.x_axis_title.as_deref(), Some("Days"));
    assert_eq!(chart.y_axis_title.as_deref(), Some("Sales"));
    let ChartKind::Line(series) = &chart.kind else {
        panic!("expected line");
    };
    assert_eq!(series.len(), 2);
    assert_eq!(series[1].legend, "Teams");
    assert_eq!(series[1].points[1].value, 100.);
    assert_eq!(series[0].points[0].color, series[0].color);
}

#[test]
fn line_accepts_numeric_x_values() {
    let chart = parse_chart(json!({
        "type": "Chart.Line",
        "data": [{"legend": "A", "values": [{"x": 1, "y": 2}, {"x": 2.5, "y": 3}]}]
    }));
    let ChartKind::Line(series) = &chart.kind else {
        panic!("expected line");
    };
    assert_eq!(series[0].points[0].label, "1");
    assert_eq!(series[0].points[1].label, "2.5");
}

#[test]
fn vertical_bar_parses_points_and_show_values() {
    let chart = parse_chart(json!({
        "type": "Chart.VerticalBar", "showBarValues": true, "color": "categoricalPurple",
        "data": [{"x": "Pear", "y": 59}, {"x": "Banana", "y": 292}]
    }));
    assert!(chart.show_values);
    let ChartKind::VerticalBar(bars) = &chart.kind else {
        panic!("expected vertical bar");
    };
    assert_eq!(bars[1].label, "Banana");
    assert_eq!(bars[1].color, ChartColor::CategoricalPurple);
}

#[test]
fn grouped_bar_parses_stacked_flag() {
    let grouped = parse_chart(json!({"type": "Chart.VerticalBar.Grouped", "data": days_series()}));
    let stacked = parse_chart(
        json!({"type": "Chart.VerticalBar.Grouped", "stacked": true, "colorSet": "diverging", "data": days_series()}),
    );
    assert!(matches!(
        grouped.kind,
        ChartKind::GroupedBar { stacked: false, .. }
    ));
    let ChartKind::GroupedBar {
        series,
        stacked: true,
    } = &stacked.kind
    else {
        panic!("expected stacked grouped bar");
    };
    assert_eq!(series.len(), 2);
    assert_eq!(stacked.legend_entries().len(), 2);
}

#[test]
fn horizontal_bar_parses_display_modes() {
    let data = json!([{"x": "Pear", "y": 59}]);
    for (name, expected) in [
        ("AbsoluteWithAxis", BarDisplayMode::AbsoluteWithAxis),
        ("AbsoluteNoAxis", BarDisplayMode::AbsoluteNoAxis),
        ("PartToWhole", BarDisplayMode::PartToWhole),
    ] {
        let chart =
            parse_chart(json!({"type": "Chart.HorizontalBar", "displayMode": name, "data": data}));
        let ChartKind::HorizontalBar { mode, .. } = chart.kind else {
            panic!("expected horizontal bar");
        };
        assert_eq!(mode, expected);
    }
    let chart = parse_chart(json!({"type": "Chart.HorizontalBar", "data": data}));
    assert!(matches!(
        chart.kind,
        ChartKind::HorizontalBar {
            mode: BarDisplayMode::AbsoluteWithAxis,
            ..
        }
    ));
}

#[test]
fn stacked_horizontal_bar_parses_rows_and_legend() {
    let chart = parse_chart(json!({
        "type": "Chart.HorizontalBar.Stacked", "title": "Sample",
        "data": [
            {"title": "Outlook", "data": [
                {"legend": "2023-05-01", "value": 24, "color": "good"},
                {"legend": "2023-05-02", "value": 27, "color": "warning"}
            ]},
            {"title": "Teams", "data": [
                {"legend": "2023-05-01", "value": 9, "color": "good"},
                {"legend": "2023-05-02", "value": 100, "color": "warning"}
            ]}
        ]
    }));
    let ChartKind::StackedHorizontalBar(rows) = &chart.kind else {
        panic!("expected stacked horizontal bar");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].legend, "Teams");
    assert_eq!(rows[1].points[1].value, 100.);
    assert_eq!(rows[1].points[1].color, ChartColor::Warning);
    assert_eq!(chart.legend_entries().len(), 2);
}

#[test]
fn unknown_chart_type_falls_back_to_nothing() {
    let card = json!({"type": "AdaptiveCard", "body": [{"type": "Chart.Radar", "data": []}]});
    assert!(AdaptiveCard::parse(&card.to_string()).is_none());
}

#[test]
fn slice_angles_cover_full_circle_in_order() {
    let angles = slice_angles(&[59., 292., 143.]);
    let (first_start, _) = angles[0];
    let (_, last_end) = angles[2];
    assert!((first_start + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    assert!((last_end - first_start - std::f32::consts::TAU).abs() < 1e-4);
    assert_eq!(angles[0].1, angles[1].0);
}

#[test]
fn slice_angles_ignore_negative_and_empty_totals() {
    let angles = slice_angles(&[-5., 10.]);
    assert_eq!(angles[0].0, angles[0].1);
    assert!((angles[1].1 - angles[1].0 - std::f32::consts::TAU).abs() < 1e-4);
    assert!(
        slice_angles(&[0., 0.])
            .iter()
            .all(|(start, end)| start == end)
    );
}

#[test]
fn unit_fraction_clamps_gauge_values() {
    assert_eq!(unit_fraction(50., 0., 100.), 0.5);
    assert_eq!(unit_fraction(-20., 0., 100.), 0.);
    assert_eq!(unit_fraction(250., 0., 100.), 1.);
    assert_eq!(unit_fraction(60., 20., 100.), 0.5);
    assert_eq!(unit_fraction(5., 10., 10.), 0.);
}

#[test]
fn nice_scale_rounds_up_to_round_steps() {
    let scale = nice_scale(292., 4);
    assert_eq!(scale.step, 100.);
    assert_eq!(scale.maximum, 300.);
    assert_eq!(scale.ticks(), vec![0., 100., 200., 300.]);
    let small = nice_scale(0.9, 4);
    assert!(small.maximum >= 0.9);
    assert_eq!(nice_scale(0., 4).maximum, 1.);
    assert_eq!(nice_scale(100., 4).maximum, 100.);
}

#[test]
fn bar_scaling_is_proportional_to_scale_maximum() {
    let scale = nice_scale(292., 4);
    assert!((unit_fraction(150., 0., scale.maximum) - 0.5).abs() < 1e-6);
    assert!(unit_fraction(292., 0., scale.maximum) < 1.);
}

#[test]
fn label_stride_skips_labels_that_do_not_fit() {
    assert_eq!(label_stride(8, 60., 40.), 1);
    assert_eq!(label_stride(8, 30., 62.), 3);
    assert_eq!(label_stride(0, 30., 62.), 1);
}

#[test]
fn series_helpers_union_categories_and_stack_totals() {
    let chart = parse_chart(json!({"type": "Chart.VerticalBar.Grouped", "data": days_series()}));
    let ChartKind::GroupedBar { series, .. } = &chart.kind else {
        panic!("expected grouped bar");
    };
    assert_eq!(series_categories(series).len(), 3);
    assert_eq!(series_maximum(series, false), 100.);
    assert_eq!(series_maximum(series, true), 127.);
}

#[test]
fn chart_values_format_without_trailing_zeros() {
    assert_eq!(format_chart_value(292.), "292");
    assert_eq!(format_chart_value(2.5), "2.5");
    assert_eq!(format_chart_value(0.333333), "0.33");
}
