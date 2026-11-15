use std::cell::RefCell;
use std::rc::Rc;

use chrono::{Datelike, Duration, Local, Months, NaiveDate, Weekday};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app_state::AppState;
use crate::call::week_grid::{MONTH_GRID_DAYS, first_of_month, month_grid_start};
use crate::theme;

pub const DATE_PICKER_TAG: &str = "shell/islands/calendar/date-picker";
const CELL_SIZE: f32 = 32.;
const PICKER_PADDING: f32 = 12.;

pub struct DatePicker {
    app: Entity<AppState>,
    month: NaiveDate,
    shown: Vec<NaiveDate>,
    first_day: Weekday,
    popover: WeakEntity<PopoverState>,
}

impl DatePicker {
    fn new(
        app: Entity<AppState>,
        shown: Vec<NaiveDate>,
        first_day: Weekday,
        month: NaiveDate,
        popover: WeakEntity<PopoverState>,
    ) -> Self {
        DatePicker {
            app,
            month: first_of_month(month),
            shown,
            first_day,
            popover,
        }
    }

    fn move_month(&mut self, forward: bool, cx: &mut Context<Self>) {
        let moved = if forward {
            self.month.checked_add_months(Months::new(1))
        } else {
            self.month.checked_sub_months(Months::new(1))
        };
        self.month = moved.unwrap_or(self.month);
        cx.notify();
    }

    fn pick(&mut self, date: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        self.app
            .update(cx, |state, cx| state.show_calendar_date(date, cx));
        self.popover
            .update(cx, |popover, cx| popover.dismiss(window, cx))
            .ok();
    }

    fn day_cell(&self, date: NaiveDate, today: NaiveDate, cx: &mut Context<Self>) -> Stateful<Div> {
        let in_month = date.month() == self.month.month();
        let shown = self.shown.contains(&date);
        let is_today = date == today;
        div()
            .id(SharedString::from(format!("date-picker-{date}")))
            .size(px(CELL_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(if in_month {
                theme::text()
            } else {
                theme::text_faint()
            })
            .when(shown && !is_today, |cell| cell.bg(theme::popup_hover()))
            .when(is_today, |cell| {
                cell.bg(theme::accent()).text_color(theme::on_accent())
            })
            .when(!is_today, |cell| {
                cell.hover(|cell| cell.bg(theme::border_strong()))
            })
            .child(date.day().to_string())
            .on_click(cx.listener(move |this, _, window, cx| this.pick(date, window, cx)))
    }
}

impl Render for DatePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let today = Local::now().date_naive();
        let start = month_grid_start(self.month, self.first_day);
        let days: Vec<NaiveDate> = (0..MONTH_GRID_DAYS)
            .map(|offset| start + Duration::days(offset))
            .collect();
        let weeks = days
            .chunks(7)
            .map(|week| {
                h_flex().children(
                    week.iter()
                        .map(|date| self.day_cell(*date, today, cx))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        v_flex()
            .debug_selector(|| DATE_PICKER_TAG.into())
            .p(px(PICKER_PADDING))
            .gap(px(4.))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .pb(px(4.))
                    .child(
                        div()
                            .pl(px(6.))
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(app_base::format::month_year_label(self.month)),
                    )
                    .child(
                        h_flex()
                            .child(
                                Button::new("date-picker-back")
                                    .small()
                                    .ghost()
                                    .icon(IconName::ChevronLeft)
                                    .tooltip(crate::i18n::tr("calendar.view.previous_month_short"))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.move_month(false, cx)),
                                    ),
                            )
                            .child(
                                Button::new("date-picker-forward")
                                    .small()
                                    .ghost()
                                    .icon(IconName::ChevronRight)
                                    .tooltip(crate::i18n::tr("calendar.view.next_month_short"))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.move_month(true, cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                h_flex().children(weekday_letters(self.first_day).map(|letter| {
                    div()
                        .size(px(CELL_SIZE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(letter)
                })),
            )
            .children(weeks)
    }
}

/// Weekday initials in column order from the first day, in the app language.
fn weekday_letters(first_day: Weekday) -> [String; 7] {
    let mut day = first_day;
    std::array::from_fn(|_| {
        let letter = app_base::format::weekday_name(day, app_base::format::WeekdayWidth::Letter);
        day = day.succ();
        letter
    })
}

pub fn date_picker_button(
    app: &Entity<AppState>,
    title: String,
    shown: Vec<NaiveDate>,
    first_day: Weekday,
    month: NaiveDate,
) -> AnyElement {
    let app = app.clone();
    let picker: Rc<RefCell<Option<Entity<DatePicker>>>> = Rc::default();
    let reset = picker.clone();
    Popover::new("calendar-date-picker")
        .anchor(Anchor::TopLeft)
        .p(px(0.))
        .trigger(
            Button::new("calendar-date-title")
                .small()
                .ghost()
                .label(title)
                .dropdown_caret(true),
        )
        .on_open_change(move |open, _, _| {
            if !*open {
                reset.borrow_mut().take();
            }
        })
        .content(move |_, _, cx| {
            if let Some(existing) = picker.borrow().clone() {
                return existing;
            }
            let popover = cx.entity().downgrade();
            let app = app.clone();
            let created =
                cx.new(|_| DatePicker::new(app, shown.clone(), first_day, month, popover));
            *picker.borrow_mut() = Some(created.clone());
            created
        })
        .into_any_element()
}
