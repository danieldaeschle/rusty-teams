use std::time::Duration;

use app_base::format::{self, WeekdayWidth, clock_format, weekday_name};
use chatsvc::{CalendarEvent, ResponseType, join_info_text};
use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, Offset, Timelike, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::calendar_date_picker::date_picker_button;
use super::calendar_event_dialog::open_calendar_event;
use super::calendar_month::month_grid;
use super::calendar_picker;
use super::join_dialog::open_join_with_id;
use super::meeting_proposal_card::meeting_proposal_cards;
use super::new_meeting_dialog::{open_new_meeting, open_new_meeting_at};
use super::rsvp_menu::rsvp_menu;
use super::scroll_edge::scroll_offset;
use crate::app_state::AppState;
use crate::call::calendar_now::{
    DEFAULT_GRID_VIEWPORT, FollowNow, MANUAL_SCROLL_EPSILON, NOW_BIAS_FRACTION, hour_height,
    now_scroll_offset_y, offset_differs, shows_hour_grid, work_start_scroll_offset_y,
};
use crate::call::schedule::DEFAULT_LENGTH_MINUTES;
use crate::call::slot_select::{SlotSelection, minute_at};
use crate::call::week::CalendarLoadFailure;
use crate::call::week_grid::{
    CalendarSpan, EventLook, WeekBlock, WeekDay, WeekLayout, calendar_index, event_look,
    hour_label, month_shown, week_days,
};
use crate::i18n::tr;
use crate::theme;
use crate::views::widgets::icon;

const GUTTER_WIDTH: f32 = 50.;
const HEADER_HEIGHT: f32 = 52.;
const DAY_HEADER_HEIGHT: f32 = 56.;
const ALL_DAY_HEIGHT: f32 = 26.;
const BLOCK_GAP: f32 = 2.;
const MINOR_LINE_OPACITY: f32 = 0.45;
const OFF_HOURS_OPACITY: f32 = 0.85;
const NOW_ELSEWHERE_OPACITY: f32 = 0.4;
pub const CALENDAR_HEADER_TAG: &str = "shell/islands/calendar/header";
pub const CALENDAR_DAY_HEADERS_TAG: &str = "shell/islands/calendar/day-headers";
pub const CALENDAR_NEW_MEETING_TAG: &str = "shell/islands/calendar/header/new-meeting";
pub const COMPACT_HEADER_WIDTH: f32 = 960.;
/// Header widths below which button labels give way to icons (German labels are longest).
const SECONDARY_LABELS_WIDTH: f32 = 1080.;
const PRIMARY_LABELS_WIDTH: f32 = 780.;

/// Which header buttons keep their text at a given header width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderLabels {
    /// Today, view and New meeting.
    pub primary: bool,
    /// Join with an ID and Meet now.
    pub secondary: bool,
}

pub fn header_labels(header_width: f32) -> HeaderLabels {
    HeaderLabels {
        primary: header_width >= PRIMARY_LABELS_WIDTH,
        secondary: header_width >= SECONDARY_LABELS_WIDTH,
    }
}
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
const SELECTION_FILL_OPACITY: f32 = 0.24;
/// Height below which the picked block drops its time label.
const SELECTION_LABEL_HEIGHT: f32 = 18.;

/// A left press on an empty slot that is still held; the block grows from `anchor`.
#[derive(Debug, Clone, Copy)]
struct SlotDrag {
    date: NaiveDate,
    anchor: i64,
}

pub struct CalendarView {
    state: Entity<AppState>,
    scroll: ScrollHandle,
    follow_now: FollowNow,
    seen_scroll_to_now: u64,
    last_programmatic_offset: f32,
    skip_manual_check: bool,
    clock_running: bool,
    /// Row height of one hour and minor lines per hour, from the time scale.
    hour_height: f32,
    steps_per_hour: usize,
    /// Block of empty slots picked on the hour grid for a new meeting.
    slot_selection: Option<SlotSelection>,
    slot_drag: Option<SlotDrag>,
    /// Focus for the hour grid, so Escape can drop the picked block.
    grid_focus: FocusHandle,
    _observation: Subscription,
    _agent_observation: Option<Subscription>,
}

impl CalendarView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&state, |_, _, cx| cx.notify());
        let agent_observation =
            crate::agent_api::handle(cx).map(|api| cx.observe(&api, |_, _, cx| cx.notify()));
        CalendarView {
            state,
            scroll: ScrollHandle::new(),
            follow_now: FollowNow { active: true },
            seen_scroll_to_now: 0,
            last_programmatic_offset: 0.,
            skip_manual_check: false,
            clock_running: false,
            hour_height: hour_height(Default::default()),
            steps_per_hour: 2,
            slot_selection: None,
            slot_drag: None,
            grid_focus: cx.focus_handle(),
            _observation: observation,
            _agent_observation: agent_observation,
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
            self.hour_height,
            Self::grid_viewport_height(window),
            NOW_BIAS_FRACTION,
        );
        self.set_programmatic_offset(offset);
    }

    /// A range without today opens at work start instead of midnight.
    fn align_scroll_to_work_start(&mut self, work_start_minute: i64, window: &Window) {
        let offset = work_start_scroll_offset_y(
            work_start_minute,
            self.hour_height,
            Self::grid_viewport_height(window),
        );
        self.set_programmatic_offset(offset);
    }

    fn set_programmatic_offset(&mut self, offset: f32) {
        self.scroll.set_offset(point(px(0.), px(offset)));
        self.last_programmatic_offset = offset;
        self.skip_manual_check = true;
    }

    fn sync_follow_from_state(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (scroll_to_now, span, today_visible, work_start) = {
            let state = self.state.read(cx);
            let today_visible = state.calendar_shows_today();
            let prefs = &state.calendar_prefs;
            self.hour_height = hour_height(prefs.time_scale);
            self.steps_per_hour = prefs.time_scale.steps_per_hour() as usize;
            (
                state.calendar_scroll_to_now,
                state.calendar_week.span,
                today_visible,
                prefs.work_hours.minute_range().0,
            )
        };
        if !shows_hour_grid(span) {
            self.stop_clock();
            return;
        }
        self.ensure_clock(cx);
        if scroll_to_now != self.seen_scroll_to_now {
            self.seen_scroll_to_now = scroll_to_now;
            if today_visible {
                self.follow_now.request_align();
            } else {
                self.align_scroll_to_work_start(work_start, window);
                return;
            }
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.follow_now.note_manual_scroll();
        if self.slot_drag.is_some() {
            // A wheel turn moves the grid under a still pointer; follow it once the
            // new offset is in.
            cx.on_next_frame(window, |view, window, cx| {
                view.drag_slot_to(window.mouse_position(), cx)
            });
        }
        cx.notify();
    }

    fn step_minutes(&self) -> i64 {
        60 / self.steps_per_hour.max(1) as i64
    }

    /// Grid step under a window position; the day's columns scroll together, so the
    /// scroll container's top plus its offset is where midnight sits.
    fn minute_at_position(&self, position: Point<Pixels>) -> i64 {
        let content_top =
            f32::from(self.scroll.bounds().top()) + f32::from(scroll_offset(&self.scroll).y);
        minute_at(
            f32::from(position.y) - content_top,
            self.hour_height,
            self.step_minutes(),
        )
    }

    fn one_slot(&self, date: NaiveDate, position: Point<Pixels>) -> SlotSelection {
        let minute = self.minute_at_position(position);
        SlotSelection::between(date, minute, minute, self.step_minutes())
    }

    /// Left press on an empty slot: start picking a block, or a double click opens
    /// New meeting at that slot with the default meeting length, as Teams does.
    fn press_slot(
        &mut self,
        date: NaiveDate,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = self.one_slot(date, event.position);
        if event.click_count >= 2 {
            self.clear_slot_selection(cx);
            let meeting =
                SlotSelection::starting_at(date, slot.start_minute, DEFAULT_LENGTH_MINUTES);
            open_meeting_for(self.state.clone(), meeting, window, cx);
            return;
        }
        self.slot_drag = Some(SlotDrag {
            date,
            anchor: slot.start_minute,
        });
        self.slot_selection = Some(slot);
        cx.notify();
    }

    fn drag_slot_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.slot_drag else {
            return;
        };
        let minute = self.minute_at_position(position);
        let selection = SlotSelection::between(drag.date, drag.anchor, minute, self.step_minutes());
        if self.slot_selection != Some(selection) {
            self.slot_selection = Some(selection);
            cx.notify();
        }
    }

    fn end_slot_drag(&mut self, cx: &mut Context<Self>) {
        if self.slot_drag.take().is_some() {
            cx.notify();
        }
    }

    /// Right press on an empty slot: keep the picked block when the press is inside
    /// it, otherwise pick just that slot, so the menu always has a block to use.
    fn right_press_slot(
        &mut self,
        date: NaiveDate,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.slot_drag = None;
        let minute = self.minute_at_position(event.position);
        if !self
            .slot_selection
            .is_some_and(|selection| selection.contains(date, minute))
        {
            self.slot_selection = Some(self.one_slot(date, event.position));
        }
        cx.notify();
    }

    fn clear_slot_selection(&mut self, cx: &mut Context<Self>) {
        self.slot_drag = None;
        if self.slot_selection.take().is_some() {
            cx.notify();
        }
    }

    /// While a block is being picked, follow the pointer outside the grid and over
    /// meetings too. Releases are caught on the grid itself (see `grid`), whose
    /// listeners already exist when the press lands.
    fn slot_drag_listener(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moved = view.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    let held = event.pressed_button == Some(MouseButton::Left);
                    moved
                        .update(cx, |view, cx| {
                            if held {
                                view.drag_slot_to(event.position, cx);
                            } else {
                                view.end_slot_drag(cx);
                            }
                        })
                        .ok();
                });
            },
        )
        .absolute()
        .size_full()
    }

    /// Invisible layer under a day's meetings that takes presses on empty slots.
    fn slot_layer(&self, date: NaiveDate, cx: &Context<Self>) -> impl IntoElement {
        let (menu_view, menu_state) = (cx.entity().downgrade(), self.state.clone());
        div()
            .id(SharedString::from(format!("calendar-slots-{date}")))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // Capture runs before the context menu notes which element to give focus
            // back to, so Escape reaches the grid after the menu closes.
            .capture_any_mouse_down(cx.listener(|view, _: &MouseDownEvent, window, cx| {
                window.focus(&view.grid_focus, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    view.press_slot(date, event, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.right_press_slot(date, event, cx)
                }),
            )
            .context_menu(move |menu, _, _| {
                let (view, state) = (menu_view.clone(), menu_state.clone());
                menu.item(
                    PopupMenuItem::new(tr("calendar.view.new_meeting"))
                        .icon(IconName::Plus)
                        .on_click(move |_, window, cx| {
                            let picked = view
                                .update(cx, |view, cx| {
                                    let picked = view.slot_selection;
                                    view.clear_slot_selection(cx);
                                    picked
                                })
                                .ok()
                                .flatten();
                            match picked {
                                Some(picked) => open_meeting_for(state.clone(), picked, window, cx),
                                None => open_new_meeting(state.clone(), window, cx),
                            }
                        }),
                )
            })
    }

    /// The picked block: an accent fill and bar, with its times when there is room.
    fn selection_block(&self, selection: &SlotSelection) -> Div {
        let row = self.hour_height;
        let top = selection.start_minute as f32 / 60. * row;
        let height = (selection.end_minute - selection.start_minute) as f32 / 60. * row;
        // The times New meeting will open with, so the label and the form agree.
        let label = selection
            .form_times(grid_offset(), &Local)
            .label(clock_format());
        div()
            .absolute()
            .top(px(top))
            .h(px(height))
            .left_0()
            .right_0()
            .px(px(BLOCK_GAP))
            .child(
                h_flex()
                    .size_full()
                    .items_start()
                    .overflow_hidden()
                    .rounded(px(6.))
                    .bg(theme::accent().opacity(SELECTION_FILL_OPACITY))
                    .child(
                        div()
                            .flex_none()
                            .w(px(STATUS_BAR_WIDTH))
                            .h_full()
                            .bg(theme::accent()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pl(px(5.))
                            .pr(px(4.))
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(theme::accent_text())
                            .truncate()
                            .when(height >= SELECTION_LABEL_HEIGHT, |text| text.child(label)),
                    ),
            )
    }

    fn header(
        &self,
        start: NaiveDate,
        span: CalendarSpan,
        dates: &[NaiveDate],
        layout: &WeekLayout,
        compact: bool,
        labels: HeaderLabels,
    ) -> impl IntoElement {
        let span_state = self.state.clone();
        let label = |key: &'static str| labels.primary.then(|| tr(key));
        let secondary = |key: &'static str| labels.secondary.then(|| tr(key));
        let (today, back, forward) = (self.state.clone(), self.state.clone(), self.state.clone());
        let (join_state, meet_state, new_state) =
            (self.state.clone(), self.state.clone(), self.state.clone());
        let pane_state = self.state.clone();
        let (back_tip, forward_tip) = match span {
            CalendarSpan::Day => ("calendar.view.previous_day", "calendar.view.next_day"),
            CalendarSpan::Month => ("calendar.view.previous_month", "calendar.view.next_month"),
            _ => ("calendar.view.previous_week", "calendar.view.next_week"),
        };
        h_flex()
            .debug_selector(|| CALENDAR_HEADER_TAG.into())
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .when(!compact, |header| {
                header.child(
                    Button::new("calendar-pane-toggle")
                        .small()
                        .ghost()
                        .icon(IconName::PanelLeft)
                        .tooltip(tr("calendar.view.toggle_pane"))
                        .on_click(move |_, _, cx| {
                            pane_state.update(cx, |state, cx| {
                                state.update_calendar_prefs(
                                    |prefs| prefs.pane_hidden = !prefs.pane_hidden,
                                    cx,
                                )
                            })
                        }),
                )
            })
            .child(
                Button::new("calendar-today")
                    .small()
                    .outline()
                    .icon(IconName::Calendar)
                    .when_some(label("calendar.view.today"), |button, text| {
                        button.label(text)
                    })
                    .tooltip(tr("calendar.view.today"))
                    .on_click(move |_, _, cx| {
                        today.update(cx, |state, cx| state.calendar_to_today(cx))
                    }),
            )
            .child(
                Button::new("calendar-back")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronLeft)
                    .tooltip(tr(back_tip))
                    .on_click(move |_, _, cx| {
                        back.update(cx, |state, cx| state.shift_calendar_week(-1, cx))
                    }),
            )
            .child(
                Button::new("calendar-forward")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronRight)
                    .tooltip(tr(forward_tip))
                    .on_click(move |_, _, cx| {
                        forward.update(cx, |state, cx| state.shift_calendar_week(1, cx))
                    }),
            )
            .child(h_flex().flex_1().min_w_0().child(date_picker_button(
                &self.state,
                span.title(start, layout),
                dates.to_vec(),
                layout.first_day,
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
                    .icon(IconName::LayoutGrid)
                    .when(labels.primary, |button| button.label(span.label()))
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
                    .when_some(secondary("calendar.view.join_with_id"), |button, text| {
                        button.label(text)
                    })
                    .tooltip(tr("calendar.view.join_with_id"))
                    .on_click(move |_, window, cx| {
                        open_join_with_id(join_state.clone(), window, cx)
                    }),
            )
            .child(
                Button::new("calendar-meet-now")
                    .small()
                    .outline()
                    .icon(IconName::Video)
                    .when_some(secondary("calendar.view.meet_now"), |button, text| {
                        button.label(text)
                    })
                    .tooltip(tr("calendar.view.meet_now"))
                    .on_click(move |_, _, cx| {
                        meet_state.update(cx, |state, cx| state.start_meet_now(cx))
                    }),
            )
            .child(
                Button::new("calendar-new-meeting")
                    .debug_selector(|| CALENDAR_NEW_MEETING_TAG.into())
                    .small()
                    .primary()
                    .icon(IconName::Plus)
                    .when_some(label("calendar.view.new_meeting"), |button, text| {
                        button.label(text)
                    })
                    .tooltip(tr("calendar.view.new_meeting"))
                    .on_click(move |_, window, cx| open_new_meeting(new_state.clone(), window, cx)),
            )
    }

    /// Day number above the full weekday name; today in the accent colour with its bar.
    fn day_headers(&self, days: &[WeekDay], today: NaiveDate) -> impl IntoElement {
        h_flex()
            .debug_selector(|| CALENDAR_DAY_HEADERS_TAG.into())
            .h(px(DAY_HEADER_HEIGHT))
            .flex_none()
            .border_b_1()
            .border_color(theme::border())
            .child(div().w(px(GUTTER_WIDTH)).flex_none())
            .children(days.iter().map(|day| {
                let is_today = day.date == today;
                let (number_color, name_color) = if is_today {
                    (theme::accent_text(), theme::accent_text())
                } else {
                    (theme::text(), theme::text_soft())
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
                            .text_color(number_color)
                            .when(is_today, |number| number.font_weight(FontWeight::SEMIBOLD))
                            .child(format::calendar_day_number(day.date)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(name_color)
                            .truncate()
                            .child(weekday_name(day.date.weekday(), WeekdayWidth::Full)),
                    )
            }))
    }

    fn all_day_row(
        &self,
        days: &[WeekDay],
        events: &[CalendarEvent],
        colors: &[Hsla],
        send_response: bool,
    ) -> Option<Div> {
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
                            let event = events[index].clone();
                            let color = colors[index];
                            let open_state = self.state.clone();
                            let (menu_state, menu_event) = (self.state.clone(), event.clone());
                            // Day-qualified id: multi-day all-day chips share object_id.
                            div()
                                .id(SharedString::from(format!(
                                    "calendar-all-day-{}-{}",
                                    event.object_id, day.date
                                )))
                                .h(px(ALL_DAY_HEIGHT - 2. * BLOCK_GAP))
                                .px(px(6.))
                                .flex()
                                .items_center()
                                .rounded(px(6.))
                                .bg(event_fill(event_look(&event), color))
                                .text_size(px(12.))
                                .truncate()
                                .cursor_pointer()
                                .child(event_title(&event))
                                .on_click(move |_, window, cx| {
                                    open_calendar_event(
                                        open_state.clone(),
                                        event.clone(),
                                        window,
                                        cx,
                                    );
                                })
                                .context_menu(move |menu, _, _| {
                                    event_menu(menu, &menu_state, &menu_event, send_response)
                                })
                        }))
                })),
        )
    }

    fn event_block(
        &self,
        block: &WeekBlock,
        event: &CalendarEvent,
        color: Hsla,
        day: NaiveDate,
        send_response: bool,
    ) -> impl IntoElement {
        let top = block.start_minute as f32 / 60. * self.hour_height;
        let height = ((block.end_minute - block.start_minute) as f32 / 60. * self.hour_height
            - BLOCK_GAP)
            .max(14.);
        let width = 1. / block.lanes as f32;
        let compact = height < 36.;
        let small = height < SMALL_TITLE_HEIGHT;
        let title_size = if small { 10. } else { 12. };
        let title_line = if small { 14. } else { 16. };
        let pad_y = if compact { 1. } else { 3. };
        // Title and organizer flow as one text; tall blocks wrap it over more lines.
        let lines = (((height - pad_y * 2.) / title_line).floor() as usize).clamp(1, 8);
        let look = event_look(event);
        // Join on the block whenever a Teams link exists (same as detail dialog).
        // Skip on tiny blocks — the button would crush the title.
        let can_join = !compact && event.online && !event.cancelled && event.join_url.is_some();
        let open_state = self.state.clone();
        let open_event = event.clone();
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
        // Day-qualified id: overnight / multi-day spans reuse object_id per column.
        div()
            .id(SharedString::from(format!(
                "calendar-event-{}-{}",
                event.object_id, day
            )))
            // Presses on a meeting must not reach the empty-slot layer under it.
            .block_mouse_except_scroll()
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
                    .rounded(px(6.))
                    .bg(event_fill(look, color))
                    .when(event.cancelled, |block| block.opacity(0.6))
                    .cursor_pointer()
                    .children(status_bar(look, color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pl(px(5.))
                            .pr(px(4.))
                            .h_full()
                            .py(px(pad_y))
                            .overflow_hidden()
                            .text_size(px(title_size))
                            .line_height(px(title_line))
                            .text_color(detail_color)
                            .line_clamp(lines)
                            .text_ellipsis()
                            .child(title_and_organizer(event, title_color)),
                    )
                    .when(event.series, |block| {
                        block.child(div().flex_none().pt(px(pad_y + 1.)).pr(px(4.)).child(icon(
                            IconName::Repeat,
                            12.,
                            detail_color,
                        )))
                    })
                    .when(can_join, |block| {
                        block.child(
                            div().flex_none().pr(px(4.)).pt(px(3.)).child(
                                Button::new(SharedString::from(format!(
                                    "calendar-join-{}-{}",
                                    event.object_id, day
                                )))
                                .xsmall()
                                .primary()
                                .label(tr("common.join"))
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
            .on_click(move |_, window, cx| {
                open_calendar_event(open_state.clone(), open_event.clone(), window, cx);
            })
            .context_menu(move |menu, _, _| {
                event_menu(menu, &menu_state, &menu_event, send_response)
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn grid(
        &self,
        days: &[WeekDay],
        events: &[CalendarEvent],
        colors: &[Hsla],
        layout: &WeekLayout,
        work_minutes: (i64, i64),
        today: NaiveDate,
        now: DateTime<Utc>,
        send_response: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row = self.hour_height;
        let steps = self.steps_per_hour;
        let total_height = 24. * row;
        let twelve_hour = clock_format().contains("%p") || clock_format().contains("%P");
        let now_minute = {
            let local = now.with_timezone(&Local);
            local.hour() * 60 + local.minute()
        };
        let now_top = now_minute as f32 / 60. * row;
        let today_shown = days.iter().any(|day| day.date == today);
        let minute_y = |minute: i64| minute as f32 / 60. * row;
        let selection = self.slot_selection;
        let dragging = self.slot_drag.is_some();
        div()
            .id("calendar-grid")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .track_focus(&self.grid_focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && view.slot_selection.is_some() {
                    view.clear_slot_selection(cx);
                    cx.stop_propagation();
                }
            }))
            .on_scroll_wheel(cx.listener(Self::on_grid_scroll_wheel))
            // Inside the grid, or outside it and over meetings (whose hitboxes block
            // the grid's): either way a left release ends the drag.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| view.end_slot_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| view.end_slot_drag(cx)),
            )
            .when(dragging, |grid| grid.child(self.slot_drag_listener(cx)))
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
                                    .h(px(row))
                                    .pt(px(2.))
                                    .pr(px(8.))
                                    .flex()
                                    .justify_end()
                                    .text_size(px(11.))
                                    .text_color(theme::text_muted())
                                    .when(hour > 0, |label| {
                                        label.child(hour_label(hour, twelve_hour))
                                    })
                            })),
                    )
                    .children(days.iter().map(|day| {
                        let work_day = layout.is_work_day(day.date.weekday());
                        let (work_start, work_end) = work_minutes;
                        let shade = |top: f32, height: f32| {
                            div()
                                .absolute()
                                .top(px(top))
                                .left_0()
                                .right_0()
                                .h(px(height))
                                .bg(theme::background().opacity(OFF_HOURS_OPACITY))
                        };
                        let shading = if work_day {
                            vec![
                                shade(0., minute_y(work_start)),
                                shade(minute_y(work_end), total_height - minute_y(work_end)),
                            ]
                        } else {
                            vec![shade(0., total_height)]
                        };
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .border_l_1()
                            .border_color(theme::border())
                            .child(self.slot_layer(day.date, cx))
                            .children(shading)
                            .children((1..24 * steps).map(|step| {
                                let line = div()
                                    .absolute()
                                    .top(px(step as f32 * row / steps as f32))
                                    .left_0()
                                    .right_0()
                                    .h(px(1.));
                                if step % steps == 0 {
                                    line.bg(theme::border())
                                } else {
                                    line.bg(theme::border().opacity(MINOR_LINE_OPACITY))
                                }
                            }))
                            .children(
                                selection
                                    .filter(|selection| selection.date == day.date)
                                    .map(|selection| self.selection_block(&selection)),
                            )
                            .children(day.blocks.iter().map(|block| {
                                self.event_block(
                                    block,
                                    &events[block.event_index],
                                    colors[block.event_index],
                                    day.date,
                                    send_response,
                                )
                            }))
                            .when(today_shown && day.date != today, |column| {
                                column.child(
                                    div()
                                        .absolute()
                                        .top(px(now_top))
                                        .left_0()
                                        .right_0()
                                        .h(px(1.))
                                        .bg(theme::accent().opacity(NOW_ELSEWHERE_OPACITY)),
                                )
                            })
                            .when(day.date == today, |column| {
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

/// The one UTC offset the hour grid lays every shown day out with.
fn grid_offset() -> FixedOffset {
    Local::now().offset().fix()
}

/// New meeting for a block picked on the grid.
fn open_meeting_for(
    state: Entity<AppState>,
    picked: SlotSelection,
    window: &mut Window,
    cx: &mut App,
) {
    let form = picked.form_times(grid_offset(), &Local);
    open_new_meeting_at(state, form.date, form.start, form.end, window, cx);
}

/// Loading line and one notice per failed source, under the calendar header.
fn load_notices(
    state: &Entity<AppState>,
    failures: &[CalendarLoadFailure],
    loading: bool,
) -> Vec<Div> {
    let mut rows = Vec::new();
    if loading {
        rows.push(
            h_flex()
                .flex_none()
                .px(px(16.))
                .pb(px(6.))
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child(tr("calendar.view.loading")),
        );
    }
    for failure in failures {
        let retry = state.clone();
        let mut args = std::collections::HashMap::new();
        args.insert("source".to_owned(), failure.source.label().to_owned());
        rows.push(
            h_flex()
                .flex_none()
                .mx(px(16.))
                .mb(px(6.))
                .px(px(12.))
                .py(px(6.))
                .gap(px(12.))
                .items_center()
                .rounded(px(6.))
                .bg(theme::red_tint())
                .text_size(px(13.))
                .text_color(theme::text())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(crate::i18n::tr_args("calendar.view.source_failed", &args)),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "calendar-retry-{}",
                        failure.source.label()
                    )))
                    .xsmall()
                    .outline()
                    .label(tr("common.retry"))
                    .tooltip(failure.message.clone())
                    .on_click(move |_, _, cx| {
                        retry.update(cx, |state, cx| state.retry_calendar_week(cx))
                    }),
                ),
        );
    }
    rows
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
        menu = menu.item(
            PopupMenuItem::new(tr("common.join"))
                .icon(IconName::Video)
                .on_click(move |_, _, cx| {
                    join_state.update(cx, |state, cx| {
                        state.join_calendar_meeting(Some(&join_url), &subject, cx)
                    })
                }),
        );
    }
    if let Some(join_url) = event.join_url.as_deref().filter(|_| event.online) {
        let info = join_info_text(join_url, event.dial_in.as_ref());
        menu = menu.item(
            PopupMenuItem::new(tr("calendar.view.copy_join_info"))
                .icon(IconName::Copy)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(info.clone()))
                }),
        );
    }
    if let Some(thread_id) = event.thread_id.clone() {
        let chat_state = state.clone();
        menu = menu.item(
            PopupMenuItem::new(tr("calendar.view.chat_with_participants"))
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

fn event_fill(look: EventLook, color: Hsla) -> Hsla {
    match look {
        EventLook::Busy => color.opacity(BUSY_FILL_OPACITY),
        EventLook::Free | EventLook::Tentative => color.opacity(FREE_FILL_OPACITY),
    }
}

fn status_bar(look: EventLook, color: Hsla) -> Option<Div> {
    let bar = div().flex_none().w(px(STATUS_BAR_WIDTH)).h_full();
    match look {
        EventLook::Busy => Some(bar.bg(color)),
        EventLook::Tentative => Some(
            bar.bg(color.opacity(BUSY_FILL_OPACITY))
                .child(
                    div()
                        .size_full()
                        .bg(pattern_slash(color, HATCH_STRIPE, HATCH_GAP)),
                ),
        ),
        EventLook::Free => None,
    }
}

/// "Daily Standup Zulal Fouque": bold title, organizer right after it on the same line.
fn title_and_organizer(event: &CalendarEvent, title_color: Hsla) -> StyledText {
    let title = event_title(event);
    let text = match event.organizer_name.as_deref() {
        Some(organizer) if !organizer.is_empty() => format!("{title} {organizer}"),
        _ => title.clone(),
    };
    StyledText::new(text).with_highlights([(
        0..title.len(),
        HighlightStyle {
            color: Some(title_color),
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        },
    )])
}

fn event_title(event: &CalendarEvent) -> String {
    let subject = if event.cancelled {
        format!("{}{}", tr("calendar.peek.canceled_prefix"), event.subject)
    } else {
        event.subject.clone()
    };
    let label = source_label(event);
    if label.is_empty() {
        subject
    } else {
        format!("{subject} · {label}")
    }
}

fn source_label(event: &CalendarEvent) -> String {
    let mut sources = event.sources.clone();
    if sources.is_empty() {
        sources.push(event.source);
    }
    sources.sort_by_key(|source| source.label());
    sources.dedup();
    if sources.len() == 1 && sources[0] == chatsvc::CalendarSource::Outlook {
        return String::new();
    }
    sources
        .iter()
        .map(|source| source.label())
        .collect::<Vec<_>>()
        .join("+")
}

impl Render for CalendarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_width = f32::from(window.viewport_size().width);
        let compact = window_width < COMPACT_HEADER_WIDTH;
        self.sync_follow_from_state(window, cx);
        let state = self.state.read(cx);
        let start = state.calendar_start();
        let events = state.calendar_week.events.clone();
        let send_response = state.rsvp_send_response;
        let failures = state.calendar_week.failures.clone();
        let loading = state.calendar_week.loading && events.is_empty();
        let offset = grid_offset();
        let span = state.calendar_week.span;
        let layout = state.calendar_layout();
        let dates = state.calendar_dates();
        let days = week_days(&events, &dates, offset);
        // A picked block goes away once its day scrolls out of view or Month shows.
        if self.slot_selection.is_some_and(|picked| {
            span == CalendarSpan::Month || !picked.fits(&dates, self.step_minutes())
        }) {
            self.slot_selection = None;
            self.slot_drag = None;
        }
        let today = Local::now().date_naive();
        let now = Utc::now();
        let accounts = state.calendar_accounts();
        let colors: Vec<Hsla> = events
            .iter()
            .map(|event| theme::calendar_color(calendar_index(event, &accounts)))
            .collect();
        let work_minutes = state.calendar_prefs.work_hours.minute_range();
        let show_picker = !compact && !state.calendar_prefs.pane_hidden;
        // Width left for the header: window minus rail, island gaps and the pane.
        let header_width = window_width
            - app_ui_kit::layout::RAIL_WIDTH
            - 2. * app_ui_kit::layout::ISLAND_GAP
            - if show_picker {
                calendar_picker::PICKER_WIDTH
            } else {
                0.
            };
        let labels = header_labels(header_width);
        h_flex()
            .debug_selector(|| "shell/islands/calendar".into())
            .flex_1()
            .w_full()
            .min_w_0()
            .h_full()
            .rounded(px(10.))
            .bg(theme::surface())
            .overflow_hidden()
            .when(show_picker, |row| {
                row.child(calendar_picker::render(&self.state, cx))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.header(start, span, &dates, &layout, compact, labels))
                    .children(load_notices(&self.state, &failures, loading))
                    .children(meeting_proposal_cards(cx))
                    .map(|calendar| {
                        if span == CalendarSpan::Month {
                            calendar.child(month_grid(
                                &self.state,
                                start,
                                &days,
                                &events,
                                &colors,
                                today,
                                offset,
                                layout.weekdays(),
                            ))
                        } else {
                            calendar
                                .child(self.day_headers(&days, today))
                                .children(self.all_day_row(&days, &events, &colors, send_response))
                                .child(self.grid(
                                    &days,
                                    &events,
                                    &colors,
                                    &layout,
                                    work_minutes,
                                    today,
                                    now,
                                    send_response,
                                    cx,
                                ))
                        }
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::header_labels;

    #[test]
    fn header_buttons_drop_their_text_as_the_header_narrows() {
        let wide = header_labels(1300.);
        assert!(wide.primary && wide.secondary);
        let middle = header_labels(900.);
        assert!(middle.primary && !middle.secondary);
        let narrow = header_labels(600.);
        assert!(!narrow.primary && !narrow.secondary);
    }
}
