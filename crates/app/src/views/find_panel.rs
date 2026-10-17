use std::time::Duration;

use chrono::{Local, NaiveDate, Offset};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::Store;
use teams_core::substrate_search::{QueryRequest, QueryResults, QueryScope, SearchSession};

use super::widgets::icon;
use crate::app_state::{AppState, Selection};
use crate::find_in_chat::{
    DATE_LABEL, DateRange, EMPTY_HINT, FIND_IN_CHANNEL, FIND_IN_CHAT, FROM_LABEL, FindFilters,
    FindHit, HAS_ATTACHMENT_LABEL, LOAD_ERROR, LOADING, MENTIONS_LABEL, NO_RESULTS_CHANNEL,
    NO_RESULTS_CHAT, Sender, from_local, from_record, from_server, group_by_month, merge,
    no_results_with_filters, same_user, step_index,
};
use crate::format;
use crate::runtime;
use crate::theme;

pub const FIND_PANEL_TAG: &str = "shell/islands/main/find";
pub const FIND_DIVIDER_TAG: &str = "shell/islands/main/find/divider-side";
pub const FIND_INPUT_TAG: &str = "shell/islands/main/find/input";
pub const FIND_RESULTS_TAG: &str = "shell/islands/main/find/results";
pub const FIND_PANEL_WIDTH: f32 = 320.;
const HEADER_HEIGHT: f32 = 48.;
const SIDE_INSET: f32 = 16.;
const INPUT_HEIGHT: f32 = 32.;
const CHIP_HEIGHT: f32 = 24.;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(200);
const LOCAL_LIMIT: usize = 100;
const FILTER_SCAN_LIMIT: usize = 500;
const SENDER_SCAN_LIMIT: usize = 200;

pub enum FindPanelEvent {
    Close,
    Jump(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Idle,
    Loading,
    Ready,
    Failed,
}

pub struct FindPanel {
    app: Entity<AppState>,
    selection: Selection,
    input: Entity<InputState>,
    query: String,
    filters: FindFilters,
    senders: Vec<Sender>,
    local_hits: Vec<FindHit>,
    hits: Vec<FindHit>,
    status: Status,
    generation: u64,
    current_index: Option<usize>,
    search_session: SearchSession,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl EventEmitter<FindPanelEvent> for FindPanel {}

impl FindPanel {
    pub fn new(
        app: Entity<AppState>,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = panel_title(&selection);
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(title));
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        let senders = senders_of(app.read(cx), selection.conversation_id());
        FindPanel {
            app,
            selection,
            input,
            query: String::new(),
            filters: FindFilters::default(),
            senders,
            local_hits: Vec::new(),
            hits: Vec::new(),
            status: Status::Idle,
            generation: 0,
            current_index: None,
            search_session: SearchSession::new(),
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.query = input.read(cx).value().trim().to_owned();
                self.schedule_search(cx);
            }
            InputEvent::PressEnter { shift, .. } => self.step(*shift, cx),
            _ => {}
        }
    }

    fn step(&mut self, backwards: bool, cx: &mut Context<Self>) {
        if let Some(index) = step_index(self.current_index, self.hits.len(), backwards) {
            self.pick(index, cx);
        }
    }

    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(hit) = self.hits.get(index) else {
            return;
        };
        self.current_index = Some(index);
        self.scroll.scroll_to_item(index);
        cx.emit(FindPanelEvent::Jump(hit.message_id.clone()));
        cx.notify();
    }

    fn update_filters(&mut self, change: impl FnOnce(&mut FindFilters), cx: &mut Context<Self>) {
        change(&mut self.filters);
        self.schedule_search(cx);
        cx.notify();
    }

    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            this.update(cx, |panel, cx| {
                if panel.generation == generation {
                    panel.run_search(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        self.current_index = None;
        if self.query.is_empty() && !self.filters.is_active() {
            self.local_hits.clear();
            self.hits.clear();
            self.status = Status::Idle;
            cx.notify();
            return;
        }
        let (store, engine, my_id) = {
            let state = self.app.read(cx);
            (
                state.store.clone(),
                state.engine.clone(),
                state.directory.me.as_ref().map(|me| me.user_id.clone()),
            )
        };
        let (today, offset) = today_and_offset();
        let conversation_id = self.selection.conversation_id().to_owned();
        let local: Vec<FindHit> =
            local_hits(&store, &conversation_id, &self.query, my_id.as_deref())
                .into_iter()
                .filter(|hit| self.filters.admits(hit, today, offset))
                .collect();
        self.local_hits = local;
        self.hits = merge(self.local_hits.clone(), Vec::new());
        let server_engine = engine.filter(|_| matches!(self.selection, Selection::Chat(_)));
        let Some(engine) = server_engine else {
            self.status = Status::Ready;
            cx.notify();
            return;
        };
        self.status = Status::Loading;
        let request = QueryRequest::new(self.query.clone(), QueryScope::Messages)
            .with_filters(self.filters.server_filters(&conversation_id));
        let search_session = self.search_session.clone();
        let generation = self.generation;
        let receiver =
            runtime::spawn(async move { engine.server_query(&search_session, &request).await });
        cx.spawn(async move |this, cx| {
            let results = match receiver.await {
                Ok(Ok(results)) => Some(results),
                _ => None,
            };
            this.update(cx, |panel, cx| panel.apply_server(generation, results, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn apply_server(
        &mut self,
        generation: u64,
        results: Option<QueryResults>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        let Some(results) = results else {
            self.status = if self.hits.is_empty() {
                Status::Failed
            } else {
                Status::Ready
            };
            cx.notify();
            return;
        };
        let (today, offset) = today_and_offset();
        let conversation_id = self.selection.conversation_id();
        let server: Vec<FindHit> = results
            .messages
            .items
            .iter()
            .filter(|message| message.conversation_id == conversation_id)
            .filter_map(from_server)
            .filter(|hit| self.filters.admits(hit, today, offset))
            .collect();
        self.hits = merge(self.local_hits.clone(), server);
        self.status = Status::Ready;
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .pl(px(SIDE_INSET))
            .pr(px(12.))
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(panel_title(&self.selection)),
            )
            .child(
                div()
                    .id("find-close")
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::row_hover()))
                    .tooltip(|window, cx| Tooltip::new("Close").build(window, cx))
                    .child(icon(IconName::Close, 14., theme::text_muted()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindPanelEvent::Close))),
            )
    }

    fn search_row(&self) -> impl IntoElement {
        h_flex()
            .debug_selector(|| FIND_INPUT_TAG.into())
            .mx(px(SIDE_INSET))
            .h(px(INPUT_HEIGHT))
            .flex_none()
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::surface_raised())
            .child(icon(IconName::Search, 14., theme::text_muted()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.input).appearance(false).bordered(false)),
            )
    }

    fn chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.weak_entity();
        h_flex()
            .px(px(SIDE_INSET))
            .pt(px(8.))
            .flex_none()
            .flex_wrap()
            .gap(px(6.))
            .child(self.from_chip(panel.clone()))
            .child(self.date_chip(panel))
            .child(self.toggle_chip(
                "find-has-attachment",
                HAS_ATTACHMENT_LABEL,
                self.filters.has_attachment,
                |filters| filters.has_attachment = !filters.has_attachment,
                cx,
            ))
            .child(self.toggle_chip(
                "find-mentions",
                MENTIONS_LABEL,
                self.filters.mentions_me,
                |filters| filters.mentions_me = !filters.mentions_me,
                cx,
            ))
    }

    fn from_chip(&self, panel: WeakEntity<FindPanel>) -> impl IntoElement {
        let selected = self.filters.from.clone();
        let label = selected
            .as_ref()
            .map_or(FROM_LABEL.to_owned(), |sender| sender.name.clone());
        let senders = self.senders.clone();
        chip_button("find-from", label, selected.is_some()).dropdown_menu(move |menu, _, _| {
            senders.iter().fold(menu, |menu, sender| {
                let checked = selected
                    .as_ref()
                    .is_some_and(|chosen| same_user(&chosen.user_id, &sender.user_id));
                let (panel, sender) = (panel.clone(), sender.clone());
                menu.item(
                    PopupMenuItem::new(sender.name.clone())
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            let pick = (!checked).then(|| sender.clone());
                            panel
                                .update(cx, |panel, cx| {
                                    panel.update_filters(|filters| filters.from = pick, cx)
                                })
                                .ok();
                        }),
                )
            })
        })
    }

    fn date_chip(&self, panel: WeakEntity<FindPanel>) -> impl IntoElement {
        let selected = self.filters.date;
        let label = selected.map_or(DATE_LABEL, DateRange::label);
        chip_button("find-date", label.to_owned(), selected.is_some()).dropdown_menu(
            move |menu, _, _| {
                DateRange::ALL.into_iter().fold(menu, |menu, range| {
                    let checked = selected == Some(range);
                    let panel = panel.clone();
                    menu.item(PopupMenuItem::new(range.label()).checked(checked).on_click(
                        move |_, _, cx| {
                            let pick = (!checked).then_some(range);
                            panel
                                .update(cx, |panel, cx| {
                                    panel.update_filters(|filters| filters.date = pick, cx)
                                })
                                .ok();
                        },
                    ))
                })
            },
        )
    }

    fn toggle_chip(
        &self,
        id: &'static str,
        label: &'static str,
        selected: bool,
        change: fn(&mut FindFilters),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(CHIP_HEIGHT))
            .px(px(12.))
            .flex()
            .flex_none()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(12.))
            .when(selected, |chip| {
                chip.bg(theme::accent()).text_color(theme::on_accent())
            })
            .when(!selected, |chip| {
                chip.bg(theme::surface_raised())
                    .text_color(theme::text_soft())
                    .hover(|chip| chip.bg(theme::border_strong()))
            })
            .child(label)
            .on_click(cx.listener(move |panel, _, _, cx| panel.update_filters(change, cx)))
    }

    fn message_line(&self) -> Option<String> {
        let no_results = || {
            if self.filters.is_active() && !self.query.is_empty() {
                no_results_with_filters(&self.query)
            } else {
                match self.selection {
                    Selection::Chat(_) => NO_RESULTS_CHAT.to_owned(),
                    Selection::Channel(_) => NO_RESULTS_CHANNEL.to_owned(),
                }
            }
        };
        match self.status {
            Status::Idle => Some(EMPTY_HINT.to_owned()),
            Status::Failed => Some(LOAD_ERROR.to_owned()),
            Status::Loading if self.hits.is_empty() => Some(LOADING.to_owned()),
            Status::Ready if self.hits.is_empty() => Some(no_results()),
            _ => None,
        }
    }

    fn results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (today, offset) = today_and_offset();
        let mut list = v_flex().w_full().pt(px(8.)).pb(px(SIDE_INSET));
        if let Some(message) = self.message_line() {
            list = list.child(
                div()
                    .px(px(SIDE_INSET))
                    .py(px(8.))
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(message),
            );
        }
        let mut index = 0;
        for group in group_by_month(&self.hits, today, offset) {
            list = list.child(
                div()
                    .px(px(SIDE_INSET))
                    .pt(px(12.))
                    .pb(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted())
                    .child(group.label),
            );
            for hit in group.hits {
                list = list.child(self.hit_row(&hit, index, today, offset, cx));
                index += 1;
            }
        }
        div()
            .debug_selector(|| FIND_RESULTS_TAG.into())
            .id("find-results")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(list)
    }

    fn hit_row(
        &self,
        hit: &FindHit,
        index: usize,
        today: NaiveDate,
        offset: chrono::FixedOffset,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let current = self.current_index == Some(index);
        let marks: Vec<(std::ops::Range<usize>, HighlightStyle)> = hit
            .highlights
            .iter()
            .filter(|range| hit.text.is_char_boundary(range.start) && range.end <= hit.text.len())
            .map(|range| {
                (
                    range.clone(),
                    HighlightStyle {
                        color: Some(theme::accent_text()),
                        font_weight: Some(FontWeight::SEMIBOLD),
                        ..Default::default()
                    },
                )
            })
            .collect();
        v_flex()
            .id(ElementId::NamedInteger("find-hit".into(), index as u64))
            .mx(px(8.))
            .px(px(8.))
            .py(px(8.))
            .gap(px(2.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(current, |row| row.bg(theme::selection()))
            .when(!current, |row| row.hover(|row| row.bg(theme::row_hover())))
            .child(
                h_flex()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(hit.sender_name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme::text_muted())
                            .child(format::list_time_label(hit.sent_at, today, offset)),
                    ),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_soft())
                    .line_clamp(2)
                    .child(StyledText::new(hit.text.clone()).with_highlights(marks)),
            )
            .on_click(cx.listener(move |panel, _, _, cx| panel.pick(index, cx)))
    }
}

impl Render for FindPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .debug_selector(|| FIND_PANEL_TAG.into())
            .h_full()
            .items_start()
            .bg(theme::surface())
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.emit(FindPanelEvent::Close);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .debug_selector(|| FIND_DIVIDER_TAG.into())
                    .w(px(1.))
                    .h_full()
                    .flex_none()
                    .bg(theme::border()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.header(cx))
                    .child(self.search_row())
                    .child(self.chips(cx))
                    .child(self.results(cx)),
            )
    }
}

fn chip_button(id: &'static str, label: String, selected: bool) -> Button {
    Button::new(id)
        .ghost()
        .compact()
        .label(label)
        .dropdown_caret(true)
        .rounded(px(CHIP_HEIGHT / 2.))
        .h(px(CHIP_HEIGHT))
        .text_size(px(12.))
        .when(selected, |chip| {
            chip.bg(theme::accent()).text_color(theme::on_accent())
        })
        .when(!selected, |chip| {
            chip.bg(theme::surface_raised())
                .text_color(theme::text_soft())
        })
}

fn panel_title(selection: &Selection) -> &'static str {
    match selection {
        Selection::Chat(_) => FIND_IN_CHAT,
        Selection::Channel(_) => FIND_IN_CHANNEL,
    }
}

fn today_and_offset() -> (NaiveDate, chrono::FixedOffset) {
    let now = Local::now();
    (now.date_naive(), now.offset().fix())
}

fn local_hits(
    store: &Store,
    conversation_id: &str,
    query: &str,
    my_id: Option<&str>,
) -> Vec<FindHit> {
    if query.is_empty() {
        return store
            .messages(conversation_id, None, FILTER_SCAN_LIMIT)
            .unwrap_or_default()
            .iter()
            .filter(|record| !record.deleted)
            .map(|record| from_record(record, my_id))
            .collect();
    }
    let found = store
        .search_messages(query, Some(conversation_id), LOCAL_LIMIT)
        .unwrap_or_default();
    let message_ids: Vec<String> = found.iter().map(|hit| hit.message_id.clone()).collect();
    let records = store
        .messages_by_id(conversation_id, &message_ids)
        .unwrap_or_default();
    found
        .iter()
        .map(|hit| from_local(hit, records.get(&hit.message_id), my_id))
        .collect()
}

fn senders_of(state: &AppState, conversation_id: &str) -> Vec<Sender> {
    let my_id = state.directory.me.as_ref().map(|me| me.user_id.as_str());
    let is_me = |user_id: &str| my_id.is_some_and(|my_id| same_user(my_id, user_id));
    let mut senders: Vec<Sender> = Vec::new();
    let mut push = |user_id: &str, name: &str| {
        if name.is_empty() || is_me(user_id) {
            return;
        }
        if senders
            .iter()
            .any(|sender| same_user(&sender.user_id, user_id))
        {
            return;
        }
        senders.push(Sender {
            user_id: user_id.to_owned(),
            name: name.to_owned(),
        });
    };
    if let Some(chat) = state
        .sidebar
        .chats
        .iter()
        .find(|chat| chat.id == conversation_id)
    {
        for member in &chat.members {
            if let Some(user_id) = &member.user_id {
                push(user_id, &member.display_name);
            }
        }
    }
    for record in state
        .store
        .messages(conversation_id, None, SENDER_SCAN_LIMIT)
        .unwrap_or_default()
        .iter()
        .rev()
    {
        if let (Some(user_id), Some(name)) = (&record.sender_id, &record.sender_name) {
            push(user_id, name);
        }
    }
    senders.sort_by_key(|sender| sender.name.to_lowercase());
    senders
}
