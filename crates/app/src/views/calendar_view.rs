use std::time::Duration;

use app_base::format::{self, clock_format};
use chatsvc::{CalendarEvent, ResponseType, join_info_text};
use chrono::{DateTime, Local, NaiveDate, Offset, Timelike, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::calendar_date_picker::date_picker_button;
use super::calendar_month::month_grid;
use super::join_dialog::open_join_with_id;
use super::new_meeting_dialog::open_new_meeting;
use super::rsvp_menu::rsvp_menu;
use super::scroll_edge::scroll_offset;
use crate::app_state::AppState;
use crate::call::calendar_now::{
    DEFAULT_GRID_VIEWPORT, FollowNow, MANUAL_SCROLL_EPSILON, NOW_BIAS_FRACTION,
    now_scroll_offset_y, offset_differs, shows_hour_grid,
};
use crate::call::join_available;
use crate::call::week_grid::{
    CalendarSpan, EventLook, WeekBlock, WeekDay, event_look, hour_label, month_shown, week_days,
};
use crate::theme;

const HOUR_HEIGHT: f32 = 120.;
const QUARTERS_PER_HOUR: usize = 4;
const QUARTER_LINE_OPACITY: f32 = 0.45;
const GUTTER_WIDTH: f32 = 50.;
const HEADER_HEIGHT: f32 = 52.;
const DAY_HEADER_HEIGHT: f32 = 56.;
const ALL_DAY_HEIGHT: f32 = 26.;
const BLOCK_GAP: f32 = 2.;
const CANCELED_PREFIX: &str = "Canceled: ";
const COPY_JOIN_INFO: &str = "Copy join info";
const COMPACT_HEADER_WIDTH: f32 = 960.;
const BUSY_FILL_OPACITY: f32 = 0.28;
const FREE_FILL_OPACITY: f32 = 0.12;
const STATUS_BAR_WIDTH: f32 = 4.;
const HATCH_STRIPE: f32 = 1.5;
const HATCH_GAP: f32 = 2.5;
const SMALL_TITLE_HEIGHT: f32 = 20.;
const TODAY_MARK_HEIGHT: f32 = 3.;
const NOW_DOT_SIZE: f32 = 8.;
const CLOCK_TICK: Duration = Duration::from_secs(30);
const CHROME_HEIGHT: f32 = HEADER_HEIGHT + DAY_HEADER_HEIGHT + ALL_DAY_HEIGHT;

pub struct CalendarView {
    state: Entity<AppState>,
    scroll: ScrollHandle,
    follow_now: FollowNow,
    seen_scroll_to_now: u64,
    last_programmatic_offset: f32,
    skip_manual_check: bool,
    clock_running: bool,
    _observation: Subscription,
}

impl CalendarView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&state, |_, _, cx| cx.notify());
        CalendarView {
            state,
            scroll: ScrollHandle::new(),
            follow_now: FollowNow { active: true },
            seen_scroll_to_now: 0,
            last_programmatic_offset: 0.,
            skip_manual_check: false,
            clock_running: false,
            _observation: observation,
        }
    }

    fn grid_viewport_height(window: &Window) -> f32 {
        (f32::from(window.viewport_size().height) - CHROME_HEIGHT).max(DEFAULT_GRID_VIEWPORT * 0.5)
    }

    fn align_scroll_to_now(&mut self, window: &Window) {
        let local = Local::now();
        let offset = now_scroll_offset_y(
            local.hour(),
            local.minute(),
            HOUR_HEIGHT,
            Self::grid_viewport_height(window),
            NOW_BIAS_FRACTION,
        );
        self.scroll.set_offset(point(px(0.), px(offset)));
        self.last_programmatic_offset = offset;
        self.skip_manual_check = true;
    }

    fn sync_follow_from_state(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (scroll_to_now, span, today_visible) = {
            let state = self.state.read(cx);
            let start = state.calendar_start();
            let today = Local::now().date_naive();
            let today_visible = start <= today
                && today < start + chrono::Duration::days(state.calendar_week.span.days());
            (
                state.calendar_scroll_to_now,
                state.calendar_week.span,
                today_visible,
            )
        };
        if !shows_hour_grid(span) {
            self.stop_clock();
            return;
        }
        self.ensure_clock(cx);
        if scroll_to_now != self.seen_scroll_to_now {
            self.seen_scroll_to_now = scroll_to_now;
            self.follow_now.request_align();
        }
        if self.skip_manual_check {
            self.skip_manual_check = false;
        } else if self.follow_now.active
            && offset_differs(
                f32::from(scroll_offset(&self.scroll).y),
                self.last_programmatic_offset,
                MANUAL_SCROLL_EPSILON,
            )
        {
            self.follow_now.note_manual_scroll();
        }
        if self.follow_now.should_follow(today_visible) {
            self.align_scroll_to_now(window);
        }
    }

    fn ensure_clock(&mut self, cx: &mut Context<Self>) {
        if self.clock_running {
            return;
        }
        self.clock_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_TICK).await;
                let keep = this
                    .update(cx, |view, cx| {
                        let span = view.state.read(cx).calendar_week.span;
                        if !shows_hour_grid(span) {
                            view.clock_running = false;
                            return false;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    fn stop_clock(&mut self) {
        self.clock_running = false;
    }

    fn on_grid_scroll_wheel(
        &mut self,
        _event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.follow_now.note_manual_scroll();
        cx.notify();
    }

    fn header(&self, start: NaiveDate, span: CalendarSpan, compact: bool) -> impl IntoElement {
        let span_state = self.state.clone();
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
                    .tooltip(match span {
                        CalendarSpan::Day => "Go to previous day",
                        CalendarSpan::Month => "Go to previous month",
                        _ => "Go to previous week",
                    })
                    .on_click(move |_, _, cx| {
                        back.update(cx, |state, cx| state.shift_calendar_week(-1, cx))
                    }),
            )
            .child(
                Button::new("calendar-forward")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronRight)
                    .tooltip(match span {
                        CalendarSpan::Day => "Go to next day",
                        CalendarSpan::Month => "Go to next month",
                        _ => "Go to next week",
                    })
                    .on_click(move |_, _, cx| {
                        forward.update(cx, |state, cx| state.shift_calendar_week(1, cx))
                    }),
            )
            .child(h_flex().flex_1().min_w_0().child(date_picker_button(
                &self.state,
                span.title(start),
                start,
                span.days(),
                if span == CalendarSpan::Month {
                    month_shown(start)
                } else {
                    start
                },
            )))
            .child(
                Button::new("calendar-span")
                    .small()
                    .ghost()
                    .label(span.label())
                    .dropdown_caret(true)
                    .dropdown_menu(move |menu, _, _| {
                        CalendarSpan::ALL.iter().fold(menu, |menu, &choice| {
                            let state = span_state.clone();
                            menu.item(
                                PopupMenuItem::new(choice.label())
                                    .checked(choice == span)
                                    .on_click(move |_, _, cx| {
                                        state.update(cx, |state, cx| {
                                            state.set_calendar_span(choice, cx)
                                        })
                                    }),
                            )
                        })
                    }),
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
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .justify_center()
                    .px(px(12.))
                    .border_l_1()
                    .border_color(theme::border())
                    .when(is_today, |header| {
                        header.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(TODAY_MARK_HEIGHT))
                                .bg(theme::accent()),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .text_color(color)
                            .child(format::calendar_day_number(day.date)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(color)
                            .child(day.date.format("%a").to_string()),
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
                                .bg(event_fill(event_look(&events[index])))
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
        send_response: bool,
    ) -> impl IntoElement {
        let top = block.start_minute as f32 / 60. * HOUR_HEIGHT;
        let height = ((block.end_minute - block.start_minute) as f32 / 60. * HOUR_HEIGHT
            - BLOCK_GAP)
            .max(14.);
        let width = 1. / block.lanes as f32;
        let compact = height < 36.;
        let small = height < SMALL_TITLE_HEIGHT;
        let look = event_look(event);
        let can_join = event.online
            && !event.cancelled
            && event.join_url.is_some()
            && join_available(event.start, event.end, now);
        let (open_state, thread_id) = (self.state.clone(), event.thread_id.clone());
        let (menu_state, menu_event) = (self.state.clone(), event.clone());
        let (join_state, join_url, subject) = (
            self.state.clone(),
            event.join_url.clone(),
            event.subject.clone(),
        );
        let (title_color, detail_color) = match look {
            EventLook::Busy => (theme::text(), theme::text_soft()),
            EventLook::Free | EventLook::Tentative => (theme::text_soft(), theme::text_muted()),
        };
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
            .px(px(BLOCK_GAP))
            .child(
                h_flex()
                    .size_full()
                    .items_start()
                    .overflow_hidden()
                    .rounded(px(4.))
                    .bg(event_fill(look))
                    .when(event.cancelled, |block| block.opacity(0.6))
                    .cursor_pointer()
                    .children(status_bar(look))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .pl(px(5.))
                            .pr(px(4.))
                            .h_full()
                            .py(px(if compact { 1. } else { 4. }))
                            .when(compact, |body| body.justify_center())
                            .child(
                                div()
                                    .text_size(px(if small { 10. } else { 12. }))
                                    .line_height(px(if small { 13. } else { 16. }))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(title_color)
                                    .when(compact, |title| title.truncate())
                                    .when(!compact, |title| title.line_clamp(2))
                                    .child(event_title(event)),
                            )
                            .when(!compact, |body| {
                                body.children(event.organizer_name.clone().map(|organizer| {
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(detail_color)
                                        .truncate()
                                        .child(organizer)
                                }))
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
            .context_menu(move |menu, _, _| {
                event_menu(menu, &menu_state, &menu_event, send_response)
            })
    }

    fn grid(
        &self,
        days: &[WeekDay],
        events: &[CalendarEvent],
        today: NaiveDate,
        now: DateTime<Utc>,
        send_response: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let total_height = 24. * HOUR_HEIGHT;
        let twelve_hour = clock_format().contains("%p") || clock_format().contains("%P");
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
            .on_scroll_wheel(cx.listener(Self::on_grid_scroll_wheel))
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
                                    .pt(px(4.))
                                    .flex()
                                    .justify_center()
                                    .text_size(px(12.))
                                    .text_color(theme::text_soft())
                                    .when(hour > 0, |label| {
                                        label.child(hour_label(hour, twelve_hour))
                                    })
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
                            .children((1..24 * QUARTERS_PER_HOUR).map(|quarter| {
                                let line = div()
                                    .absolute()
                                    .top(
                                        px(quarter as f32 * HOUR_HEIGHT / QUARTERS_PER_HOUR as f32),
                                    )
                                    .left_0()
                                    .right_0();
                                if quarter % QUARTERS_PER_HOUR == 0 {
                                    line.h(px(1.)).bg(theme::border())
                                } else {
                                    line.h(px(1.))
                                        .bg(theme::border().opacity(QUARTER_LINE_OPACITY))
                                }
                            }))
                            .children(day.blocks.iter().map(|block| {
                                self.event_block(
                                    block,
                                    &events[block.event_index],
                                    now,
                                    send_response,
                                )
                            }))
                            .when(day.date == today, |column| {
                                let now_top = now_minute as f32 / 60. * HOUR_HEIGHT;
                                column
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(now_top - 1.))
                                            .left_0()
                                            .right_0()
                                            .h(px(2.))
                                            .bg(theme::accent()),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(now_top - NOW_DOT_SIZE / 2.))
                                            .left_0()
                                            .size(px(NOW_DOT_SIZE))
                                            .rounded_full()
                                            .bg(theme::accent()),
                                    )
                            })
                    })),
            )
    }
}

fn event_menu(
    menu: PopupMenu,
    state: &Entity<AppState>,
    event: &CalendarEvent,
    send_response: bool,
) -> PopupMenu {
    let mut menu = menu;
    if let Some(join_url) = event
        .join_url
        .clone()
        .filter(|_| event.online && !event.cancelled)
    {
        let (join_state, subject) = (state.clone(), event.subject.clone());
        menu = menu.item(PopupMenuItem::new("Join").icon(IconName::Video).on_click(
            move |_, _, cx| {
                join_state.update(cx, |state, cx| {
                    state.join_calendar_meeting(Some(&join_url), &subject, cx)
                })
            },
        ));
    }
    if let Some(join_url) = event.join_url.as_deref().filter(|_| event.online) {
        let info = join_info_text(join_url, event.dial_in.as_ref());
        menu = menu.item(
            PopupMenuItem::new(COPY_JOIN_INFO)
                .icon(IconName::Copy)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(info.clone()))
                }),
        );
    }
    if let Some(thread_id) = event.thread_id.clone() {
        let chat_state = state.clone();
        menu = menu.item(
            PopupMenuItem::new("Chat with participants")
                .icon(IconName::MessageSquare)
                .on_click(move |_, _, cx| {
                    chat_state.update(cx, |state, cx| {
                        state.open_meeting_chat(Some(&thread_id), cx)
                    })
                }),
        );
    }
    if event.response == ResponseType::Organizer || event.organizer_name.is_none() {
        return menu;
    }
    rsvp_menu(
        menu.separator(),
        state,
        &event.object_id,
        event.response,
        send_response,
    )
}

fn event_fill(look: EventLook) -> Hsla {
    match look {
        EventLook::Busy => theme::accent().opacity(BUSY_FILL_OPACITY),
        EventLook::Free | EventLook::Tentative => theme::accent().opacity(FREE_FILL_OPACITY),
    }
}

fn status_bar(look: EventLook) -> Option<Div> {
    let bar = div().flex_none().w(px(STATUS_BAR_WIDTH)).h_full();
    match look {
        EventLook::Busy => Some(bar.bg(theme::accent())),
        EventLook::Tentative => Some(
            bar.bg(theme::accent().opacity(BUSY_FILL_OPACITY)).child(
                div()
                    .size_full()
                    .bg(pattern_slash(theme::accent(), HATCH_STRIPE, HATCH_GAP)),
            ),
        ),
        EventLook::Free => None,
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
        self.sync_follow_from_state(window, cx);
        let state = self.state.read(cx);
        let start = state.calendar_start();
        let events = state.calendar_week.events.clone();
        let send_response = state.rsvp_send_response;
        let offset = Local::now().offset().fix();
        let span = state.calendar_week.span;
        let days = week_days(&events, start, span.days(), offset);
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
            .child(self.header(start, span, compact))
            .map(|calendar| {
                if span == CalendarSpan::Month {
                    calendar.child(month_grid(
                        &self.state,
                        start,
                        &days,
                        &events,
                        today,
                        offset,
                    ))
                } else {
                    calendar
                        .child(self.day_headers(&days, today))
                        .children(self.all_day_row(&days, &events))
                        .child(self.grid(&days, &events, today, now, send_response, cx))
                }
            })
    }
}
