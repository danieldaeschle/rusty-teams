use std::time::Duration;

use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::new_chat::NewChatDraft;
use super::scroll_edge::{scroll_offset, with_top_edge};
use super::widgets::{icon, menu_icon, symbol};
use crate::app_state::AppState;
use crate::i18n::tr;
use crate::call::{
    CALL_ROW_HEIGHT, CallRow, EMPTY_HEIGHT, HistoryFilter, ListEntry, SpeedDialEntry,
};
use crate::theme;

pub const CALLS_DIAL_TAG: &str = "shell/islands/main/calls-dial";
pub const CALLS_HISTORY_TAG: &str = "shell/islands/main/calls-history";
pub const CALLS_SPEED_DIAL_TAG: &str = "shell/islands/main/calls-speed-dial";
const CALLS_SCROLL_DIVIDER: &str = "shell/islands/main/calls-history/divider-scroll";
const SPEED_DIAL_WIDTH: f32 = 340.;
const SPEED_DIAL_MIN_WINDOW_WIDTH: f32 = 1100.;
const HEADER_HEIGHT: f32 = 60.;
const PAGE_PADDING: f32 = 20.;
const DIAL_WIDTH: f32 = 340.;
const HISTORY_HEADER_HEIGHT: f32 = 52.;
const CALL_BUTTON_HEIGHT: f32 = 32.;
const FALLBACK_VIEWPORT: f32 = 900.;
const VIEWPORT_MARGIN: f32 = 300.;
const AVATAR_SIZE: f32 = 36.;
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

fn meeting_avatar() -> Div {
    div()
        .size(px(AVATAR_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::border_strong())
        .child(icon(IconName::Video, 16., theme::text()))
}

fn call_row(
    state: &Entity<AppState>,
    row: &CallRow,
    can_open_chat: bool,
    on_speed_dial: bool,
    directory: &crate::data::Directory,
) -> AnyElement {
    let avatar = if row.meeting {
        meeting_avatar().into_any_element()
    } else {
        person_avatar(directory, row.user_id.as_deref(), &row.title, AVATAR_SIZE)
    };
    let title_color = if row.missed {
        theme::red_soft()
    } else {
        theme::text()
    };
    let click_state = state.clone();
    let click_row = row.clone();
    let menu_state = state.clone();
    let menu_row = row.clone();
    let can_call_back = !row.meeting && row.user_id.is_some();
    let can_add_to_speed_dial = can_call_back && !on_speed_dial;
    h_flex()
        .id(SharedString::from(format!("call-{}", row.key)))
        .h(px(CALL_ROW_HEIGHT))
        .flex_none()
        .px(px(10.))
        .gap(px(12.))
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .child(avatar)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_baseline()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(14.))
                                .text_color(title_color)
                                .child(row.title.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::text_muted())
                                .child(row.time_label.clone()),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(if row.missed {
                            theme::red_soft()
                        } else {
                            theme::text_muted()
                        })
                        .child(row.subline.clone()),
                ),
        )
        .on_click(move |_, _, cx| {
            click_state.update(cx, |state, cx| state.activate_call_row(&click_row, cx));
        })
        .context_menu(move |popup, _, _| {
            let call_state = menu_state.clone();
            let call_row = menu_row.clone();
            let chat_state = menu_state.clone();
            let chat_row = menu_row.clone();
            let dial_state = menu_state.clone();
            let dial_row = menu_row.clone();
            popup
                .when(can_call_back, |popup| {
                    popup.item(
                        PopupMenuItem::new("Call back")
                            .icon(menu_icon(IconName::Phone))
                            .on_click(move |_, _, cx| {
                                call_state.update(cx, |state, cx| {
                                    state.call_back_from_history(&call_row, cx)
                                });
                            }),
                    )
                })
                .when(can_open_chat, |popup| {
                    popup.item(
                        PopupMenuItem::new("Open chat")
                            .icon(menu_icon(IconName::MessageSquare))
                            .on_click(move |_, _, cx| {
                                chat_state.update(cx, |state, cx| {
                                    state.open_chat_of_call_row(&chat_row, cx)
                                });
                            }),
                    )
                })
                .when(can_add_to_speed_dial, |popup| {
                    popup.item(
                        PopupMenuItem::new(tr("calls.page.speed_dial_add"))
                            .icon(menu_icon(IconName::Star))
                            .on_click(move |_, _, cx| {
                                let Some(user_id) = dial_row.user_id.as_deref() else {
                                    return;
                                };
                                dial_state.update(cx, |state, cx| {
                                    state.add_to_speed_dial(user_id, &dial_row.title, cx)
                                });
                            }),
                    )
                })
        })
        .into_any_element()
}

fn history_filters(state: &Entity<AppState>, current: HistoryFilter) -> AnyElement {
    let pills = HistoryFilter::ALL.into_iter().map(|filter| {
        let selected = filter == current;
        let state = state.clone();
        div()
            .id(SharedString::from(format!(
                "calls-filter-{}",
                filter.label()
            )))
            .h(px(24.))
            .px(px(10.))
            .flex()
            .flex_none()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(12.))
            .when(selected, |pill| {
                pill.bg(theme::accent()).text_color(theme::on_accent())
            })
            .when(!selected, |pill| {
                pill.bg(theme::surface_raised())
                    .text_color(theme::text_soft())
                    .hover(|pill| pill.bg(theme::border_strong()))
            })
            .child(filter.label())
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| state.set_call_history_filter(filter, cx))
            })
    });
    h_flex()
        .gap(px(6.))
        .items_center()
        .children(pills)
        .into_any_element()
}

fn empty_row(text: &'static str) -> AnyElement {
    div()
        .h(px(EMPTY_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.))
        .text_color(theme::text_muted())
        .child(text)
        .into_any_element()
}

fn speed_dial_row(
    state: &Entity<AppState>,
    entry: &SpeedDialEntry,
    directory: &crate::data::Directory,
) -> AnyElement {
    let click_state = state.clone();
    let user_id = entry.user_id.clone();
    let menu_state = state.clone();
    let menu_user_id = entry.user_id.clone();
    h_flex()
        .id(SharedString::from(format!("speed-dial-{}", entry.user_id)))
        .h(px(CALL_ROW_HEIGHT))
        .flex_none()
        .px(px(10.))
        .gap(px(12.))
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .child(person_avatar(
            directory,
            Some(&entry.user_id),
            &entry.name,
            AVATAR_SIZE,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(14.))
                .text_color(theme::text())
                .child(entry.name.clone()),
        )
        .on_click(move |_, _, cx| {
            click_state.update(cx, |state, cx| state.call_person(&user_id, false, cx));
        })
        .context_menu(move |popup, _, _| {
            let video_state = menu_state.clone();
            let video_user_id = menu_user_id.clone();
            let remove_state = menu_state.clone();
            let remove_user_id = menu_user_id.clone();
            popup
                .item(
                    PopupMenuItem::new("Video call")
                        .icon(menu_icon(IconName::Video))
                        .on_click(move |_, _, cx| {
                            video_state.update(cx, |state, cx| {
                                state.call_person(&video_user_id, true, cx)
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new("Remove from speed dial")
                        .icon(menu_icon(IconName::Delete))
                        .on_click(move |_, _, cx| {
                            remove_state.update(cx, |state, cx| {
                                state.remove_from_speed_dial(&remove_user_id, cx)
                            });
                        }),
                )
        })
        .into_any_element()
}

pub fn shows_speed_dial(window_width: f32) -> bool {
    window_width >= SPEED_DIAL_MIN_WINDOW_WIDTH
}

pub struct CallsPage {
    app: Entity<AppState>,
    people: Entity<NewChatDraft>,
    speed_dial_people: Entity<NewChatDraft>,
    adding_speed_dial: bool,
    scroll: ScrollHandle,
    _observations: [Subscription; 3],
}

impl CallsPage {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let people_app = app.clone();
        let people = cx.new(|cx| NewChatDraft::new(people_app, window, cx).compact());
        let speed_dial_app = app.clone();
        let speed_dial_people =
            cx.new(|cx| NewChatDraft::new(speed_dial_app, window, cx).compact());
        let observations = [
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&people, |_, _, cx| cx.notify()),
            cx.observe(&speed_dial_people, |_, _, cx| cx.notify()),
        ];
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        CallsPage {
            app,
            people,
            speed_dial_people,
            adding_speed_dial: false,
            scroll: ScrollHandle::new(),
            _observations: observations,
        }
    }

    fn call_picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = self.people.read(cx).chips().to_vec();
        let [person] = picked.as_slice() else {
            return;
        };
        let user_id = person.user_id.clone();
        self.app
            .update(cx, |state, cx| state.call_person(&user_id, false, cx));
        self.people
            .update(cx, |people, cx| people.reset(window, cx));
    }

    fn open_speed_dial_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.adding_speed_dial = true;
        self.speed_dial_people
            .update(cx, |people, cx| people.focus_query(window, cx));
        cx.notify();
    }

    fn close_speed_dial_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.adding_speed_dial = false;
        self.speed_dial_people
            .update(cx, |people, cx| people.reset(window, cx));
        cx.notify();
    }

    fn confirm_speed_dial_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = self.speed_dial_people.read(cx).chips().to_vec();
        self.app.update(cx, |state, cx| {
            for person in &picked {
                state.add_to_speed_dial(&person.user_id, &person.name, cx);
            }
        });
        self.close_speed_dial_add(window, cx);
    }

    fn speed_dial_adder(&self, cx: &mut Context<Self>) -> Div {
        let picked = !self.speed_dial_people.read(cx).chips().is_empty();
        v_flex()
            .gap(px(12.))
            .child(self.speed_dial_people.clone())
            .child(
                h_flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        Button::new("speed-dial-cancel")
                            .label(tr("common.cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_speed_dial_add(window, cx)
                            })),
                    )
                    .child(
                        Button::new("speed-dial-confirm")
                            .label(tr("common.add"))
                            .when(picked, |button| button.primary())
                            .disabled(!picked)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_speed_dial_add(window, cx)
                            })),
                    ),
            )
    }

    fn speed_dial_empty(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(tr("calls.page.speed_dial_empty")),
            )
            .child(
                Button::new("speed-dial-empty-add")
                    .label(tr("common.add"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_speed_dial_add(window, cx)),
                    ),
            )
    }

    fn speed_dial_column(&self, cx: &mut Context<Self>) -> Div {
        let state = self.app.read(cx);
        let rows = state
            .speed_dial
            .iter()
            .map(|entry| speed_dial_row(&self.app, entry, &state.directory))
            .collect::<Vec<_>>();
        let body = if self.adding_speed_dial {
            div().p(px(PAGE_PADDING)).child(self.speed_dial_adder(cx))
        } else if rows.is_empty() {
            self.speed_dial_empty(cx)
        } else {
            div().child(
                div()
                    .id("calls-speed-dial-scroll")
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .px(px(PAGE_PADDING - 10.))
                            .py(px(8.))
                            .children(rows),
                    ),
            )
        };
        v_flex()
            .debug_selector(|| CALLS_SPEED_DIAL_TAG.into())
            .w(px(SPEED_DIAL_WIDTH))
            .h_full()
            .flex_none()
            .child(
                h_flex()
                    .h(px(HISTORY_HEADER_HEIGHT))
                    .flex_none()
                    .pl(px(PAGE_PADDING))
                    .pr(px(PAGE_PADDING - 6.))
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(tr("calls.page.speed_dial")),
                    )
                    .child(
                        div()
                            .id("speed-dial-add")
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::row_hover()))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(tr("calls.page.speed_dial_add"))
                                    .build(window, cx)
                            })
                            .child(symbol("person_add", 18., theme::text_soft()))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_speed_dial_add(window, cx)
                            })),
                    ),
            )
            .child(body.flex_1().min_h_0())
    }

    fn dial_column(&self, cx: &mut Context<Self>) -> Div {
        let picked = self.people.read(cx).chips().len();
        let callable = picked == 1;
        v_flex()
            .debug_selector(|| CALLS_DIAL_TAG.into())
            .w(px(DIAL_WIDTH))
            .h_full()
            .flex_none()
            .p(px(PAGE_PADDING))
            .gap(px(12.))
            .child(self.people.clone())
            .child(
                Button::new("calls-dial-call")
                    .w_full()
                    .h(px(CALL_BUTTON_HEIGHT))
                    .icon(IconName::Phone)
                    .label(tr("common.call"))
                    .when(callable, |button| button.primary())
                    .disabled(!callable)
                    .when(picked > 1, |button| button.tooltip(tr("calls.page.pick_one")))
                    .on_click(cx.listener(|this, _, window, cx| this.call_picked(window, cx))),
            )
    }

    fn history_list(&self, window_height: f32, cx: &mut Context<Self>) -> Div {
        let now = Utc::now();
        let offset = Local::now().offset().fix();
        if self.app.read(cx).calls_list.needs_rebuild(now, offset) {
            let app = self.app.clone();
            cx.defer(move |cx| app.update(cx, |state, cx| state.rebuild_calls_list(cx)));
        }
        let top = -f32::from(scroll_offset(&self.scroll).y);
        let measured = f32::from(self.scroll.bounds().size.height);
        let height = if measured > 1. {
            measured
        } else {
            window_height.max(FALLBACK_VIEWPORT)
        };
        let state = self.app.read(cx);
        let visible = state
            .calls_list
            .window(top - VIEWPORT_MARGIN, top + height + VIEWPORT_MARGIN);
        let mut list = v_flex().w_full().px(px(PAGE_PADDING - 10.)).py(px(8.));
        if visible.before > 0. {
            list = list.child(div().h(px(visible.before)).flex_none());
        }
        for entry in &state.calls_list.entries()[visible.range.clone()] {
            list = list.child(match entry {
                ListEntry::Empty(text) => empty_row(text),
                ListEntry::Call(row) => call_row(
                    &self.app,
                    row,
                    state.can_open_chat_of_call_row(row),
                    row.user_id
                        .as_deref()
                        .is_some_and(|user_id| state.on_speed_dial(user_id)),
                    &state.directory,
                ),
            });
        }
        if visible.after > 0. {
            list = list.child(div().h(px(visible.after)).flex_none());
        }
        list
    }

    fn history_column(&self, window_height: f32, cx: &mut Context<Self>) -> Div {
        let filter = self.app.read(cx).calls_list.filter();
        v_flex()
            .debug_selector(|| CALLS_HISTORY_TAG.into())
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .h(px(HISTORY_HEADER_HEIGHT))
                    .flex_none()
                    .px(px(PAGE_PADDING))
                    .gap(px(16.))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(tr("calls.page.history")),
                    )
                    .child(history_filters(&self.app, filter)),
            )
            .child(with_top_edge(
                div()
                    .id("calls-history-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .track_scroll(&self.scroll)
                    .child(self.history_list(window_height, cx)),
                &self.scroll,
                CALLS_SCROLL_DIVIDER,
            ))
    }
}

impl Render for CallsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_height = f32::from(window.viewport_size().height);
        let window_width = f32::from(window.viewport_size().width);
        v_flex()
            .debug_selector(|| "shell/islands/main".into())
            .flex_1()
            .w_full()
            .min_w_0()
            .h_full()
            .rounded(px(10.))
            .bg(theme::surface())
            .overflow_hidden()
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .px(px(PAGE_PADDING))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text())
                            .child(tr("calls.page.title")),
                    ),
            )
            .child(div().w_full().h(px(1.)).flex_none().bg(theme::border()))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.dial_column(cx))
                    .child(div().w(px(1.)).h_full().flex_none().bg(theme::border()))
                    .child(self.history_column(window_height, cx))
                    .when(shows_speed_dial(window_width), |columns| {
                        columns
                            .child(div().w(px(1.)).h_full().flex_none().bg(theme::border()))
                            .child(self.speed_dial_column(cx))
                    }),
            )
    }
}
