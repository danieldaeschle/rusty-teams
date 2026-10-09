use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{CardTable, ContainerStyle, HorizontalAlignment, VerticalAlignment};

use super::adaptive_card::{CardContext, container_tint, items_view, selectable, sized_column};
use crate::theme;

const CELL_PADDING: f32 = 6.;
const GRID_RADIUS: f32 = 4.;

pub(super) fn table_view(table: &CardTable, id: &str, context: &CardContext) -> AnyElement {
    let grid_color = grid_color(table.grid_style);
    v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(GRID_RADIUS))
        .when(table.show_grid_lines, |grid| {
            grid.border_1().border_color(grid_color)
        })
        .children(table.rows.iter().enumerate().map(|(row_index, row)| {
            let header = table.first_row_as_header && row_index == 0;
            h_flex()
                .w_full()
                .items_stretch()
                .when(header, |line| {
                    line.bg(theme::table_header())
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .when_some(container_tint(row.style), |line, tint| line.bg(tint))
                .when(row_index > 0 && table.show_grid_lines, |line| {
                    line.border_t_1().border_color(grid_color)
                })
                .children(row.cells.iter().enumerate().map(|(column_index, cell)| {
                    let cell_id = format!("{id}-{row_index}-{column_index}");
                    let body = selectable(
                        items_view(&cell.items, &cell_id, context, false).into_any_element(),
                        cell.select_action.as_ref(),
                        &cell_id,
                        false,
                        context,
                    );
                    let aligned = v_flex()
                        .min_w(px(0.))
                        .p(px(CELL_PADDING))
                        .overflow_hidden()
                        .when(column_index > 0 && table.show_grid_lines, |cell| {
                            cell.border_l_1().border_color(grid_color)
                        })
                        .when_some(container_tint(cell.style), |cell, tint| cell.bg(tint))
                        .map(|cell_box| {
                            match table.cell_vertical_alignment(row, cell, column_index) {
                                VerticalAlignment::Top => cell_box.justify_start(),
                                VerticalAlignment::Center => cell_box.justify_center(),
                                VerticalAlignment::Bottom => cell_box.justify_end(),
                            }
                        })
                        .map(
                            |cell_box| match table.cell_horizontal_alignment(row, column_index) {
                                HorizontalAlignment::Left => cell_box.text_left(),
                                HorizontalAlignment::Center => cell_box.text_center(),
                                HorizontalAlignment::Right => cell_box.text_right(),
                            },
                        );
                    sized_column(aligned, table.columns[column_index].width).child(body)
                }))
        }))
        .into_any_element()
}

fn grid_color(style: ContainerStyle) -> Hsla {
    match style {
        ContainerStyle::Default => theme::border_strong(),
        ContainerStyle::Emphasis => theme::text_faint(),
        ContainerStyle::Accent => theme::accent(),
        ContainerStyle::Good => theme::green(),
        ContainerStyle::Warning => theme::amber(),
        ContainerStyle::Attention => theme::red(),
    }
}
