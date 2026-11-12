use app_base::format::clock_format;
use chatsvc::CalendarEvent;
use chrono::{Datelike, FixedOffset, NaiveDate};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app_state::AppState;
use crate::call::week_grid::{WeekDay, is_effective_all_day, month_shown};
use crate::theme;

pub const MONTH_GRID_TAG: &str = "shell/islands/calendar/month";
const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAY_HEADER_HEIGHT: f32 = 32.;
const VISIBLE_EVENTS: usize = 3;
const DAY_NUMBER_SIZE: f32 = 24.;
const CHIP_HEIGHT: f32 = 20.;

fn day_events(day: &WeekDay) -> Vec<usize> {
    let mut timed: Vec<(i64, usize)> = day
        .blocks
        .iter()
        .map(|block| (block.start_minute, block.event_index))
        .collect();
    timed.sort();
    day.all_day
        .iter()
        .copied()
        .chain(timed.into_iter().map(|(_, index)| index))
        .collect()
}

fn more_label(hidden: usize) -> String {
    format!("+{hidden} more")
}

fn event_chip(event: &CalendarEvent, offset: FixedOffset) -> Div {
    let all_day = is_effective_all_day(event, offset);
    let time = (!all_day).then(|| {
        event
            .start
            .with_timezone(&offset)
            .format(clock_format())
            .to_string()
    });
    h_flex()
        .h(px(CHIP_HEIGHT))
        .flex_none()
        .w_full()
        .px(px(6.))
        .gap(px(6.))
        .items_center()
        .rounded(px(4.))
        .bg(theme::accent().opacity(if all_day { 0.28 } else { 0.14 }))
        .text_size(px(11.5))
        .text_color(theme::text())
        .children(time.map(|time| div().flex_none().text_color(theme::text_soft()).child(time)))
        .child(div().flex_1().min_w_0().truncate().child({
            let mut sources = event.sources.clone();
            if sources.is_empty() {
                sources.push(event.source);
            }
            sources.sort_by_key(|source| source.label());
            sources.dedup();
            if sources.len() == 1 && sources[0] == chatsvc::CalendarSource::Outlook {
                event.subject.clone()
            } else {
                let names = sources
                    .iter()
                    .map(|source| source.label())
                    .collect::<Vec<_>>()
                    .join("+");
                format!("{} · {names}", event.subject)
            }
        }))
}

fn day_cell(
    state: &Entity<AppState>,
    day: &WeekDay,
    events: &[CalendarEvent],
    shown_month: NaiveDate,
    today: NaiveDate,
    offset: FixedOffset,
) -> Stateful<Div> {
    let date = day.date;
    let in_month = date.month() == shown_month.month();
    let is_today = date == today;
    let indexes = day_events(day);
    let hidden = indexes.len().saturating_sub(VISIBLE_EVENTS);
    let number = if date.day() == 1 {
        date.format("%b %-d").to_string()
    } else {
        date.day().to_string()
    };
    let open_state = state.clone();
    v_flex()
        .id(SharedString::from(format!("calendar-month-{date}")))
        .flex_1()
        .min_w_0()
        .h_full()
        .p(px(4.))
        .gap(px(2.))
        .overflow_hidden()
        .cursor_pointer()
        .when(!in_month, |cell| cell.bg(theme::background().opacity(0.35)))
        .hover(|cell| cell.bg(theme::row_hover()))
        .child(
            h_flex().child(
                div()
                    .h(px(DAY_NUMBER_SIZE))
                    .min_w(px(DAY_NUMBER_SIZE))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .text_size(px(12.))
                    .when(is_today, |number| {
                        number
                            .bg(theme::accent())
                            .text_color(theme::on_accent())
                            .font_weight(FontWeight::SEMIBOLD)
                    })
                    .when(!is_today, |number| {
                        number.text_color(if in_month {
                            theme::text()
                        } else {
                            theme::text_faint()
                        })
                    })
                    .child(number),
            ),
        )
        .children(
            indexes
                .iter()
                .take(VISIBLE_EVENTS)
                .filter_map(|index| events.get(*index))
                .map(|event| event_chip(event, offset)),
        )
        .when(hidden > 0, |cell| {
            cell.child(
                div()
                    .px(px(6.))
                    .text_size(px(11.5))
                    .text_color(theme::text_muted())
                    .child(more_label(hidden)),
            )
        })
        .on_click(move |_, _, cx| {
            open_state.update(cx, |state, cx| state.show_calendar_day(date, cx));
        })
}

fn horizontal_line() -> Div {
    div().w_full().h(px(1.)).flex_none().bg(theme::border())
}

fn vertical_line() -> Div {
    div().w(px(1.)).h_full().flex_none().bg(theme::border())
}

pub fn month_grid(
    state: &Entity<AppState>,
    start: NaiveDate,
    days: &[WeekDay],
    events: &[CalendarEvent],
    today: NaiveDate,
    offset: FixedOffset,
) -> Div {
    let shown_month = month_shown(start);
    let mut rows = v_flex().flex_1().min_h_0().w_full();
    for week in days.chunks(7) {
        let mut row = h_flex().flex_1().min_h_0().w_full();
        for (position, day) in week.iter().enumerate() {
            if position > 0 {
                row = row.child(vertical_line());
            }
            row = row.child(day_cell(state, day, events, shown_month, today, offset));
        }
        rows = rows.child(horizontal_line()).child(row);
    }
    v_flex()
        .debug_selector(|| MONTH_GRID_TAG.into())
        .flex_1()
        .min_h_0()
        .w_full()
        .child(
            h_flex()
                .h(px(WEEKDAY_HEADER_HEIGHT))
                .flex_none()
                .w_full()
                .children(WEEKDAY_NAMES.iter().map(|name| {
                    div()
                        .flex_1()
                        .px(px(10.))
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(*name)
                })),
        )
        .child(rows)
}

#[cfg(test)]
mod tests {
    use super::{WeekDay, day_events, more_label};
    use crate::call::week_grid::WeekBlock;
    use chrono::NaiveDate;

    #[test]
    fn a_day_lists_all_day_events_first_then_by_start() {
        let block = |event_index, start_minute| WeekBlock {
            event_index,
            start_minute,
            end_minute: start_minute + 30,
            lane: 0,
            lanes: 1,
        };
        let day = WeekDay {
            date: NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(),
            all_day: vec![4],
            blocks: vec![block(2, 600), block(1, 540)],
        };
        assert_eq!(day_events(&day), [4, 1, 2]);
        assert_eq!(more_label(2), "+2 more");
    }
}
