use serde::{Deserialize, Serialize};

use crate::markdown::escape_html;

pub const TABLE_MARK: char = '\u{FFFB}';
pub const PICKER_COLUMNS: usize = 7;
pub const PICKER_ROWS: usize = 5;
const EMPTY_CELL: &str = "&nbsp;";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    pub row: usize,
    pub column: usize,
}

impl Cell {
    pub fn new(row: usize, column: usize) -> Cell {
        Cell { row, column }
    }
}

/// A composer table: plain text cells, every row as wide as the first.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableEdit {
    InsertRowAbove,
    InsertRowBelow,
    InsertColumnLeft,
    InsertColumnRight,
    DeleteRow,
    DeleteColumn,
    DeleteTable,
}

impl TableEdit {
    pub const INSERTS: [TableEdit; 4] = [
        TableEdit::InsertRowAbove,
        TableEdit::InsertRowBelow,
        TableEdit::InsertColumnLeft,
        TableEdit::InsertColumnRight,
    ];
    pub const DELETES: [TableEdit; 3] = [
        TableEdit::DeleteRow,
        TableEdit::DeleteColumn,
        TableEdit::DeleteTable,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TableEdit::InsertRowAbove => "Insert row above",
            TableEdit::InsertRowBelow => "Insert row below",
            TableEdit::InsertColumnLeft => "Insert column to the left",
            TableEdit::InsertColumnRight => "Insert column to the right",
            TableEdit::DeleteRow => "Delete row",
            TableEdit::DeleteColumn => "Delete column",
            TableEdit::DeleteTable => "Delete table",
        }
    }
}

impl Table {
    pub fn new(rows: usize, columns: usize) -> Table {
        Table {
            rows: vec![vec![String::new(); columns.max(1)]; rows.max(1)],
        }
    }

    /// Short rows are filled up to the widest one.
    pub fn from_rows(mut rows: Vec<Vec<String>>) -> Table {
        rows.retain(|row| !row.is_empty());
        let columns = rows.iter().map(Vec::len).max().unwrap_or(1);
        for row in &mut rows {
            row.resize(columns, String::new());
        }
        if rows.is_empty() {
            return Table::new(1, 1);
        }
        Table { rows }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn column_count(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }

    pub fn cell(&self, cell: Cell) -> Option<&str> {
        self.rows
            .get(cell.row)?
            .get(cell.column)
            .map(String::as_str)
    }

    pub fn set_cell(&mut self, cell: Cell, text: &str) {
        if let Some(slot) = self
            .rows
            .get_mut(cell.row)
            .and_then(|row| row.get_mut(cell.column))
        {
            *slot = text.to_owned();
        }
    }

    pub fn last_cell(&self) -> Cell {
        Cell::new(self.row_count() - 1, self.column_count() - 1)
    }

    /// Tab: the next cell, and past the last one a new row (`true`) like Teams.
    pub fn next_cell(&mut self, cell: Cell) -> (Cell, bool) {
        if cell.column + 1 < self.column_count() {
            return (Cell::new(cell.row, cell.column + 1), false);
        }
        if cell.row + 1 < self.row_count() {
            return (Cell::new(cell.row + 1, 0), false);
        }
        self.rows.push(vec![String::new(); self.column_count()]);
        (Cell::new(cell.row + 1, 0), true)
    }

    /// Shift+Tab: the previous cell; the first cell stays.
    pub fn previous_cell(&self, cell: Cell) -> Cell {
        match (cell.row, cell.column) {
            (_, column) if column > 0 => Cell::new(cell.row, column - 1),
            (0, _) => cell,
            (row, _) => Cell::new(row - 1, self.column_count() - 1),
        }
    }

    /// The cell to focus afterwards; `None` when the table is gone.
    pub fn apply(&mut self, edit: TableEdit, cell: Cell) -> Option<Cell> {
        let columns = self.column_count();
        match edit {
            TableEdit::InsertRowAbove => {
                self.rows.insert(cell.row, vec![String::new(); columns]);
                Some(Cell::new(cell.row, cell.column))
            }
            TableEdit::InsertRowBelow => {
                self.rows.insert(cell.row + 1, vec![String::new(); columns]);
                Some(Cell::new(cell.row + 1, cell.column))
            }
            TableEdit::InsertColumnLeft => {
                for row in &mut self.rows {
                    row.insert(cell.column, String::new());
                }
                Some(Cell::new(cell.row, cell.column))
            }
            TableEdit::InsertColumnRight => {
                for row in &mut self.rows {
                    row.insert(cell.column + 1, String::new());
                }
                Some(Cell::new(cell.row, cell.column + 1))
            }
            TableEdit::DeleteRow if self.row_count() > 1 => {
                self.rows.remove(cell.row);
                Some(Cell::new(cell.row.min(self.row_count() - 1), cell.column))
            }
            TableEdit::DeleteColumn if columns > 1 => {
                for row in &mut self.rows {
                    row.remove(cell.column);
                }
                Some(Cell::new(
                    cell.row,
                    cell.column.min(self.column_count() - 1),
                ))
            }
            TableEdit::DeleteRow | TableEdit::DeleteColumn | TableEdit::DeleteTable => None,
        }
    }

    pub fn is_blank(&self) -> bool {
        self.rows
            .iter()
            .flatten()
            .all(|cell| cell.trim().is_empty())
    }

    /// Rows on lines, cells split by tabs: what a spreadsheet pastes.
    pub fn plain_text(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.join("\t").replace('\n', " "))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The table Teams sends: CKEditor's `figure.table`, `copy-paste-table` on the table, a one
    /// line cell without `<p>`, every line of a longer cell in its own `<p>`, empty `&nbsp;`.
    pub fn to_html(&self) -> String {
        let mut html =
            String::from("<figure class=\"table\"><table class=\"copy-paste-table\"><tbody>");
        for row in &self.rows {
            html.push_str("<tr>");
            for cell in row {
                html.push_str("<td>");
                if cell.trim().is_empty() {
                    html.push_str(EMPTY_CELL);
                } else if !cell.contains('\n') {
                    html.push_str(&escape_html(cell));
                } else {
                    for line in cell.split('\n') {
                        html.push_str("<p>");
                        if line.trim().is_empty() {
                            html.push_str(EMPTY_CELL);
                        } else {
                            html.push_str(&escape_html(line));
                        }
                        html.push_str("</p>");
                    }
                }
                html.push_str("</td>");
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table></figure>");
        html
    }
}

fn split_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut characters = inner.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' if characters.peek() == Some(&'|') => {
                current.push('|');
                characters.next();
            }
            '|' => cells.push(std::mem::take(&mut current).trim().to_owned()),
            other => current.push(other),
        }
    }
    cells.push(current.trim().to_owned());
    Some(cells)
}

fn is_delimiter_row(line: &str) -> bool {
    split_row(line).is_some_and(|cells| {
        cells.iter().all(|cell| {
            let dashes = cell.trim_start_matches(':').trim_end_matches(':');
            !dashes.is_empty() && dashes.chars().all(|character| character == '-')
        })
    })
}

/// A GitHub markdown table at the start of `lines`: header, delimiter row, body rows.
/// Returns the raw cell sources and how many lines it spans.
pub(crate) fn markdown_table(lines: &[&str]) -> Option<(Vec<Vec<String>>, usize)> {
    let header = split_row(lines.first()?)?;
    if !lines.get(1).is_some_and(|line| is_delimiter_row(line)) {
        return None;
    }
    let delimiter_columns = split_row(lines[1])?.len();
    if delimiter_columns != header.len() {
        return None;
    }
    let mut rows = vec![header];
    let mut consumed = 2;
    for line in &lines[2..] {
        match split_row(line) {
            Some(cells) if !line.trim().is_empty() => rows.push(cells),
            _ => break,
        }
        consumed += 1;
    }
    Some((rows, consumed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_walks_the_cells_and_adds_a_row_after_the_last() {
        let mut table = Table::new(2, 2);
        assert_eq!(table.next_cell(Cell::new(0, 0)), (Cell::new(0, 1), false));
        assert_eq!(table.next_cell(Cell::new(0, 1)), (Cell::new(1, 0), false));
        assert_eq!(table.next_cell(Cell::new(1, 1)), (Cell::new(2, 0), true));
        assert_eq!(table.row_count(), 3);
        assert_eq!(table.column_count(), 2);
    }

    #[test]
    fn shift_tab_walks_back_and_stops_at_the_first_cell() {
        let table = Table::new(2, 3);
        assert_eq!(table.previous_cell(Cell::new(1, 0)), Cell::new(0, 2));
        assert_eq!(table.previous_cell(Cell::new(0, 1)), Cell::new(0, 0));
        assert_eq!(table.previous_cell(Cell::new(0, 0)), Cell::new(0, 0));
    }

    #[test]
    fn rows_and_columns_insert_and_delete_around_the_cell() {
        let mut table = Table::from_rows(vec![
            vec!["a".into(), "b".into()],
            vec!["c".into(), "d".into()],
        ]);
        assert_eq!(
            table.apply(TableEdit::InsertRowAbove, Cell::new(1, 1)),
            Some(Cell::new(1, 1))
        );
        assert_eq!(table.rows[1], ["", ""]);
        assert_eq!(
            table.apply(TableEdit::InsertColumnRight, Cell::new(0, 0)),
            Some(Cell::new(0, 1))
        );
        assert_eq!(table.rows[0], ["a", "", "b"]);
        table.apply(TableEdit::DeleteColumn, Cell::new(0, 1));
        table.apply(TableEdit::DeleteRow, Cell::new(1, 0));
        assert_eq!(table.rows, [["a", "b"], ["c", "d"]]);
        assert_eq!(table.apply(TableEdit::DeleteTable, Cell::new(0, 0)), None);
    }

    #[test]
    fn deleting_the_last_row_or_column_deletes_the_table() {
        let mut table = Table::new(1, 2);
        assert_eq!(table.apply(TableEdit::DeleteRow, Cell::new(0, 0)), None);
        let mut table = Table::new(2, 1);
        assert_eq!(table.apply(TableEdit::DeleteColumn, Cell::new(0, 0)), None);
    }

    #[test]
    fn html_is_the_copy_paste_table_teams_sends() {
        let table = Table::from_rows(vec![vec!["a<b".into(), String::new()], vec!["x\ny".into()]]);
        assert_eq!(
            table.to_html(),
            "<figure class=\"table\"><table class=\"copy-paste-table\"><tbody>\
             <tr><td>a&lt;b</td><td>&nbsp;</td></tr>\
             <tr><td><p>x</p><p>y</p></td><td>&nbsp;</td></tr></tbody></table></figure>"
        );
    }

    #[test]
    fn markdown_tables_need_a_delimiter_row() {
        let lines = ["| A | B |", "|---|:-:|", "| 1 | 2 \\| 3 |", "", "after"];
        let (rows, consumed) = markdown_table(&lines).unwrap();
        assert_eq!(consumed, 3);
        assert_eq!(rows, [["A", "B"], ["1", "2 | 3"]]);
        assert!(markdown_table(&["a | b", "c | d"]).is_none());
        assert!(markdown_table(&["| a | b |", "|---|"]).is_none());
    }
}
