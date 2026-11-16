//! Mini month grid shared by the date picker popover and the calendar list pane.

use std::rc::Rc;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::call::calendar_list::mini_month_cell;
use crate::call::week_grid::{MONTH_GRID_DAYS, month_grid_start};
use crate::theme;

pub type PickDate = Rc<dyn Fn(NaiveDate, &mut Window, &mut App)>;

/// Weekday initials in column order from the first day, in the app language.
pub fn weekday_letters(first_day: Weekday) -> [String; 7] {
    let mut day = first_day;
    std::array::from_fn(|_| {
        let letter = app_base::format::weekday_name(day, app_base::format::WeekdayWidth::Letter);
        day = day.succ();
        letter
    })
}

/// Weekday initials and six weeks of day cells; the shown range is a band, today a circle.
#[allow(clippy::too_many_arguments)]
pub fn mini_month_grid(
    id: &'static str,
    month: NaiveDate,
    shown: &[NaiveDate],
    first_day: Weekday,
    today: NaiveDate,
    cell_size: f32,
    on_pick: PickDate,
) -> Div {
    let start = month_grid_start(month, first_day);
    let weeks = (0..MONTH_GRID_DAYS)
        .map(|offset| start + Duration::days(offset))
        .collect::<Vec<_>>()
        .chunks(7)
        .map(|week| {
            h_flex().children(week.iter().map(|date| {
                let cell = mini_month_cell(*date, month, shown, today);
                let pick = on_pick.clone();
                let date = *date;
                div()
                    .id(SharedString::from(format!("{id}-{date}")))
                    .size(px(cell_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(cell.shown, |band| band.bg(theme::popup_hover()))
                    .child(
                        div()
                            .size(px(cell_size - 4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(if cell.in_month {
                                theme::text()
                            } else {
                                theme::text_faint()
                            })
                            .when(cell.today, |circle| {
                                circle
                                    .bg(theme::accent())
                                    .text_color(theme::on_accent())
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .when(!cell.today, |circle| {
                                circle.hover(|circle| circle.bg(theme::border_strong()))
                            })
                            .child(date.day().to_string()),
                    )
                    .on_click(move |_, window, cx| pick(date, window, cx))
            }))
        })
        .collect::<Vec<_>>();
    v_flex()
        .child(h_flex().children(weekday_letters(first_day).map(|letter| {
            div()
                .size(px(cell_size))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(letter)
        })))
        .children(weeks)
}
