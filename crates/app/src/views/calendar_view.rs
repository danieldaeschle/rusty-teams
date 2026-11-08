use chatsvc::{CalendarEvent, ResponseType};
use chrono::{DateTime, Local, NaiveDate, Offset, Timelike, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::join_dialog::open_join_with_id;
use super::new_meeting_dialog::open_new_meeting;
use crate::app_state::AppState;
use crate::call::join_available;
use crate::call::week_grid::{
    WORK_WEEK_DAYS, WeekBlock, WeekDay, hour_label, range_label, week_days,
};
use crate::theme;

const HOUR_HEIGHT: f32 = 48.;
const GUTTER_WIDTH: f32 = 64.;
const HEADER_HEIGHT: f32 = 52.;
const DAY_HEADER_HEIGHT: f32 = 56.;
const ALL_DAY_HEIGHT: f32 = 26.;
const BLOCK_GAP: f32 = 2.;
const FIRST_VISIBLE_HOUR: f32 = 8.;
const CANCELED_PREFIX: &str = "Canceled: ";
const COMPACT_HEADER_WIDTH: f32 = 960.;
const EVENT_FILL_OPACITY: f32 = 0.28;

pub struct CalendarView {
    state: Entity<AppState>,
    scroll: ScrollHandle,
    scrolled_to_morning: bool,
    _observation: Subscription,
}

impl CalendarView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&state, |_, _, cx| cx.notify());
        CalendarView {
            state,
            scroll: ScrollHandle::new(),
            scrolled_to_morning: false,
            _observation: observation,
        }
    }

    fn header(&self, start: NaiveDate, compact: bool) -> impl IntoElement {
        let label = |text: &'static str| (!compact).then_some(text);
        let (today, back, forward) = (self.state.clone(), self.state.clone(), self.state.clone());
        let (join_state, meet_state, new_state) =
            (self.state.clone(), self.state.clone(), self.state.clone());
        h_flex()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .child(
                div()
                    .flex_none()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .mr(px(8.))
                    .child("Calendar"),
            )
            .child(
                Button::new("calendar-today")
                    .small()
                    .outline()
                    .icon(IconName::Calendar)
                    .when_some(label("Today"), |button, text| button.label(text))
                    .tooltip("Today")
                    .on_click(move |_, _, cx| {
                        today.update(cx, |state, cx| state.calendar_to_today(cx))
                    }),
            )
            .child(
                Button::new("calendar-back")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronLeft)
                    .tooltip("Go to previous week")
                    .on_click(move |_, _, cx| {
                        back.update(cx, |state, cx| state.shift_calendar_week(-1, cx))
                    }),
            )
            .child(
                Button::new("calendar-forward")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronRight)
                    .tooltip("Go to next week")
                    .on_click(move |_, _, cx| {
                        forward.update(cx, |state, cx| state.shift_calendar_week(1, cx))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(range_label(start, WORK_WEEK_DAYS)),
            )
            .child(
                Button::new("calendar-join-id")
                    .small()
                    .outline()
                    .icon(IconName::Hash)
                    .when_some(label("Join with an ID"), |button, text| button.label(text))
                    .tooltip("Join with an ID")
                    .on_click(move |_, window, cx| {
                        open_join_with_id(join_state.clone(), window, cx)
                    }),
            )
            .child(
                Button::new("calendar-meet-now")
                    .small()
                    .outline()
                    .icon(IconName::Video)
                    .when_some(label("Meet now"), |button, text| button.label(text))
                    .tooltip("Meet now")
                    .on_click(move |_, _, cx| {
                        meet_state.update(cx, |state, cx| state.start_meet_now(cx))
                    }),
            )
            .child(
                Button::new("calendar-new-meeting")
                    .small()
                    .primary()
                    .icon(IconName::Plus)
                    .when_some(label("New meeting"), |button, text| button.label(text))
                    .tooltip("New meeting")
                    .on_click(move |_, window, cx| open_new_meeting(new_state.clone(), window, cx)),
            )
    }

    fn day_headers(&self, days: &[WeekDay], today: NaiveDate) -> impl IntoElement {
        h_flex()
            .h(px(DAY_HEADER_HEIGHT))
            .flex_none()
            .border_b_1()
            .border_color(theme::border())
            .child(div().w(px(GUTTER_WIDTH)).flex_none())
            .children(days.iter().map(|day| {
                let is_today = day.date == today;
                let color = if is_today {
                    theme::accent_text()
                } else {
                    theme::text_soft()
                };
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .justify_center()
                    .px(px(8.))
                    .border_l_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(day.date.format("%d").to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(color)
                            .child(day.date.format("%A").to_string()),
                    )
            }))
    }

    fn all_day_row(&self, days: &[WeekDay], events: &[CalendarEvent]) -> Option<Div> {
        if days.iter().all(|day| day.all_day.is_empty()) {
            return None;
        }
        Some(
            h_flex()
                .flex_none()
                .border_b_1()
                .border_color(theme::border())
                .child(div().w(px(GUTTER_WIDTH)).flex_none())
                .children(days.iter().map(|day| {
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(BLOCK_GAP))
                        .p(px(BLOCK_GAP))
                        .border_l_1()
                        .border_color(theme::border())
                        .children(day.all_day.iter().map(|&index| {
                            div()
                                .h(px(ALL_DAY_HEIGHT - 2. * BLOCK_GAP))
                                .px(px(6.))
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .bg(event_fill(events[index].response))
                                .text_size(px(12.))
                                .truncate()
                                .child(event_title(&events[index]))
                        }))
                })),
        )
    }

    fn event_block(
        &self,
        block: &WeekBlock,
        event: &CalendarEvent,
        now: DateTime<Utc>,
    ) -> impl IntoElement {
        let top = block.start_minute as f32 / 60. * HOUR_HEIGHT;
        let height = ((block.end_minute - block.start_minute) as f32 / 60. * HOUR_HEIGHT
            - BLOCK_GAP)
            .max(14.);
        let width = 1. / block.lanes as f32;
        let compact = height < 36.;
        let can_join = event.online
            && !event.cancelled
            && event.join_url.is_some()
            && join_available(event.start, event.end, now);
        let time = format!(
            "{} - {}",
            event.start.with_timezone(&Local).format("%-I:%M %p"),
            event.end.with_timezone(&Local).format("%-I:%M %p")
        );
        let (open_state, thread_id) = (self.state.clone(), event.thread_id.clone());
        let (join_state, join_url, subject) = (
            self.state.clone(),
            event.join_url.clone(),
            event.subject.clone(),
        );
        div()
            .id(SharedString::from(format!(
                "calendar-event-{}",
                event.object_id
            )))
            .absolute()
            .top(px(top))
            .h(px(height))
            .left(relative(width * block.lane as f32))
            .w(relative(width))
            .pr(px(BLOCK_GAP))
            .child(
                h_flex()
                    .size_full()
                    .overflow_hidden()
                    .rounded(px(4.))
                    .bg(event_fill(event.response))
                    .border_l_2()
                    .border_color(theme::accent())
                    .when(event.cancelled, |block| block.opacity(0.6))
                    .cursor_pointer()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .px(px(6.))
                            .py(px(if compact { 1. } else { 4. }))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(event_title(event)),
                            )
                            .when(!compact, |body| {
                                body.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme::text_soft())
                                        .truncate()
                                        .child(match &event.organizer_name {
                                            Some(organizer) => format!("{time}  {organizer}"),
                                            None => time,
                                        }),
                                )
                            }),
                    )
                    .when(can_join, |block| {
                        block.child(
                            div().flex_none().pr(px(4.)).pt(px(3.)).h_full().child(
                                Button::new(SharedString::from(format!(
                                    "calendar-join-{}",
                                    event.object_id
                                )))
                                .xsmall()
                                .primary()
                                .label("Join")
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    join_state.update(cx, |state, cx| {
                                        state.join_calendar_meeting(
                                            join_url.as_deref(),
                                            &subject,
                                            cx,
                                        )
                                    });
                                }),
                            ),
                        )
                    }),
            )
            .on_click(move |_, _, cx| {
                open_state.update(cx, |state, cx| {
                    state.open_meeting_chat(thread_id.as_deref(), cx)
                });
            })
    }

    fn grid(
        &self,
        days: &[WeekDay],
        events: &[CalendarEvent],
        today: NaiveDate,
        now: DateTime<Utc>,
    ) -> impl IntoElement {
        let total_height = 24. * HOUR_HEIGHT;
        let now_minute = {
            let local = now.with_timezone(&Local);
            local.hour() * 60 + local.minute()
        };
        div()
            .id("calendar-grid")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                h_flex()
                    .h(px(total_height))
                    .w_full()
                    .child(
                        v_flex()
                            .w(px(GUTTER_WIDTH))
                            .flex_none()
                            .children((0..24).map(|hour| {
                                div()
                                    .h(px(HOUR_HEIGHT))
                                    .pr(px(8.))
                                    .flex()
                                    .justify_end()
                                    .text_size(px(11.))
                                    .text_color(theme::text_muted())
                                    .when(hour > 0, |label| label.child(hour_label(hour)))
                            })),
                    )
                    .children(days.iter().map(|day| {
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .border_l_1()
                            .border_color(theme::border())
                            .children((1..24).map(|hour| {
                                div()
                                    .absolute()
                                    .top(px(hour as f32 * HOUR_HEIGHT))
                                    .left_0()
                                    .right_0()
                                    .h(px(1.))
                                    .bg(theme::border())
                            }))
                            .children(day.blocks.iter().map(|block| {
                                self.event_block(block, &events[block.event_index], now)
                            }))
                            .when(day.date == today, |column| {
                                column.child(
                                    div()
                                        .absolute()
                                        .top(px(now_minute as f32 / 60. * HOUR_HEIGHT))
                                        .left_0()
                                        .right_0()
                                        .h(px(2.))
                                        .bg(theme::red()),
                                )
                            })
                    })),
            )
    }
}

fn event_fill(response: ResponseType) -> Hsla {
    match response {
        ResponseType::NotResponded => theme::surface_raised(),
        ResponseType::Tentative => theme::accent().opacity(EVENT_FILL_OPACITY / 2.),
        _ => theme::accent().opacity(EVENT_FILL_OPACITY),
    }
}

fn event_title(event: &CalendarEvent) -> String {
    if event.cancelled {
        format!("{CANCELED_PREFIX}{}", event.subject)
    } else {
        event.subject.clone()
    }
}

impl Render for CalendarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = f32::from(window.viewport_size().width) < COMPACT_HEADER_WIDTH;
        if !self.scrolled_to_morning {
            self.scrolled_to_morning = true;
            self.scroll
                .set_offset(point(px(0.), px(-FIRST_VISIBLE_HOUR * HOUR_HEIGHT)));
        }
        let state = self.state.read(cx);
        let start = state.calendar_start();
        let events = state.calendar_week.events.clone();
        let offset = Local::now().offset().fix();
        let days = week_days(&events, start, WORK_WEEK_DAYS, offset);
        let today = Local::now().date_naive();
        let now = Utc::now();
        v_flex()
            .debug_selector(|| "shell/islands/calendar".into())
            .flex_1()
            .w_full()
            .min_w_0()
            .h_full()
            .rounded(px(10.))
            .bg(theme::surface())
            .overflow_hidden()
            .child(self.header(start, compact))
            .child(self.day_headers(&days, today))
            .children(self.all_day_row(&days, &events))
            .child(self.grid(&days, &events, today, now))
    }
}
