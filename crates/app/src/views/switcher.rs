use gpui_kit::assets::IconName;
use gpui_kit::component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::Duration;

use chrono::{Local, Offset};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::SearchHit;
use teams_core::substrate_search::{SearchSession, Suggestions};
use teams_core::{HIGHLIGHT_END, HIGHLIGHT_START};

use super::avatar::{person_avatar, spec_avatar, team_avatar};
use super::switcher_server::{
    PersonResult, merge_messages, people_results, server_message_results,
};
use super::widgets::icon;
use crate::app_state::{AppState, Selection, chat_title};
use crate::data::{Directory, is_one_on_one};
use crate::format;
use crate::fuzzy;
use crate::runtime;
use crate::sidebar_model::{AvatarSpec, avatar_for};
use crate::theme::{self, ElevatedSurface as _};

const MAX_RESULTS: usize = 9;
const MAX_RESULTS_WITH_MESSAGES: usize = 5;
const MAX_MESSAGE_RESULTS: usize = 5;
const MIN_MESSAGE_QUERY_CHARS: usize = 2;
const MAX_PEOPLE_RESULTS: usize = 4;
const MIN_SERVER_QUERY_CHARS: usize = 2;
const SERVER_DEBOUNCE: Duration = Duration::from_millis(200);
const MESSAGE_GROUP: &str = "Messages";
const PEOPLE_GROUP: &str = "People";
const CARD_WIDTH: f32 = 520.;
const CARD_TOP: f32 = 6.;

pub enum SwitcherEvent {
    Pick(Selection),
    PickMessage {
        selection: Selection,
        message_id: String,
    },
    PickPerson {
        user_id: String,
    },
    Close,
}

#[derive(Clone)]
pub struct MessageResult {
    pub selection: Selection,
    pub message_id: String,
    pub author: String,
    pub conversation: String,
    pub date: String,
    pub snippet: String,
    pub matches: Vec<Range<usize>>,
}

pub fn parse_snippet(snippet: &str) -> (String, Vec<Range<usize>>) {
    let mut text = String::with_capacity(snippet.len());
    let mut matches = Vec::new();
    let mut start: Option<usize> = None;
    for character in snippet.chars() {
        match character {
            HIGHLIGHT_START => start = Some(text.len()),
            HIGHLIGHT_END => {
                if let Some(begin) = start.take().filter(|begin| *begin < text.len()) {
                    matches.push(begin..text.len());
                }
            }
            '\n' | '\r' => text.push(' '),
            other => text.push(other),
        }
    }
    (text, matches)
}

pub fn message_results(
    hits: Vec<SearchHit>,
    conversations: &HashMap<String, (String, Selection)>,
    today: chrono::NaiveDate,
    offset: chrono::FixedOffset,
) -> Vec<MessageResult> {
    hits.into_iter()
        .filter_map(|hit| {
            let (conversation, selection) = conversations.get(&hit.conversation_id)?.clone();
            let (snippet, matches) = parse_snippet(&hit.snippet);
            Some(MessageResult {
                selection,
                message_id: hit.message_id,
                author: hit.sender_name.unwrap_or_else(|| "Unknown".to_owned()),
                conversation,
                date: format::list_time_label(hit.created_at, today, offset),
                snippet,
                matches,
            })
        })
        .collect()
}

#[derive(Clone)]
pub enum CandidateAvatar {
    Chat(AvatarSpec),
    Team { name: String, key: String },
}

#[derive(Clone)]
pub struct Candidate {
    pub title: String,
    pub subtitle: String,
    pub avatar: CandidateAvatar,
    pub selection: Selection,
}

impl Candidate {
    fn group(&self) -> &'static str {
        match self.selection {
            Selection::Chat(_) => "Chats",
            Selection::Channel(_) => "Channels",
        }
    }
}

pub struct Switcher {
    app: Entity<AppState>,
    input: Entity<InputState>,
    candidates: Vec<(String, Candidate)>,
    conversations: HashMap<String, (String, Selection)>,
    results: Vec<Candidate>,
    people: Vec<PersonResult>,
    local_messages: Vec<MessageResult>,
    messages: Vec<MessageResult>,
    search_session: SearchSession,
    generation: u64,
    rows_scroll: ScrollHandle,
    highlighted: usize,
    _subscription: Subscription,
}

impl EventEmitter<SwitcherEvent> for Switcher {}

pub fn candidates_from(state: &AppState) -> Vec<(String, Candidate)> {
    let me = state.directory.me.as_ref();
    let chats = state.sidebar.chats.iter().map(|chat| {
        let title = chat_title(chat);
        (
            title.clone(),
            Candidate {
                title,
                subtitle: if is_one_on_one(chat) {
                    "Chat".to_owned()
                } else {
                    format!("Group chat, {} participants", chat.members.len())
                },
                avatar: CandidateAvatar::Chat(avatar_for(chat, me)),
                selection: Selection::Chat(chat.id.clone()),
            },
        )
    });
    let channels = state.sidebar.teams.iter().flat_map(|team| {
        team.channels.iter().map(|channel| {
            (
                format!("{} / {}", team.team.name, channel.name),
                Candidate {
                    title: channel.name.clone(),
                    subtitle: team.team.name.clone(),
                    avatar: CandidateAvatar::Team {
                        name: team.team.name.clone(),
                        key: team.team.id.clone(),
                    },
                    selection: Selection::Channel(channel.id.clone()),
                },
            )
        })
    });
    chats.chain(channels).collect()
}

pub fn candidate_avatar(
    directory: &Directory,
    avatar: &CandidateAvatar,
    size: f32,
    ring: Hsla,
) -> AnyElement {
    match avatar {
        CandidateAvatar::Chat(spec) => spec_avatar(directory, spec, size, ring),
        CandidateAvatar::Team { name, key } => {
            let photo = directory.avatar(&teams_core::team_avatar_key(key));
            team_avatar(photo, name, key, size, size / 4.)
        }
    }
}

pub fn clamp_highlight(current: usize, delta: isize, length: usize) -> usize {
    if length == 0 {
        return 0;
    }
    (current as isize + delta).clamp(0, length as isize - 1) as usize
}

impl Switcher {
    pub fn new(
        app: Entity<AppState>,
        candidates: Vec<(String, Candidate)>,
        initial_query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Jump to a chat, channel, person or message")
        });
        if !initial_query.is_empty() {
            let query = initial_query.to_owned();
            input.update(cx, |input, cx| input.set_value(query, window, cx));
        }
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        input.update(cx, |input, cx| input.focus(window, cx));
        let conversations = candidates
            .iter()
            .map(|(title, candidate)| {
                (
                    candidate.selection.conversation_id().to_owned(),
                    (title.clone(), candidate.selection.clone()),
                )
            })
            .collect();
        let mut switcher = Switcher {
            app,
            input,
            candidates,
            conversations,
            results: Vec::new(),
            people: Vec::new(),
            local_messages: Vec::new(),
            messages: Vec::new(),
            search_session: SearchSession::new(),
            generation: 0,
            rows_scroll: ScrollHandle::new(),
            highlighted: 0,
            _subscription: subscription,
        };
        switcher.refresh(initial_query, cx);
        switcher
    }

    fn refresh(&mut self, query: &str, cx: &mut Context<Self>) {
        let wants_messages = query.trim().chars().count() >= MIN_MESSAGE_QUERY_CHARS;
        let limit = if wants_messages {
            MAX_RESULTS_WITH_MESSAGES
        } else {
            MAX_RESULTS
        };
        self.results = fuzzy::rank(query, &self.candidates, limit);
        self.results
            .sort_by_key(|candidate| candidate.group() != "Chats");
        self.local_messages = if wants_messages {
            let hits = self
                .app
                .read(cx)
                .store
                .search_messages(query, None, MAX_MESSAGE_RESULTS)
                .unwrap_or_default();
            let now = Local::now();
            message_results(
                hits,
                &self.conversations,
                now.date_naive(),
                now.offset().fix(),
            )
        } else {
            Vec::new()
        };
        self.messages = self.local_messages.clone();
        self.people.clear();
        self.highlighted = 0;
        self.generation += 1;
        if query.trim().chars().count() >= MIN_SERVER_QUERY_CHARS {
            self.schedule_server_search(query.trim().to_owned(), cx);
        }
    }

    fn schedule_server_search(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(engine) = self.app.read(cx).engine.clone() else {
            return;
        };
        let generation = self.generation;
        let search_session = self.search_session.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SERVER_DEBOUNCE).await;
            let still_current = this
                .update(cx, |switcher, _| switcher.generation == generation)
                .unwrap_or(false);
            if !still_current {
                return;
            }
            let receiver =
                runtime::spawn(
                    async move { engine.server_suggestions(&search_session, &query).await },
                );
            let suggestions = match receiver.await {
                Ok(Ok(suggestions)) => suggestions,
                _ => return,
            };
            this.update(cx, |switcher, cx| {
                switcher.apply_server(generation, suggestions, cx)
            })
            .ok();
        })
        .detach();
    }

    fn apply_server(&mut self, generation: u64, suggestions: Suggestions, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        let shown_titles: HashSet<String> = self
            .results
            .iter()
            .map(|candidate| candidate.title.clone())
            .collect();
        let my_user_id = self
            .app
            .read(cx)
            .directory
            .me
            .as_ref()
            .map(|me| me.user_id.clone());
        self.people = people_results(
            &suggestions.people,
            &shown_titles,
            my_user_id.as_deref(),
            MAX_PEOPLE_RESULTS,
        );
        let now = Local::now();
        let server = server_message_results(
            &suggestions.messages,
            &self.conversations,
            now.date_naive(),
            now.offset().fix(),
        );
        self.messages = merge_messages(&self.local_messages, server, MAX_MESSAGE_RESULTS);
        self.highlighted = self.highlighted.min(self.result_count().saturating_sub(1));
        cx.notify();
    }

    fn result_count(&self) -> usize {
        self.results.len() + self.people.len() + self.messages.len()
    }

    fn on_input_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                self.refresh(&query, cx);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => self.pick(cx),
            _ => {}
        }
    }

    fn pick(&mut self, cx: &mut Context<Self>) {
        let person_index = self.highlighted.checked_sub(self.results.len());
        let message_index = person_index.and_then(|index| index.checked_sub(self.people.len()));
        if let Some(candidate) = self.results.get(self.highlighted) {
            cx.emit(SwitcherEvent::Pick(candidate.selection.clone()));
        } else if let Some(person) = person_index.and_then(|index| self.people.get(index)) {
            cx.emit(SwitcherEvent::PickPerson {
                user_id: person.user_id.clone(),
            });
        } else if let Some(message) = message_index.and_then(|index| self.messages.get(index)) {
            cx.emit(SwitcherEvent::PickMessage {
                selection: message.selection.clone(),
                message_id: message.message_id.clone(),
            });
        } else {
            cx.emit(SwitcherEvent::Close);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.highlighted = clamp_highlight(self.highlighted, 1, self.result_count()),
            "up" => self.highlighted = clamp_highlight(self.highlighted, -1, self.result_count()),
            "escape" => cx.emit(SwitcherEvent::Close),
            _ => return,
        }
        self.rows_scroll.scroll_to_item(row_position(
            &self.results,
            self.people.len(),
            self.highlighted,
        ));
        cx.notify();
    }
}

fn row_position(results: &[Candidate], people: usize, highlighted: usize) -> usize {
    let groups = |candidates: &[Candidate]| {
        candidates
            .iter()
            .enumerate()
            .filter(|(index, candidate)| {
                *index == 0 || candidates[index - 1].group() != candidate.group()
            })
            .count()
    };
    if highlighted < results.len() {
        return groups(&results[..=highlighted]) + highlighted;
    }
    let before_people = groups(results) + results.len();
    let person = highlighted - results.len();
    if person < people {
        return before_people + 1 + person;
    }
    let before_messages = before_people + if people > 0 { 1 + people } else { 0 };
    before_messages + 1 + (person - people)
}

fn group_header(label: &'static str) -> AnyElement {
    div()
        .px(px(12.))
        .pt(px(8.))
        .pb(px(4.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(label)
        .into_any_element()
}

impl Render for Switcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let highlighted = self.highlighted;
        let directory = &self.app.read(cx).directory;
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_group = "";
        for (index, candidate) in self.results.iter().enumerate() {
            if candidate.group() != last_group {
                last_group = candidate.group();
                rows.push(group_header(last_group));
            }
            let selection = candidate.selection.clone();
            let avatar = candidate_avatar(directory, &candidate.avatar, 28., theme::popup());
            let kind_icon = match candidate.selection {
                Selection::Chat(_) => IconName::MessageCircle,
                Selection::Channel(_) => IconName::Hash,
            };
            rows.push(
                h_flex()
                    .id(ElementId::Name(format!("switch-{index}").into()))
                    .mx(px(6.))
                    .px(px(6.))
                    .h(px(44.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(index == highlighted, |row| row.bg(theme::popup_hover()))
                    .hover(|row| row.bg(theme::popup_hover()))
                    .child(avatar)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .text_color(theme::text())
                                    .child(candidate.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(candidate.subtitle.clone()),
                            ),
                    )
                    .child(icon(kind_icon, 14., theme::text_muted()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SwitcherEvent::Pick(selection.clone()));
                    }))
                    .into_any_element(),
            );
        }
        if !self.people.is_empty() {
            rows.push(group_header(PEOPLE_GROUP));
        }
        for (offset, person) in self.people.iter().enumerate() {
            let index = self.results.len() + offset;
            let user_id = person.user_id.clone();
            rows.push(
                h_flex()
                    .id(ElementId::Name(format!("switch-person-{index}").into()))
                    .mx(px(6.))
                    .px(px(6.))
                    .h(px(44.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(index == highlighted, |row| row.bg(theme::popup_hover()))
                    .hover(|row| row.bg(theme::popup_hover()))
                    .child(person_avatar(
                        directory,
                        Some(&person.user_id),
                        &person.name,
                        28.,
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .text_color(theme::text())
                                    .child(person.name.clone()),
                            )
                            .when(!person.subtitle.is_empty(), |column| {
                                column.child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(theme::text_muted())
                                        .child(person.subtitle.clone()),
                                )
                            }),
                    )
                    .child(icon(IconName::User, 14., theme::text_muted()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SwitcherEvent::PickPerson {
                            user_id: user_id.clone(),
                        });
                    }))
                    .into_any_element(),
            );
        }
        if !self.messages.is_empty() {
            rows.push(group_header(MESSAGE_GROUP));
        }
        for (offset, message) in self.messages.iter().enumerate() {
            let index = self.results.len() + self.people.len() + offset;
            let (selection, message_id) = (message.selection.clone(), message.message_id.clone());
            let marks = message
                .matches
                .iter()
                .map(|range| {
                    (
                        range.clone(),
                        HighlightStyle {
                            color: Some(theme::text()),
                            font_weight: Some(FontWeight::BOLD),
                            ..Default::default()
                        },
                    )
                })
                .collect::<Vec<_>>();
            rows.push(
                h_flex()
                    .id(ElementId::Name(format!("switch-message-{index}").into()))
                    .mx(px(6.))
                    .px(px(6.))
                    .h(px(52.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(index == highlighted, |row| row.bg(theme::popup_hover()))
                    .hover(|row| row.bg(theme::popup_hover()))
                    .child(person_avatar(directory, None, &message.author, 28.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(1.))
                            .child(
                                h_flex()
                                    .gap(px(6.))
                                    .items_baseline()
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::text())
                                            .child(message.author.clone()),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(12.))
                                            .text_color(theme::text_muted())
                                            .child(format!("in {}", message.conversation)),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.))
                                            .text_color(theme::text_muted())
                                            .child(message.date.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .text_color(theme::text_soft())
                                    .child(
                                        StyledText::new(message.snippet.clone())
                                            .with_highlights(marks),
                                    ),
                            ),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SwitcherEvent::PickMessage {
                            selection: selection.clone(),
                            message_id: message_id.clone(),
                        });
                    }))
                    .into_any_element(),
            );
        }
        let footer = h_flex()
            .h(px(32.))
            .px(px(12.))
            .gap(px(16.))
            .items_center()
            .text_size(px(11.))
            .text_color(theme::text_muted())
            .child("Enter to open")
            .child("Arrows to select")
            .child("Esc to close");
        div()
            .id("switcher-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(CARD_TOP))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(SwitcherEvent::Close)))
            .child(
                v_flex()
                    .id("switcher-card")
                    .w(px(CARD_WIDTH))
                    .max_h(px(640.))
                    .rounded(px(10.))
                    .flyout_surface()
                    .overflow_hidden()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(
                        h_flex()
                            .p(px(8.))
                            .pl(px(12.))
                            .gap(px(8.))
                            .items_center()
                            .child(icon(IconName::Search, 14., theme::text_muted()))
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&self.input).appearance(false).bordered(false),
                                ),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("switcher-rows")
                            .track_scroll(&self.rows_scroll)
                            .w_full()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .py_1()
                            .children(rows),
                    )
                    .child(footer),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Candidate, CandidateAvatar, clamp_highlight, parse_snippet, row_position};
    use crate::app_state::Selection;
    use teams_core::{HIGHLIGHT_END, HIGHLIGHT_START};

    #[test]
    fn snippet_markers_become_ranges() {
        let raw = format!("a {HIGHLIGHT_START}Build{HIGHLIGHT_END} 42\nok");
        let (text, matches) = parse_snippet(&raw);
        assert_eq!(text, "a Build 42 ok");
        assert_eq!(matches, vec![2..7]);
    }

    #[test]
    fn unterminated_or_empty_markers_are_ignored() {
        let raw = format!("{HIGHLIGHT_START}{HIGHLIGHT_END}x{HIGHLIGHT_START}y");
        assert_eq!(parse_snippet(&raw), ("xy".to_owned(), Vec::new()));
    }

    #[test]
    fn highlight_stays_inside_the_list() {
        assert_eq!(clamp_highlight(0, -1, 3), 0);
        assert_eq!(clamp_highlight(2, 1, 3), 2);
        assert_eq!(clamp_highlight(1, 1, 3), 2);
        assert_eq!(clamp_highlight(0, 1, 0), 0);
    }

    fn candidate(selection: Selection) -> Candidate {
        Candidate {
            title: String::new(),
            subtitle: String::new(),
            avatar: CandidateAvatar::Team {
                name: String::new(),
                key: String::new(),
            },
            selection,
        }
    }

    #[test]
    fn row_positions_count_the_group_headers() {
        let results = [
            candidate(Selection::Chat("a".into())),
            candidate(Selection::Chat("b".into())),
            candidate(Selection::Channel("c".into())),
        ];
        assert_eq!(row_position(&results, 2, 0), 1);
        assert_eq!(row_position(&results, 2, 2), 4);
        assert_eq!(row_position(&results, 2, 3), 6);
        assert_eq!(row_position(&results, 2, 5), 9);
        assert_eq!(row_position(&results, 0, 3), 6);
    }
}
