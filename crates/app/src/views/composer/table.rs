use std::collections::HashMap;
use std::ops::Range;

use gpui_kit::component::input::{
    Copy, Cut, InlineToken, InputEvent, Paste, Textarea, TextareaState,
};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{Cell, PICKER_COLUMNS, PICKER_ROWS, TABLE_MARK, Table, TableEdit};

use super::{Composer, StepKind};
use crate::theme::{self, ElevatedSurface as _};
use crate::views::actions::{
    DeleteTable, DeleteTableColumn, DeleteTableRow, InsertTableColumnLeft, InsertTableColumnRight,
    InsertTableRowAbove, InsertTableRowBelow,
};

pub(super) const TOKEN_PREFIX: &str = "table-";
const CELL_MAX_ROWS: usize = 12;
const PICKER_SQUARE: f32 = 16.;
const PICKER_GAP: f32 = 3.;
const TABLE_MARGIN: f32 = 4.;
const CELL_PADDING_X: f32 = 2.;
const CELL_PADDING_Y: f32 = 3.;
const MENU_WIDTH: f32 = 200.;
const MENU_PADDING: f32 = 4.;
const MENU_ROW_PADDING_X: f32 = 10.;
const MENU_ROW_PADDING_Y: f32 = 6.;
pub(super) const SELECTED_TABLE_TAG: &str = "composer/table-selected";

pub(super) struct TableView {
    cells: Vec<Vec<Entity<TextareaState>>>,
    _subscriptions: Vec<Subscription>,
}

impl TableView {
    fn shape(&self) -> (usize, usize) {
        (self.cells.len(), self.cells.first().map_or(0, Vec::len))
    }
}

/// The rows and columns the pointer covers in the size picker.
pub(super) struct TablePicker {
    rows: usize,
    columns: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableLineMenu {
    Insert,
    Remove,
}

impl TableLineMenu {
    /// Teams' order: "Insert column or row" and "Delete column, row, or table".
    pub(super) fn edits(self) -> &'static [TableEdit] {
        match self {
            TableLineMenu::Insert => &[
                TableEdit::InsertColumnRight,
                TableEdit::InsertColumnLeft,
                TableEdit::InsertRowAbove,
                TableEdit::InsertRowBelow,
            ],
            TableLineMenu::Remove => &[
                TableEdit::DeleteColumn,
                TableEdit::DeleteRow,
                TableEdit::DeleteTable,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TableMenu {
    pub(super) kind: TableLineMenu,
    pub(super) highlighted: usize,
}

impl TableMenu {
    pub(super) fn new(kind: TableLineMenu) -> Self {
        TableMenu {
            kind,
            highlighted: 0,
        }
    }

    pub(super) fn step(&mut self, delta: isize) {
        let count = self.kind.edits().len() as isize;
        self.highlighted = (self.highlighted as isize + delta).rem_euclid(count) as usize;
    }

    pub(super) fn chosen(&self) -> TableEdit {
        self.kind.edits()[self.highlighted]
    }
}

pub(super) fn token(number: usize) -> InlineToken {
    InlineToken::new(format!("{TOKEN_PREFIX}{number}"), TABLE_MARK.to_string()).block()
}

pub(super) fn is_table_token(token: &InlineToken) -> bool {
    token.id().starts_with(TOKEN_PREFIX)
}

fn token_number(id: &str) -> usize {
    id.strip_prefix(TOKEN_PREFIX)
        .and_then(|number| number.parse().ok())
        .unwrap_or_default()
}

fn caret_on_first_line(state: &TextareaState) -> bool {
    !state.value()[..state.cursor()].contains('\n')
}

fn caret_on_last_line(state: &TextareaState) -> bool {
    !state.value()[state.cursor()..].contains('\n')
}

impl Composer {
    fn table_tokens(&self, cx: &App) -> Vec<(Range<usize>, String)> {
        self.input
            .read(cx)
            .tokens()
            .iter()
            .filter(|span| is_table_token(span.token()))
            .map(|span| (span.range(), span.token().id().to_string()))
            .collect()
    }

    fn table_line(&self, token_id: &str, cx: &App) -> Option<usize> {
        let (range, _) = self
            .table_tokens(cx)
            .into_iter()
            .find(|(_, id)| id == token_id)?;
        let index = self.draft.line_at(range.start);
        self.draft.table(index).is_some().then_some(index)
    }

    /// Every table line gets its token and cell inputs; inputs follow the draft's cells.
    pub(super) fn sync_tables(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).value().as_ref() != self.draft.text() {
            return;
        }
        let ranges = self.draft.line_ranges();
        let tokens = self.table_tokens(cx);
        for (index, line) in ranges.iter().enumerate() {
            if self.draft.table(index).is_none() || tokens.iter().any(|(range, _)| range == line) {
                continue;
            }
            self.table_counter += 1;
            let token = token(self.table_counter);
            self.input.update(cx, |state, cx| {
                let selection = state.selected_range();
                if state
                    .replace_range_with_token(line.clone(), token, window, cx)
                    .is_ok()
                {
                    state.set_selected_range(selection, cx);
                }
            });
        }
        let tokens = self.table_tokens(cx);
        self.tables
            .retain(|id, _| tokens.iter().any(|(_, token_id)| token_id == id));
        for (range, id) in tokens {
            let Some(table) = self.draft.table(self.draft.line_at(range.start)).cloned() else {
                continue;
            };
            self.sync_table_view(&id, &table, window, cx);
        }
    }

    fn sync_table_view(
        &mut self,
        token_id: &str,
        table: &Table,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shape = (table.row_count(), table.column_count());
        if self
            .tables
            .get(token_id)
            .is_some_and(|view| view.shape() == shape)
        {
            let cells = self.tables[token_id].cells.clone();
            for (row, inputs) in cells.iter().enumerate() {
                for (column, input) in inputs.iter().enumerate() {
                    let text = table.cell(Cell::new(row, column)).unwrap_or_default();
                    if input.read(cx).value().as_ref() != text {
                        let text = text.to_owned();
                        input.update(cx, |state, cx| state.set_value(text, window, cx));
                    }
                }
            }
            return;
        }
        let mut subscriptions = Vec::new();
        let cells = table
            .rows
            .iter()
            .enumerate()
            .map(|(row, texts)| {
                texts
                    .iter()
                    .enumerate()
                    .map(|(column, text)| {
                        let text = text.clone();
                        let input = cx.new(|cx| {
                            TextareaState::new(window, cx)
                                .auto_grow(1, CELL_MAX_ROWS)
                                .default_value(text)
                        });
                        let id = token_id.to_owned();
                        let cell = Cell::new(row, column);
                        subscriptions.push(cx.subscribe_in(
                            &input,
                            window,
                            move |this, _, event: &InputEvent, window, cx| {
                                this.on_cell_event(&id, cell, event, window, cx)
                            },
                        ));
                        input
                    })
                    .collect()
            })
            .collect();
        self.tables.insert(
            token_id.to_owned(),
            TableView {
                cells,
                _subscriptions: subscriptions,
            },
        );
    }

    fn on_cell_event(
        &mut self,
        token_id: &str,
        cell: Cell,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Focus => {
                self.active_cell = Some((token_id.to_owned(), cell));
                self.table_picker = None;
            }
            InputEvent::Change => {
                let Some(index) = self.table_line(token_id, cx) else {
                    return;
                };
                let Some(input) = self.cell_input(token_id, cell) else {
                    return;
                };
                let text = input.read(cx).value().to_string();
                let Some(mut table) = self.draft.table(index).cloned() else {
                    return;
                };
                if table.cell(cell) == Some(text.as_str()) {
                    return;
                }
                table.set_cell(cell, &text);
                self.draft.set_table(index, table);
                self.record(
                    StepKind::CellTyping {
                        table: token_number(token_id),
                        row: cell.row,
                        column: cell.column,
                    },
                    cx,
                );
                self.schedule_draft_save(cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn cell_input(&self, token_id: &str, cell: Cell) -> Option<Entity<TextareaState>> {
        self.tables
            .get(token_id)?
            .cells
            .get(cell.row)?
            .get(cell.column)
            .cloned()
    }

    pub(super) fn focused_cell_input(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<Entity<TextareaState>> {
        let (token_id, cell) = self.focused_cell(window, cx)?;
        self.cell_input(&token_id, cell)
    }

    /// The cell that has the keyboard, if any.
    pub(super) fn focused_cell(&self, window: &Window, cx: &App) -> Option<(String, Cell)> {
        self.tables.iter().find_map(|(id, view)| {
            view.cells.iter().enumerate().find_map(|(row, inputs)| {
                inputs.iter().enumerate().find_map(|(column, input)| {
                    input
                        .focus_handle(cx)
                        .is_focused(window)
                        .then(|| (id.clone(), Cell::new(row, column)))
                })
            })
        })
    }

    pub(super) fn focus_cell(
        &mut self,
        token_id: &str,
        cell: Cell,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.cell_input(token_id, cell) else {
            return;
        };
        input.update(cx, |state, cx| {
            let end = state.value().len();
            state.focus(window, cx);
            state.set_selected_range(end..end, cx);
        });
        self.active_cell = Some((token_id.to_owned(), cell));
        cx.notify();
    }

    fn token_at_line(&self, index: usize, cx: &App) -> Option<String> {
        let line = self.draft.line_ranges().get(index)?.clone();
        self.table_tokens(cx)
            .into_iter()
            .find(|(range, _)| *range == line)
            .map(|(_, id)| id)
    }

    pub(super) fn focus_table_line(
        &mut self,
        index: usize,
        last: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(token_id) = self.token_at_line(index, cx) else {
            return;
        };
        let Some(table) = self.draft.table(index) else {
            return;
        };
        let cell = if last {
            table.last_cell()
        } else {
            Cell::new(0, 0)
        };
        self.focus_cell(&token_id, cell, window, cx);
    }

    pub(super) fn insert_table(
        &mut self,
        rows: usize,
        columns: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.table_picker = None;
        let cursor = match self.focused_cell(window, cx) {
            Some((token_id, _)) => self
                .table_line(&token_id, cx)
                .map_or(self.selection(cx).end, |index| {
                    self.draft.line_ranges()[index].end
                }),
            None => self.selection(cx).end,
        };
        let index = self.draft.insert_table(cursor, Table::new(rows, columns));
        let after = self.draft.line_ranges()[index].end;
        self.commit(Some(after), window, cx);
        self.focus_table_line(index, false, window, cx);
    }

    fn leave_table(
        &mut self,
        token_id: &str,
        below: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.table_line(token_id, cx) else {
            return;
        };
        let cursor = self.draft.line_beside_table(index, below);
        self.active_cell = None;
        self.focus(window, cx);
        self.commit(Some(cursor), window, cx);
    }

    /// Tab, Shift+Tab, Up and Down while a cell has the keyboard. `false` leaves the key to
    /// the cell.
    pub(super) fn table_key(
        &mut self,
        key: TableKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((token_id, cell)) = self.focused_cell(window, cx) else {
            return false;
        };
        let Some(index) = self.table_line(&token_id, cx) else {
            return false;
        };
        let Some(mut table) = self.draft.table(index).cloned() else {
            return false;
        };
        let Some(input) = self.cell_input(&token_id, cell) else {
            return false;
        };
        let target = match key {
            TableKey::Next => {
                let (next, added_row) = table.next_cell(cell);
                if added_row {
                    self.draft.set_table(index, table);
                    self.record(StepKind::Other, cx);
                    self.sync_tables(window, cx);
                    self.schedule_draft_save(cx);
                }
                next
            }
            TableKey::Previous => table.previous_cell(cell),
            TableKey::Down if !caret_on_last_line(input.read(cx)) => return false,
            TableKey::Up if !caret_on_first_line(input.read(cx)) => return false,
            TableKey::Down if cell.row + 1 == table.row_count() => {
                self.leave_table(&token_id, true, window, cx);
                return true;
            }
            TableKey::Up if cell.row == 0 => {
                self.leave_table(&token_id, false, window, cx);
                return true;
            }
            TableKey::Down => Cell::new(cell.row + 1, cell.column),
            TableKey::Up => Cell::new(cell.row - 1, cell.column),
        };
        self.focus_cell(&token_id, target, window, cx);
        true
    }

    pub(super) fn edit_table(
        &mut self,
        edit: TableEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((token_id, cell)) = self
            .focused_cell(window, cx)
            .or_else(|| self.active_cell.clone())
        else {
            return;
        };
        let Some(index) = self.table_line(&token_id, cx) else {
            return;
        };
        let Some(mut table) = self.draft.table(index).cloned() else {
            return;
        };
        match table.apply(edit, cell) {
            Some(target) => {
                self.draft.set_table(index, table);
                self.record(StepKind::Other, cx);
                self.sync_tables(window, cx);
                self.schedule_draft_save(cx);
                self.focus_cell(&token_id, target, window, cx);
            }
            None => {
                let cursor = self.draft.remove_table(index);
                self.active_cell = None;
                self.focus(window, cx);
                self.commit(Some(cursor), window, cx);
            }
        }
        cx.notify();
    }

    /// The caret never rests on a table's line: coming from above it enters the first
    /// cell, from below the first cell of the last row. Only a move without an edit
    /// counts, so a paste that passes the table on its way does not enter it.
    pub(super) fn redirect_caret_from_table(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.input.read(cx);
        let selection = state.selected_range();
        let focused = state.focus_handle(cx).is_focused(window);
        let value = state.value();
        if value.as_ref() != self.caret_text {
            self.caret_text = value.to_string();
            self.caret_before_table = selection.start;
            return;
        }
        if !focused || !selection.is_empty() || value.as_ref() != self.draft.text() {
            return;
        }
        let Some(index) = self.draft.table_at(selection.start) else {
            self.caret_before_table = selection.start;
            return;
        };
        let line = self.draft.line_ranges()[index].clone();
        let from_below = self.caret_before_table > line.end;
        let resting = if from_below {
            self.draft
                .line_ranges()
                .get(index + 1)
                .map_or(line.end, |next| next.start)
        } else {
            line.start.saturating_sub(1)
        };
        self.input.update(cx, |state, cx| {
            state.set_selected_range(resting..resting, cx)
        });
        self.caret_before_table = resting;
        let token = self.token_at_line(index, cx);
        let Some(token_id) = token else {
            return;
        };
        let row = if from_below {
            self.draft
                .table(index)
                .map_or(0, |table| table.row_count() - 1)
        } else {
            0
        };
        self.focus_cell(&token_id, Cell::new(row, 0), window, cx);
    }

    pub(super) fn in_table(&self, window: &Window, cx: &App) -> bool {
        self.focused_cell(window, cx).is_some()
    }

    /// Ctrl+Alt+= and Ctrl+Alt+- or the format bar buttons; only inside a table, like Teams.
    pub(super) fn open_table_menu(
        &mut self,
        kind: TableLineMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.in_table(window, cx) {
            return false;
        }
        self.table_picker = None;
        self.table_menu = Some(TableMenu::new(kind));
        if !self.format_bar_open() {
            self.set_format_bar(true, cx);
        }
        cx.notify();
        true
    }

    pub(super) fn accept_table_menu(
        &mut self,
        edit: Option<TableEdit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.table_menu.take() else {
            return;
        };
        self.edit_table(edit.unwrap_or(menu.chosen()), window, cx);
    }

    pub(super) fn render_table_menu(
        &self,
        kind: TableLineMenu,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.table_menu.filter(|menu| menu.kind == kind)?;
        let rows = kind.edits().iter().enumerate().map(|(index, edit)| {
            let edit = *edit;
            div()
                .id(ElementId::Name(format!("table-menu-{edit:?}").into()))
                .px(px(MENU_ROW_PADDING_X))
                .py(px(MENU_ROW_PADDING_Y))
                .rounded(px(6.))
                .text_size(px(13.))
                .text_color(theme::text())
                .cursor_pointer()
                .when(index == menu.highlighted, |row| {
                    row.bg(theme::popup_hover())
                })
                .hover(|row| row.bg(theme::popup_hover()))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.accept_table_menu(Some(edit), window, cx)
                }))
                .child(edit.label())
        });
        Some(
            v_flex()
                .id("table-line-menu")
                .min_w(px(MENU_WIDTH))
                .p(px(MENU_PADDING))
                .rounded(px(10.))
                .flyout_surface()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.table_menu = None;
                    cx.notify();
                }))
                .children(rows)
                .into_any_element(),
        )
    }

    pub(super) fn open_table_picker(&mut self, cx: &mut Context<Self>) {
        self.table_picker = Some(TablePicker {
            rows: 1,
            columns: 1,
        });
        cx.notify();
    }

    pub(super) fn render_table_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.table_picker.as_ref()?;
        let grid = v_flex()
            .gap(px(PICKER_GAP))
            .children((1..=PICKER_ROWS).map(|row| {
                h_flex()
                    .gap(px(PICKER_GAP))
                    .children((1..=PICKER_COLUMNS).map(|column| {
                        let covered = row <= picker.rows && column <= picker.columns;
                        div()
                            .id(ElementId::Name(format!("table-size-{row}-{column}").into()))
                            .size(px(PICKER_SQUARE))
                            .rounded(px(2.))
                            .bg(if covered {
                                theme::accent().opacity(0.6)
                            } else {
                                theme::border_strong()
                            })
                            .cursor_pointer()
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered && let Some(picker) = this.table_picker.as_mut() {
                                    picker.rows = row;
                                    picker.columns = column;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.insert_table(row, column, window, cx)
                            }))
                    }))
            }));
        Some(
            v_flex()
                .id("table-picker")
                .p(px(10.))
                .gap(px(8.))
                .rounded(px(10.))
                .flyout_surface()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.table_picker = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(format!("Insert {}x{} table", picker.columns, picker.rows)),
                )
                .child(grid)
                .into_any_element(),
        )
    }

    /// The table token: a grid of cell inputs over the full width, solid lines like Teams.
    /// The editor leaves a selected token's highlight to the token, so a selection over the
    /// table tints every cell in the text selection color.
    pub(super) fn render_table_token(
        &self,
        token_id: &str,
        width: Pixels,
        selected: bool,
    ) -> Option<AnyElement> {
        let view = self.tables.get(token_id)?;
        let rows = view.cells.iter().map(|inputs| {
            h_flex()
                .w_full()
                .items_stretch()
                .children(inputs.iter().map(|input| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(px(CELL_PADDING_X))
                        .py(px(CELL_PADDING_Y))
                        .border_r_1()
                        .border_b_1()
                        .border_color(theme::border_strong())
                        .when(selected, |cell| cell.bg(theme::selection()))
                        .child(
                            Textarea::new(input)
                                .appearance(false)
                                .bordered(false)
                                .small()
                                .context_menu(|menu, _, _| table_menu(menu)),
                        )
                }))
        });
        Some(
            v_flex()
                .id(ElementId::Name(format!("composer-{token_id}").into()))
                .when(selected, |table| {
                    table.debug_selector(|| SELECTED_TABLE_TAG.into())
                })
                .w(width)
                .my(px(TABLE_MARGIN))
                .border_t_1()
                .border_l_1()
                .border_color(theme::border_strong())
                .on_any_mouse_down(|_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .children(rows)
                .into_any_element(),
        )
    }
}

fn table_menu(menu: NativeMenu) -> NativeMenu {
    let menu = TableEdit::INSERTS
        .into_iter()
        .fold(menu, |menu, edit| {
            menu.menu(edit.label(), edit_action(edit))
        })
        .separator();
    TableEdit::DELETES
        .into_iter()
        .fold(menu, |menu, edit| {
            menu.menu(edit.label(), edit_action(edit))
        })
        .separator()
        .menu("Cut", Box::new(Cut))
        .menu("Copy", Box::new(Copy))
        .menu("Paste", Box::new(Paste))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableKey {
    Next,
    Previous,
    Up,
    Down,
}

fn edit_action(edit: TableEdit) -> Box<dyn Action> {
    match edit {
        TableEdit::InsertRowAbove => Box::new(InsertTableRowAbove),
        TableEdit::InsertRowBelow => Box::new(InsertTableRowBelow),
        TableEdit::InsertColumnLeft => Box::new(InsertTableColumnLeft),
        TableEdit::InsertColumnRight => Box::new(InsertTableColumnRight),
        TableEdit::DeleteRow => Box::new(DeleteTableRow),
        TableEdit::DeleteColumn => Box::new(DeleteTableColumn),
        TableEdit::DeleteTable => Box::new(DeleteTable),
    }
}

pub(super) type TableViews = HashMap<String, TableView>;

#[cfg(test)]
mod tests {
    use teams_core::TableEdit;

    use super::{TableLineMenu, TableMenu};

    #[test]
    fn the_line_menus_list_teams_entries_in_teams_order() {
        assert_eq!(
            TableLineMenu::Insert.edits(),
            &[
                TableEdit::InsertColumnRight,
                TableEdit::InsertColumnLeft,
                TableEdit::InsertRowAbove,
                TableEdit::InsertRowBelow
            ]
        );
        assert_eq!(
            TableLineMenu::Remove.edits(),
            &[
                TableEdit::DeleteColumn,
                TableEdit::DeleteRow,
                TableEdit::DeleteTable
            ]
        );
    }

    #[test]
    fn the_menu_highlight_wraps_both_ways() {
        let mut menu = TableMenu::new(TableLineMenu::Remove);
        assert_eq!(menu.chosen(), TableEdit::DeleteColumn);
        menu.step(-1);
        assert_eq!(menu.chosen(), TableEdit::DeleteTable);
        menu.step(1);
        menu.step(1);
        assert_eq!(menu.chosen(), TableEdit::DeleteRow);
    }
}
