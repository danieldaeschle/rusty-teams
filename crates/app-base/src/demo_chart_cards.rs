use serde_json::{Value, json};

const FRUIT: [(&str, u32); 8] = [
    ("Pear", 59),
    ("Banana", 292),
    ("Apple", 143),
    ("Peach", 98),
    ("Kiwi", 179),
    ("Grapefruit", 50),
    ("Orange", 212),
    ("Cantaloupe", 68),
];

const PRODUCTS: [(&str, [u32; 8]); 4] = [
    ("Outlook", [24, 27, 18, 30, 20, 35, 40, 45]),
    ("Teams", [9, 100, 22, 40, 30, 45, 50, 55]),
    ("Office", [10, 20, 30, 40, 50, 60, 70, 80]),
    ("Windows", [10, 20, 30, 40, 50, 60, 70, 80]),
];

const TREND: [(&str, [u32; 6]); 5] = [
    ("Outlook", [99, 6, 63, 64, 63, 78]),
    ("Teams", [12, 82, 12, 33, 1, 80]),
    ("Office", [66, 93, 65, 13, 90, 48]),
    ("Windows", [9, 19, 0, 61, 21, 72]),
    ("Exchange", [35, 11, 91, 97, 97, 45]),
];

pub fn cards() -> Vec<Value> {
    vec![
        card(vec![slice_chart("Chart.Donut", "Fruit sales")]),
        card(vec![slice_chart("Chart.Pie", "Fruit sales")]),
        card(vec![
            text("Basic"),
            json!({"type": "Chart.Gauge", "value": 50, "subLabel": "Risk score", "segments": [
                {"legend": "Low risk", "size": 33, "color": "good"},
                {"legend": "Medium risk", "size": 34, "color": "warning"},
                {"legend": "High risk", "size": 33, "color": "attention"}
            ]}),
            text("Single value"),
            json!({"type": "Chart.Gauge", "value": 35, "valueFormat": "fraction", "segments": [
                {"legend": "Used", "size": 35},
                {"legend": "Unused", "size": 65, "color": "neutral"}
            ]}),
        ]),
        card(vec![json!({
            "type": "Chart.Line", "title": "Daily usage", "xAxisTitle": "Days", "yAxisTitle": "Sales",
            "colorSet": "categorical", "data": series_data(&TREND)
        })]),
        card(vec![json!({
            "type": "Chart.VerticalBar", "title": "Fruit sales", "xAxisTitle": "Fruit", "yAxisTitle": "Crates",
            "showBarValues": true, "colorSet": "categorical", "data": xy_data(&FRUIT)
        })]),
        card(vec![
            json!({
                "type": "Chart.VerticalBar.Grouped", "title": "Grouped", "xAxisTitle": "Days", "yAxisTitle": "Sales",
                "colorSet": "diverging", "data": series_data(&PRODUCTS)
            }),
            json!({
                "type": "Chart.VerticalBar.Grouped", "title": "Stacked", "stacked": true, "xAxisTitle": "Days",
                "yAxisTitle": "Sales", "data": series_data(&PRODUCTS)
            }),
        ]),
        card(vec![
            text("AbsoluteWithAxis"),
            json!({
                "type": "Chart.HorizontalBar", "title": "Fruit sales", "xAxisTitle": "Fruit", "yAxisTitle": "Crates",
                "colorSet": "diverging", "data": xy_data(&FRUIT[..6])
            }),
            text("AbsoluteNoAxis"),
            json!({
                "type": "Chart.HorizontalBar", "title": "Fruit sales", "displayMode": "AbsoluteNoAxis",
                "data": xy_data(&FRUIT[..6])
            }),
            text("PartToWhole"),
            json!({"type": "Chart.HorizontalBar", "title": "Learning goal", "displayMode": "PartToWhole",
                "color": "categoricalPurple", "data": [
                    {"x": "Yes, I have defined my day of learning goal", "y": 15},
                    {"x": "No, I haven't yet had time to do it", "y": 24},
                    {"x": "I am not interested in learning", "y": 2}
            ]}),
        ]),
        card(vec![json!({
            "type": "Chart.HorizontalBar.Stacked", "title": "Sample", "showBarValues": true, "data": [
                stacked_row("Outlook", [24, 27, 18]),
                stacked_row("Teams", [9, 100, 22]),
                stacked_row("Office", [40, 12, 60])
            ]
        })]),
    ]
}

fn card(body: Vec<Value>) -> Value {
    json!({"type": "AdaptiveCard", "version": "1.5", "body": body})
}

fn text(label: &str) -> Value {
    json!({"type": "TextBlock", "text": label, "size": "medium", "weight": "bolder", "separator": true, "spacing": "large"})
}

fn slice_chart(chart_type: &str, title: &str) -> Value {
    let data: Vec<Value> = FRUIT
        .iter()
        .map(|(legend, value)| json!({"legend": legend, "value": value}))
        .collect();
    json!({"type": chart_type, "title": title, "colorSet": "categorical", "data": data})
}

fn xy_data(points: &[(&str, u32)]) -> Vec<Value> {
    points
        .iter()
        .map(|(label, value)| json!({"x": label, "y": value}))
        .collect()
}

fn series_data<const COUNT: usize>(series: &[(&str, [u32; COUNT])]) -> Vec<Value> {
    series
        .iter()
        .map(|(legend, values)| {
            let points: Vec<Value> = values
                .iter()
                .enumerate()
                .map(|(index, value)| json!({"x": format!("2023-05-{:02}", index + 1), "y": value}))
                .collect();
            json!({"legend": legend, "values": points})
        })
        .collect()
}

fn stacked_row(title: &str, values: [u32; 3]) -> Value {
    let colors = ["good", "warning", "attention"];
    let points: Vec<Value> = values
        .iter()
        .zip(colors)
        .enumerate()
        .map(|(index, (value, color))| {
            json!({"legend": format!("2023-05-{:02}", index + 1), "value": value, "color": color})
        })
        .collect();
    json!({"title": title, "data": points})
}
