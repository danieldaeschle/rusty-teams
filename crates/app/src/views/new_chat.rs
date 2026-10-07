use std::collections::HashSet;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    h_flex,
    input::{Backspace, Enter, Input, InputEvent, InputState, MoveDown, MoveUp},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::ChatRecord;
use teams_core::MentionCandidate;

use super::avatar::person_avatar;
use super::switcher::clamp_highlight;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::data::{self, Directory, Person};
use crate::runtime;
use crate::theme;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(200);
const SUGGESTED_LIMIT: usize = 6;
const RESULT_LIMIT: usize = 8;
const POPUP_WIDTH: f32 = 300.;
const POPUP_ROW_HEIGHT: f32 = 44.;
const CHIP_NAME_MAX_WIDTH: f32 = 180.;
const DEFAULT_PLACEHOLDER: &str = "Type a message";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub user_id: String,
    pub name: String,
    pub mail: Option<String>,
}

pub enum NewChatEvent {
    Changed,
    FocusComposer,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnterAction {
    Add,
    Advance,
    Nothing,
}

fn enter_action(query_is_empty: bool, chip_count: usize, has_highlighted: bool) -> EnterAction {
    match (query_is_empty, chip_count, has_highlighted) {
        (true, 1.., _) => EnterAction::Advance,
        (_, _, true) => EnterAction::Add,
        _ => EnterAction::Nothing,
    }
}

fn people_in(chats: &[ChatRecord], me: Option<&Person>, one_on_one_only: bool) -> Vec<Pick> {
    let mut ordered: Vec<&ChatRecord> = chats
        .iter()
        .filter(|chat| !one_on_one_only || data::is_one_on_one(chat))
        .collect();
    ordered.sort_by_key(|chat| std::cmp::Reverse(chat.last_message_at));
    let mut seen = HashSet::new();
    ordered
        .into_iter()
        .flat_map(|chat| data::others(chat, me))
        .filter_map(|(user_id, name)| {
            let user_id = user_id.filter(|user_id| seen.insert(user_id.clone()))?;
            Some(Pick {
                user_id,
                name,
                mail: None,
            })
        })
        .collect()
}

fn not_chosen(pick: &Pick, me: Option<&Person>, chosen: &[Pick]) -> bool {
    me.is_none_or(|me| me.user_id != pick.user_id)
        && chosen.iter().all(|chip| chip.user_id != pick.user_id)
}

pub fn suggested_people(
    chats: &[ChatRecord],
    me: Option<&Person>,
    chosen: &[Pick],
    limit: usize,
) -> Vec<Pick> {
    people_in(chats, me, true)
        .into_iter()
        .filter(|pick| not_chosen(pick, me, chosen))
        .take(limit)
        .collect()
}

fn matches_query(pick: &Pick, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    pick.name.to_lowercase().contains(&needle)
        || pick
            .mail
            .as_ref()
            .is_some_and(|mail| mail.to_lowercase().contains(&needle))
}

pub fn search_results(
    known: Vec<Pick>,
    remote: Vec<Pick>,
    query: &str,
    me: Option<&Person>,
    chosen: &[Pick],
    limit: usize,
) -> Vec<Pick> {
    let mut seen = HashSet::new();
    known
        .into_iter()
        .filter(|pick| matches_query(pick, query))
        .chain(remote)
        .filter(|pick| not_chosen(pick, me, chosen) && seen.insert(pick.user_id.clone()))
        .take(limit)
        .collect()
}

pub fn existing_one_on_one<'chats>(
    chats: &'chats [ChatRecord],
    me: Option<&Person>,
    user_id: &str,
) -> Option<&'chats ChatRecord> {
    chats.iter().find(|chat| {
        data::is_one_on_one(chat)
            && data::others(chat, me)
                .iter()
                .any(|(member_id, _)| member_id.as_deref() == Some(user_id))
    })
}

pub fn group_label(chips: &[Pick], group_name: &str) -> String {
    let name = group_name.trim();
    if name.is_empty() {
        chips
            .iter()
            .map(|chip| chip.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        name.to_owned()
    }
}

pub fn composer_placeholder(chips: &[Pick], group_name: &str) -> String {
    match chips {
        [] => DEFAULT_PLACEHOLDER.to_owned(),
        [chip] => format!("Message {}", chip.name),
        _ => format!("Message {}", group_label(chips, group_name)),
    }
}

fn pick_from_user(user: graph::User) -> Option<Pick> {
    let mail = user.mail.or(user.user_principal_name);
    let name = user.display_name.or_else(|| mail.clone())?;
    Some(Pick {
        user_id: user.id,
        name,
        mail,
    })
}

fn pick_from_candidate(candidate: MentionCandidate) -> Option<Pick> {
    match candidate {
        MentionCandidate::Person(person) => Some(Pick {
            user_id: person.user_id,
            name: person.display_name,
            mail: person.mail,
        }),
        _ => None,
    }
}

pub struct NewChatDraft {
    app: Entity<AppState>,
    query: Entity<InputState>,
    name: Entity<InputState>,
    chips: Vec<Pick>,
    remote: Vec<Pick>,
    highlighted: usize,
    popup_open: bool,
    searching: bool,
    search_failed: bool,
    generation: u64,
    search: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NewChatEvent> for NewChatDraft {}

impl NewChatDraft {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Name or email"));
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Group name (optional)"));
        let subscriptions = vec![
            cx.subscribe_in(&query, window, Self::on_query_event),
            cx.subscribe_in(&name, window, Self::on_name_event),
        ];
        NewChatDraft {
            app,
            query,
            name,
            chips: Vec::new(),
            remote: Vec::new(),
            highlighted: 0,
            popup_open: true,
            searching: false,
            search_failed: false,
            generation: 0,
            search: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn chips(&self) -> &[Pick] {
        &self.chips
    }

    pub fn group_name(&self, cx: &App) -> String {
        self.name.read(cx).value().trim().to_owned()
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chips.clear();
        self.query
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.name
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.on_query_changed(cx);
    }

    pub fn focus_query(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.query.focus_handle(cx).is_focused(window)
            || self.name.focus_handle(cx).is_focused(window)
    }

    pub fn escape(&mut self, cx: &mut Context<Self>) {
        if self.popup_visible(cx) {
            self.popup_open = false;
            cx.notify();
        } else {
            cx.emit(NewChatEvent::Close);
        }
    }

    fn query_text(&self, cx: &App) -> String {
        self.query.read(cx).value().trim().to_owned()
    }

    fn results(&self, cx: &App) -> Vec<Pick> {
        let state = self.app.read(cx);
        let me = state.directory.me.as_ref();
        let query = self.query_text(cx);
        if query.is_empty() {
            return suggested_people(&state.sidebar.chats, me, &self.chips, SUGGESTED_LIMIT);
        }
        search_results(
            people_in(&state.sidebar.chats, me, false),
            self.remote.clone(),
            &query,
            me,
            &self.chips,
            RESULT_LIMIT,
        )
    }

    fn popup_visible(&self, cx: &App) -> bool {
        self.popup_open && (!self.results(cx).is_empty() || !self.query_text(cx).is_empty())
    }

    fn on_query_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.on_query_changed(cx),
            InputEvent::Focus => {
                self.popup_open = true;
                cx.notify();
            }
            InputEvent::Blur => {
                self.popup_open = false;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {}
        }
    }

    fn on_name_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => cx.emit(NewChatEvent::Changed),
            InputEvent::PressEnter { .. } => cx.emit(NewChatEvent::FocusComposer),
            InputEvent::Focus | InputEvent::Blur => {}
        }
        cx.notify();
    }

    fn on_query_changed(&mut self, cx: &mut Context<Self>) {
        self.popup_open = true;
        self.highlighted = 0;
        self.generation += 1;
        self.search = None;
        self.searching = false;
        self.search_failed = false;
        self.remote.clear();
        let query = self.query_text(cx);
        let (demo, engine) = {
            let state = self.app.read(cx);
            (state.mode.demo, state.engine.clone())
        };
        if !query.is_empty() {
            if demo {
                self.remote = crate::demo::mention_candidates("", &query)
                    .into_iter()
                    .filter_map(pick_from_candidate)
                    .collect();
            } else if let Some(engine) = engine {
                self.searching = true;
                let generation = self.generation;
                self.search = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(SEARCH_DEBOUNCE).await;
                    let receiver =
                        runtime::spawn(async move { engine.search_people(&query).await });
                    let result = receiver.await;
                    this.update(cx, |this, cx| this.apply_search(generation, result, cx))
                        .ok();
                }));
            }
        }
        cx.notify();
    }

    fn apply_search(
        &mut self,
        generation: u64,
        result: Result<
            teams_core::Result<Vec<graph::User>>,
            tokio::sync::oneshot::error::RecvError,
        >,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        self.searching = false;
        match result {
            Ok(Ok(users)) => self.remote = users.into_iter().filter_map(pick_from_user).collect(),
            _ => self.search_failed = true,
        }
        self.highlighted = 0;
        cx.notify();
    }

    fn add(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if self.chips.iter().all(|chip| chip.user_id != pick.user_id) {
            self.chips.push(pick);
        }
        self.query
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.on_query_changed(cx);
        cx.emit(NewChatEvent::Changed);
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.chips.len() {
            self.chips.remove(index);
            self.highlighted = 0;
            cx.emit(NewChatEvent::Changed);
            cx.notify();
        }
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let length = self.results(cx).len();
        self.highlighted = clamp_highlight(self.highlighted, delta, length);
        cx.notify();
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let highlighted = self
            .popup_open
            .then(|| self.results(cx).into_iter().nth(self.highlighted))
            .flatten();
        let action = enter_action(
            self.query_text(cx).is_empty(),
            self.chips.len(),
            highlighted.is_some(),
        );
        match (action, highlighted) {
            (EnterAction::Add, Some(pick)) => self.add(pick, window, cx),
            (EnterAction::Advance, _) if self.chips.len() >= 2 => {
                self.popup_open = false;
                self.name.update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
            }
            (EnterAction::Advance, _) => {
                self.popup_open = false;
                cx.emit(NewChatEvent::FocusComposer);
                cx.notify();
            }
            _ => {}
        }
    }

    fn backspace(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.query_text(cx).is_empty() || self.chips.is_empty() {
            return false;
        }
        self.remove(self.chips.len() - 1, cx);
        true
    }

    fn render_chip(&self, index: usize, chip: &Pick, cx: &mut Context<Self>) -> impl IntoElement {
        let directory = &self.app.read(cx).directory;
        h_flex()
            .id(ElementId::Name(format!("new-chat-chip-{index}").into()))
            .h(px(26.))
            .pl(px(3.))
            .pr(px(6.))
            .gap(px(6.))
            .items_center()
            .rounded(px(13.))
            .bg(theme::surface_raised())
            .border_1()
            .border_color(theme::border_strong())
            .child(person_avatar(
                directory,
                Some(&chip.user_id),
                &chip.name,
                20.,
            ))
            .child(
                div()
                    .max_w(px(CHIP_NAME_MAX_WIDTH))
                    .truncate()
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .child(chip.name.clone()),
            )
            .child(
                div()
                    .id(ElementId::Name(
                        format!("new-chat-chip-remove-{index}").into(),
                    ))
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::row_hover()))
                    .child(icon(IconName::Close, 12., theme::text_muted()))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove(index, cx))),
            )
    }

    fn render_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.popup_visible(cx) {
            return None;
        }
        let results = self.results(cx);
        let query = self.query_text(cx);
        let directory = &self.app.read(cx).directory;
        let label = if query.is_empty() {
            "Suggested"
        } else {
            "People"
        };
        let rows = results.into_iter().enumerate().map(|(index, pick)| {
            let detail = pick.mail.clone().unwrap_or_default();
            let chosen = pick.clone();
            h_flex()
                .id(ElementId::Name(format!("new-chat-result-{index}").into()))
                .px(px(6.))
                .h(px(POPUP_ROW_HEIGHT))
                .gap(px(10.))
                .items_center()
                .rounded(px(6.))
                .cursor_pointer()
                .when(index == self.highlighted, |row| {
                    row.bg(theme::border_strong())
                })
                .hover(|row| row.bg(theme::row_hover()))
                .child(person_avatar(
                    directory,
                    Some(&pick.user_id),
                    &pick.name,
                    28.,
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .text_color(theme::text())
                                .child(pick.name.clone()),
                        )
                        .when(!detail.is_empty(), |column| {
                            column.child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(theme::text_muted())
                                    .child(detail),
                            )
                        }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.add(chosen.clone(), window, cx);
                }))
        });
        let note = |text: String, color: Hsla| {
            div()
                .px(px(6.))
                .py(px(8.))
                .text_size(px(12.))
                .text_color(color)
                .child(text)
        };
        let results_empty = self.results(cx).is_empty();
        Some(
            deferred(
                v_flex()
                    .id("new-chat-popup")
                    .absolute()
                    .top(relative(1.))
                    .left(px(20.))
                    .mt(px(4.))
                    .w(px(POPUP_WIDTH))
                    .p(px(6.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme::border_strong())
                    .bg(theme::surface_raised())
                    .shadow_lg()
                    .occlude()
                    .when(!results_empty, |popup| {
                        popup.child(
                            div()
                                .px(px(6.))
                                .pb(px(4.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text_muted())
                                .child(label),
                        )
                    })
                    .children(rows)
                    .when(self.search_failed, |popup| {
                        popup.child(note("Search failed".to_owned(), theme::red_soft()))
                    })
                    .when(self.searching && results_empty, |popup| {
                        popup.child(note("Searching ...".to_owned(), theme::text_muted()))
                    })
                    .when(
                        !self.searching && !self.search_failed && results_empty,
                        |popup| {
                            popup.child(note(
                                format!("No people found for \"{query}\""),
                                theme::text_muted(),
                            ))
                        },
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn render_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chips: Vec<AnyElement> = self
            .chips
            .iter()
            .enumerate()
            .map(|(index, chip)| self.render_chip(index, chip, cx).into_any_element())
            .collect();
        h_flex()
            .relative()
            .w_full()
            .min_h(px(60.))
            .flex_none()
            .px(px(20.))
            .py(px(8.))
            .gap(px(8.))
            .flex_wrap()
            .items_center()
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(theme::text_muted())
                    .child("To:"),
            )
            .children(chips)
            .child(
                div()
                    .flex_1()
                    .min_w(px(160.))
                    .child(Input::new(&self.query).appearance(false).bordered(false)),
            )
            .children(self.render_popup(cx))
    }

    fn render_name_row(&self, window: &Window, cx: &App) -> Option<impl IntoElement> {
        if self.chips.len() < 2 {
            return None;
        }
        let focused = self.name.focus_handle(cx).is_focused(window);
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .px(px(20.))
                .py(px(8.))
                .gap(px(10.))
                .items_center()
                .border_b_1()
                .border_color(theme::border())
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(theme::text_muted())
                        .child("Name"),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .bg(theme::surface())
                        .border_1()
                        .border_color(if focused {
                            theme::accent()
                        } else {
                            theme::border_strong()
                        })
                        .child(
                            div()
                                .w_full()
                                .child(Input::new(&self.name).appearance(false).bordered(false)),
                        ),
                ),
        )
    }

    pub fn render_empty(&self, directory: &Directory, group_name: &str) -> AnyElement {
        let column = v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(8.));
        match self.chips.as_slice() {
            [] => column.into_any_element(),
            [chip] => column
                .child(person_avatar(
                    directory,
                    Some(&chip.user_id),
                    &chip.name,
                    56.,
                ))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(chip.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child("No messages yet. Say hi."),
                )
                .into_any_element(),
            chips => {
                let title = group_name.trim();
                column
                    .when(!title.is_empty(), |column| {
                        column.child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text())
                                .child(title.to_owned()),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::text_muted())
                            .child(format!(
                                "New group chat with you and {} people",
                                chips.len()
                            )),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Render for NewChatDraft {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faces: Vec<String> = self
            .chips
            .iter()
            .map(|chip| chip.user_id.clone())
            .chain(self.results(cx).into_iter().map(|pick| pick.user_id))
            .collect();
        let app = self.app.clone();
        cx.defer(move |cx| {
            app.update(cx, |state, cx| state.request_avatars(faces, cx));
        });
        v_flex()
            .w_full()
            .flex_none()
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.query.focus_handle(cx).is_focused(window) && this.popup_visible(cx) {
                    this.move_highlight(-1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.query.focus_handle(cx).is_focused(window) && this.popup_visible(cx) {
                    this.move_highlight(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                if this.query.focus_handle(cx).is_focused(window) {
                    this.accept(window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Backspace, window, cx| {
                if this.query.focus_handle(cx).is_focused(window) && this.backspace(cx) {
                    cx.stop_propagation();
                }
            }))
            .child(self.render_bar(cx))
            .children(self.render_name_row(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use store::{ChatRecord, MemberRecord};

    use super::{
        EnterAction, Pick, composer_placeholder, enter_action, existing_one_on_one, group_label,
        search_results, suggested_people,
    };
    use crate::data::Person;

    fn me() -> Person {
        Person {
            user_id: "me".into(),
            display_name: "Me".into(),
        }
    }

    fn pick(user_id: &str, name: &str, mail: Option<&str>) -> Pick {
        Pick {
            user_id: user_id.into(),
            name: name.into(),
            mail: mail.map(str::to_owned),
        }
    }

    fn chat(id: &str, kind: &str, members: &[(&str, &str)], day: u32) -> ChatRecord {
        ChatRecord {
            id: id.into(),
            kind: kind.into(),
            members: members
                .iter()
                .map(|(user_id, name)| MemberRecord {
                    user_id: Some((*user_id).into()),
                    display_name: (*name).into(),
                })
                .collect(),
            last_message_at: Some(Utc.with_ymd_and_hms(2026, 1, day, 9, 0, 0).unwrap()),
            ..Default::default()
        }
    }

    fn chats() -> Vec<ChatRecord> {
        vec![
            chat("old", "oneOnOne", &[("me", "Me"), ("ann", "Ann")], 1),
            chat("new", "oneOnOne", &[("me", "Me"), ("bob", "Bob")], 5),
            chat(
                "group",
                "group",
                &[("me", "Me"), ("cid", "Cid"), ("ann", "Ann")],
                9,
            ),
        ]
    }

    #[test]
    fn suggestions_are_recent_one_on_one_partners() {
        let found = suggested_people(&chats(), Some(&me()), &[], 6);
        let ids: Vec<&str> = found.iter().map(|pick| pick.user_id.as_str()).collect();
        assert_eq!(ids, ["bob", "ann"]);
    }

    #[test]
    fn suggestions_skip_chosen_people_and_respect_the_limit() {
        let chosen = [pick("bob", "Bob", None)];
        let found = suggested_people(&chats(), Some(&me()), &chosen, 6);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].user_id, "ann");
        assert_eq!(suggested_people(&chats(), Some(&me()), &[], 1).len(), 1);
    }

    #[test]
    fn search_puts_local_matches_first_and_dedupes() {
        let known = vec![pick("ann", "Ann Local", None), pick("cid", "Cid", None)];
        let remote = vec![
            pick("ann", "Ann Remote", Some("ann@example.com")),
            pick("anna", "Anna", Some("anna@example.com")),
            pick("me", "Me Anna", None),
        ];
        let found = search_results(known, remote, "ann", Some(&me()), &[], 8);
        let ids: Vec<&str> = found.iter().map(|pick| pick.user_id.as_str()).collect();
        assert_eq!(ids, ["ann", "anna"]);
        assert_eq!(found[0].name, "Ann Local");
    }

    #[test]
    fn search_excludes_chosen_people_and_caps_the_list() {
        let remote: Vec<Pick> = (0..12)
            .map(|index| pick(&format!("u{index}"), &format!("User {index}"), None))
            .collect();
        let chosen = [pick("u0", "User 0", None)];
        let found = search_results(Vec::new(), remote, "user", None, &chosen, 8);
        assert_eq!(found.len(), 8);
        assert!(found.iter().all(|pick| pick.user_id != "u0"));
    }

    #[test]
    fn local_search_matches_the_mail_address() {
        let known = vec![pick("ann", "Ann", Some("a.nowak@example.com"))];
        let found = search_results(known, Vec::new(), "nowak", None, &[], 8);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn existing_chat_is_found_only_for_one_on_one() {
        let chats = chats();
        let found = existing_one_on_one(&chats, Some(&me()), "ann").map(|chat| chat.id.as_str());
        assert_eq!(found, Some("old"));
        assert!(existing_one_on_one(&chats, Some(&me()), "cid").is_none());
    }

    #[test]
    fn placeholder_follows_the_recipients() {
        let one = [pick("ann", "Ann", None)];
        let two = [pick("ann", "Ann", None), pick("bob", "Bob", None)];
        assert_eq!(composer_placeholder(&[], ""), "Type a message");
        assert_eq!(composer_placeholder(&one, "ignored"), "Message Ann");
        assert_eq!(composer_placeholder(&two, " "), "Message Ann, Bob");
        assert_eq!(composer_placeholder(&two, " Crew "), "Message Crew");
        assert_eq!(group_label(&two, ""), "Ann, Bob");
    }

    #[test]
    fn enter_adds_the_highlight_or_moves_on() {
        assert_eq!(enter_action(false, 0, true), EnterAction::Add);
        assert_eq!(enter_action(true, 0, true), EnterAction::Add);
        assert_eq!(enter_action(true, 1, true), EnterAction::Advance);
        assert_eq!(enter_action(false, 2, false), EnterAction::Nothing);
        assert_eq!(enter_action(true, 0, false), EnterAction::Nothing);
    }

    mod keys {
        use std::sync::Arc;

        use gpui_kit::test::TestWindowExt as _;
        use gpui_kit::{AppContext as _, Entity, TestAppContext, WindowOptions};
        use store::Store;

        use super::super::NewChatDraft;
        use crate::app_state::{AppState, Mode};

        fn open(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, Entity<NewChatDraft>) {
            cx.update(gpui_kit::init);
            let store = Arc::new(Store::open_in_memory().unwrap());
            crate::demo::seed(&store);
            let mode = Mode {
                demo: true,
                read_only: false,
                demo_sync: None,
            };
            cx.update(|cx| {
                let app = cx.new(|_| {
                    let mut state = AppState::new(store, mode);
                    crate::demo::seed_directory(&mut state);
                    state
                });
                gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                    cx.new(|cx| NewChatDraft::new(app, window, cx))
                })
                .unwrap()
            })
        }

        #[gpui_kit::test]
        fn enter_adds_a_chip_and_backspace_removes_it(cx: &mut TestAppContext) {
            let (handle, draft) = open(cx);
            let names = cx
                .update_window(handle, |_, window, cx| {
                    draft.update(cx, |draft, cx| draft.focus_query(window, cx));
                    window.render_frame(cx);
                    window.input("mara", cx);
                    window.render_frame(cx);
                    window.press("enter", cx);
                    window.render_frame(cx);
                    let added: Vec<String> = draft
                        .read(cx)
                        .chips()
                        .iter()
                        .map(|chip| chip.name.clone())
                        .collect();
                    window.press("backspace", cx);
                    window.render_frame(cx);
                    (added, draft.read(cx).chips().len())
                })
                .unwrap();
            assert_eq!(names.0, ["Mara Lindqvist"]);
            assert_eq!(names.1, 0);
        }
    }
}
