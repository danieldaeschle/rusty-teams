use serde_json::Value;

use crate::adaptive_card::{
    CardAction, CardElement, CardItem, ColumnWidth, ContainerStyle, VerticalAlignment,
    parse_column_width, parse_container_style, parse_items, parse_select_action,
    parse_vertical_alignment, string_field,
};
use crate::card_layout::{HorizontalAlignment, parse_horizontal_alignment};

#[derive(Debug, Clone, PartialEq)]
pub struct CardTable {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub first_row_as_header: bool,
    pub show_grid_lines: bool,
    pub grid_style: ContainerStyle,
    pub horizontal_alignment: Option<HorizontalAlignment>,
    pub vertical_alignment: Option<VerticalAlignment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    pub width: ColumnWidth,
    pub horizontal_alignment: Option<HorizontalAlignment>,
    pub vertical_alignment: Option<VerticalAlignment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    pub style: ContainerStyle,
    pub horizontal_alignment: Option<HorizontalAlignment>,
    pub vertical_alignment: Option<VerticalAlignment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub items: Vec<CardItem>,
    pub style: ContainerStyle,
    pub vertical_alignment: Option<VerticalAlignment>,
    pub select_action: Option<CardAction>,
}

impl CardTable {
    pub fn cell_horizontal_alignment(
        &self,
        row: &TableRow,
        column_index: usize,
    ) -> HorizontalAlignment {
        row.horizontal_alignment
            .or_else(|| self.columns.get(column_index)?.horizontal_alignment)
            .or(self.horizontal_alignment)
            .unwrap_or_default()
    }

    pub fn cell_vertical_alignment(
        &self,
        row: &TableRow,
        cell: &TableCell,
        column_index: usize,
    ) -> VerticalAlignment {
        cell.vertical_alignment
            .or(row.vertical_alignment)
            .or_else(|| self.columns.get(column_index)?.vertical_alignment)
            .or(self.vertical_alignment)
            .unwrap_or_default()
    }
}

pub(crate) fn parse_table(value: &Value) -> Option<CardElement> {
    let rows: Vec<TableRow> = value
        .get("rows")?
        .as_array()?
        .iter()
        .filter_map(parse_row)
        .collect();
    if rows.is_empty() {
        return None;
    }
    let column_count = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    let declared: Vec<TableColumn> = value
        .get("columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(parse_column)
        .collect();
    let columns = (0..column_count.max(declared.len()))
        .map(|index| declared.get(index).cloned().unwrap_or_else(default_column))
        .collect();
    Some(CardElement::Table(CardTable {
        columns,
        rows,
        first_row_as_header: value
            .get("firstRowAsHeader")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        show_grid_lines: value
            .get("showGridLines")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        grid_style: parse_container_style(string_field(value, "gridStyle")),
        horizontal_alignment: parse_horizontal_alignment(string_field(
            value,
            "horizontalCellContentAlignment",
        )),
        vertical_alignment: parse_optional_vertical(value, "verticalCellContentAlignment"),
    }))
}

fn default_column() -> TableColumn {
    TableColumn {
        width: ColumnWidth::Weighted(1.),
        horizontal_alignment: None,
        vertical_alignment: None,
    }
}

fn parse_column(value: &Value) -> TableColumn {
    let width = value
        .get("width")
        .map_or(ColumnWidth::Weighted(1.), |width| {
            parse_column_width(Some(width))
        });
    TableColumn {
        width,
        horizontal_alignment: parse_horizontal_alignment(string_field(
            value,
            "horizontalCellContentAlignment",
        )),
        vertical_alignment: parse_optional_vertical(value, "verticalCellContentAlignment"),
    }
}

fn parse_row(value: &Value) -> Option<TableRow> {
    if value.get("isVisible").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let cells: Vec<TableCell> = value
        .get("cells")?
        .as_array()?
        .iter()
        .map(parse_cell)
        .collect();
    (!cells.is_empty()).then(|| TableRow {
        cells,
        style: parse_container_style(string_field(value, "style")),
        horizontal_alignment: parse_horizontal_alignment(string_field(
            value,
            "horizontalCellContentAlignment",
        )),
        vertical_alignment: parse_optional_vertical(value, "verticalCellContentAlignment"),
    })
}

fn parse_cell(value: &Value) -> TableCell {
    TableCell {
        items: parse_items(value.get("items")),
        style: parse_container_style(string_field(value, "style")),
        vertical_alignment: parse_optional_vertical(value, "verticalContentAlignment"),
        select_action: parse_select_action(value),
    }
}

fn parse_optional_vertical(value: &Value, key: &str) -> Option<VerticalAlignment> {
    string_field(value, key)?;
    Some(parse_vertical_alignment(string_field(value, key)))
}
