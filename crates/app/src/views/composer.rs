use std::collections::HashMap;
use std::ops::Range;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    h_flex,
    input::{
        Enter, Escape, IndentInline, InlineToken, InputContent, InputEvent, MoveDown, MoveUp,
        Textarea, TextareaState,
    },
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{MentionCandidate, MentionInput};

use super::avatar::{person_avatar, square_avatar};
use super::widgets::{icon, symbol};
use crate::app_state::AppState;
use crate::runtime;
use crate::theme;

const MIN_ROWS: usize = 1;
const MAX_ROWS: usize = 8;
const MENTION_DEBOUNCE: Duration = Duration::from_millis(150);
const MENTION_LIMIT: usize = 8;
const MENTION_QUERY_MAX_CHARS: usize = 32;
const MENTION_QUERY_MAX_WORDS: usize = 3;
const POPUP_WIDTH: f32 = 380.;
const POPUP_ROW_HEIGHT: f32 = 44.;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyPreview {
    pub message_id: String,
    pub author: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub text: String,
    pub mentions: Vec<MentionInput>,
    pub reply: Option<ReplyPreview>,
}

pub enum ComposerEvent {
    Submit(Outgoing),
}

struct MentionPopup {
    range: Range<usize>,
    query: String,
    candidates: Vec<MentionCandidate>,
    highlighted: usize,
}

pub struct Composer {
    app: Entity<AppState>,
    input: Entity<TextareaState>,
    conversation_id: Option<String>,
    reply: Option<ReplyPreview>,
    mention_inputs: HashMap<String, MentionInput>,
    mention_counter: usize,
    popup: Option<MentionPopup>,
    lookup: Option<Task<()>>,
    _subscription: Subscription,
}

impl EventEmitter<ComposerEvent> for Composer {}

/// Enter sends, Shift+Enter is a newline (handled by the input itself). Blank text never sends.
pub fn submitted_text(event: &InputEvent, value: &str) -> Option<String> {
    match event {
        InputEvent::PressEnter { shift: false, .. } => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        _ => None,
    }
}

pub fn active_mention(
    text: &str,
    cursor: usize,
    tokens: &[Range<usize>],
) -> Option<(Range<usize>, String)> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let at = before.rfind('@')?;
    if before[..at]
        .chars()
        .next_back()
        .is_some_and(|previous| !previous.is_whitespace())
    {
        return None;
    }
    let query = &before[at + 1..];
    let rejected = query.contains('\n')
        || query.starts_with(char::is_whitespace)
        || query.ends_with(char::is_whitespace)
        || query.chars().count() > MENTION_QUERY_MAX_CHARS
        || query.split_whitespace().count() > MENTION_QUERY_MAX_WORDS
        || tokens
            .iter()
            .any(|token| token.start < cursor && at < token.end);
    (!rejected).then(|| (at..cursor, query.to_owned()))
}

fn candidate_subtitle(candidate: &MentionCandidate) -> String {
    match candidate {
        MentionCandidate::Person(person) => person
            .job_title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .or_else(|| person.mail.clone())
            .unwrap_or_default(),
        MentionCandidate::Channel { .. } => "Channel".to_owned(),
        MentionCandidate::Team { .. } => "Team".to_owned(),
    }
}

fn candidate_key(candidate: &MentionCandidate) -> &str {
    match candidate {
        MentionCandidate::Person(person) => &person.user_id,
        MentionCandidate::Channel { channel_id, .. } => channel_id,
        MentionCandidate::Team { team_id, .. } => team_id,
    }
}

impl Composer {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(MIN_ROWS, MAX_ROWS)
                .submit_on_enter(true)
                .placeholder("Nachricht")
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        Composer {
            app,
            input,
            conversation_id: None,
            reply: None,
            mention_inputs: HashMap::new(),
            mention_counter: 0,
            popup: None,
            lookup: None,
            _subscription: subscription,
        }
    }

    fn on_input_event(
        &mut self,
        input: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        if matches!(event, InputEvent::Change) {
            self.update_mention(cx);
        }
        let value = input.read(cx).value();
        if submitted_text(event, &value).is_some() {
            self.submit_current(window, cx);
        }
    }

    fn update_mention(&mut self, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let tokens: Vec<Range<usize>> = state.tokens().iter().map(|span| span.range()).collect();
        let found = active_mention(&state.value(), state.cursor(), &tokens);
        let Some((range, query)) = found else {
            self.close_popup();
            return;
        };
        let unchanged = self
            .popup
            .as_ref()
            .is_some_and(|popup| popup.query == query && popup.range == range);
        if unchanged {
            return;
        }
        let previous = self.popup.take();
        self.popup = Some(MentionPopup {
            range,
            query: query.clone(),
            candidates: previous.map(|popup| popup.candidates).unwrap_or_default(),
            highlighted: 0,
        });
        self.request_candidates(query, cx);
    }

    fn close_popup(&mut self) {
        self.popup = None;
        self.lookup = None;
    }

    fn request_candidates(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.conversation_id.clone() else {
            return;
        };
        let (demo, engine) = {
            let state = self.app.read(cx);
            (state.mode.demo, state.engine.clone())
        };
        if demo {
            let candidates = crate::demo::mention_candidates(&conversation_id, &query);
            self.apply_candidates(&query, candidates, cx);
            return;
        }
        let Some(engine) = engine else {
            return;
        };
        self.lookup = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MENTION_DEBOUNCE).await;
            let task_query = query.clone();
            let receiver = runtime::spawn(async move {
                engine
                    .mention_candidates(&conversation_id, &task_query, MENTION_LIMIT)
                    .await
            });
            if let Ok(Ok(candidates)) = receiver.await {
                this.update(cx, |this, cx| this.apply_candidates(&query, candidates, cx))
                    .ok();
            }
        }));
    }

    fn apply_candidates(
        &mut self,
        query: &str,
        candidates: Vec<MentionCandidate>,
        cx: &mut Context<Self>,
    ) {
        let Some(popup) = self.popup.as_mut().filter(|popup| popup.query == query) else {
            return;
        };
        popup.candidates = candidates;
        popup.highlighted = 0;
        cx.notify();
    }

    fn popup_is_open(&self) -> bool {
        self.popup
            .as_ref()
            .is_some_and(|popup| !popup.candidates.is_empty())
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(popup) = self.popup.as_mut() {
            let last = popup.candidates.len().saturating_sub(1) as isize;
            popup.highlighted = (popup.highlighted as isize + delta).clamp(0, last) as usize;
            cx.notify();
        }
    }

    fn accept_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        self.lookup = None;
        let Some(candidate) = popup.candidates.get(popup.highlighted) else {
            return;
        };
        self.mention_counter += 1;
        let token_id = format!(
            "mention-{}-{}",
            self.mention_counter,
            candidate_key(candidate)
        );
        let label = format!("@{}", candidate.display_name());
        self.mention_inputs
            .insert(token_id.clone(), candidate.to_mention());
        self.input.update(cx, |state, cx| {
            let token = InlineToken::new(token_id, label);
            if state
                .replace_range_with_token(popup.range, token, window, cx)
                .is_ok()
            {
                state.insert(" ", window, cx);
            }
        });
        self.close_popup();
        cx.notify();
    }

    pub fn set_conversation(
        &mut self,
        conversation_id: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = format!("Nachricht an {name}");
        self.input.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx)
        });
        if self.conversation_id.as_deref() != Some(conversation_id) {
            self.conversation_id = Some(conversation_id.to_owned());
            self.reply = None;
            self.mention_inputs.clear();
            self.close_popup();
            cx.notify();
        }
    }

    pub fn set_reply(&mut self, reply: Option<ReplyPreview>, cx: &mut Context<Self>) {
        self.reply = reply;
        cx.notify();
    }

    fn outgoing(&self, cx: &App) -> Option<Outgoing> {
        let state = self.input.read(cx);
        let text = state.value().trim().to_owned();
        if text.is_empty() {
            return None;
        }
        let mentions = state
            .tokens()
            .iter()
            .filter_map(|span| self.mention_inputs.get(span.token().id().as_ref()))
            .cloned()
            .collect();
        Some(Outgoing {
            text,
            mentions,
            reply: self.reply.clone(),
        })
    }

    pub fn submit_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(outgoing) = self.outgoing(cx) else {
            return;
        };
        self.input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.mention_inputs.clear();
        self.reply = None;
        self.close_popup();
        cx.emit(ComposerEvent::Submit(outgoing));
        cx.notify();
    }

    pub fn restore(&mut self, outgoing: &Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        self.mention_inputs.clear();
        let mut content = InputContent::new(outgoing.text.clone());
        let mut cursor = 0;
        for mention in &outgoing.mentions {
            let needle = format!("@{}", mention.text);
            let Some(offset) = outgoing.text[cursor..].find(&needle) else {
                continue;
            };
            let range = cursor + offset..cursor + offset + needle.len();
            self.mention_counter += 1;
            let token_id = format!("mention-{}", self.mention_counter);
            let token = InlineToken::new(token_id.clone(), needle);
            if let Ok(next) = content.clone().with_token(range.clone(), token) {
                content = next;
                self.mention_inputs.insert(token_id, mention.clone());
                cursor = range.end;
            }
        }
        let end = outgoing.text.len();
        self.input.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            state.set_selected_range(end..end, cx);
        });
        self.reply = outgoing.reply.clone();
        self.close_popup();
        cx.notify();
    }

    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_owned();
        let end = text.len();
        self.input.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.set_selected_range(end..end, cx);
        });
        self.mention_inputs.clear();
        self.update_mention(cx);
        cx.notify();
    }

    pub fn is_empty(&self, cx: &App) -> bool {
        self.input.read(cx).value().trim().is_empty()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.focus(window, cx));
    }

    fn render_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popup = self
            .popup
            .as_ref()
            .filter(|popup| !popup.candidates.is_empty())?;
        let directory = &self.app.read(cx).directory;
        let rows = popup
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let name = candidate.display_name().to_owned();
                let avatar = match candidate {
                    MentionCandidate::Person(person) => {
                        person_avatar(directory, Some(&person.user_id), &name, 28.)
                    }
                    other => square_avatar(&name, candidate_key(other), 28., 7.).into_any_element(),
                };
                let subtitle = candidate_subtitle(candidate);
                h_flex()
                    .id(ElementId::Name(format!("mention-row-{index}").into()))
                    .mx(px(6.))
                    .px(px(6.))
                    .h(px(POPUP_ROW_HEIGHT))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(index == popup.highlighted, |row| {
                        row.bg(theme::surface_raised())
                    })
                    .hover(|row| row.bg(theme::row_hover()))
                    .child(avatar)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .text_color(theme::text())
                                    .child(name),
                            )
                            .when(!subtitle.is_empty(), |column| {
                                column.child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(theme::text_muted())
                                        .child(subtitle),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(popup) = this.popup.as_mut() {
                            popup.highlighted = index;
                        }
                        this.accept_mention(window, cx);
                    }))
            });
        Some(
            v_flex()
                .id("mention-popup")
                .absolute()
                .bottom(relative(1.))
                .left_0()
                .mb(px(6.))
                .w(px(POPUP_WIDTH))
                .py(px(6.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::border_strong())
                .bg(theme::surface())
                .shadow_lg()
                .occlude()
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_reply_strip(&self, cx: &mut Context<Self>) -> Option<Div> {
        let reply = self.reply.as_ref()?;
        Some(
            h_flex()
                .w_full()
                .mb(px(6.))
                .gap(px(10.))
                .items_center()
                .pl(px(10.))
                .pr(px(4.))
                .py(px(6.))
                .rounded(px(8.))
                .bg(theme::surface())
                .border_1()
                .border_color(theme::border())
                .child(icon(IconName::Undo2, 14., theme::accent_text()))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::accent_text())
                                .child(format!("Antwort an {}", reply.author)),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(reply.excerpt.clone()),
                        ),
                )
                .child(
                    div()
                        .id("reply-close")
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::row_hover()))
                        .child(icon(IconName::Close, 14., theme::text_muted()))
                        .on_click(cx.listener(|this, _, _, cx| this.set_reply(None, cx))),
                ),
        )
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.input.focus_handle(cx).contains_focused(window, cx);
        let empty = self.is_empty(cx);
        let send = div()
            .id("composer-send")
            .size(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .child(symbol("keyboard_return", 20., white()))
            .when(empty, |button| button.opacity(0.4))
            .when(!empty, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(white().opacity(0.1)))
                    .on_click(cx.listener(|this, _, window, cx| this.submit_current(window, cx)))
            });
        let input = Textarea::new(&self.input)
            .appearance(false)
            .bordered(false)
            .token(|context, _, _| {
                div()
                    .h(context.line_height())
                    .px(px(5.))
                    .rounded(px(5.))
                    .bg(if context.is_selected() {
                        theme::accent().opacity(0.4)
                    } else {
                        theme::mention_background(false)
                    })
                    .text_color(theme::mention_text())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(context.token().label().clone())
            });
        v_flex()
            .w_full()
            .flex_none()
            .px(px(24.))
            .pt(px(8.))
            .pb(px(12.))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                if this.popup_is_open() {
                    this.move_highlight(-1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                if this.popup_is_open() {
                    this.move_highlight(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                if this.popup_is_open() {
                    this.accept_mention(window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if this.popup_is_open() {
                    this.accept_mention(window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, _, cx| {
                if this.popup.is_some() {
                    this.close_popup();
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .children(self.render_reply_strip(cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .children(self.render_popup(cx))
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap(px(6.))
                            .py(px(6.))
                            .pl(px(12.))
                            .pr(px(6.))
                            .rounded(px(10.))
                            .bg(theme::surface())
                            .border_1()
                            .border_color(if focused {
                                theme::accent()
                            } else {
                                theme::border_strong()
                            })
                            .child(div().flex_1().min_w_0().child(input))
                            .child(send),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{active_mention, submitted_text};
    use gpui_kit::component::input::InputEvent;

    fn press(shift: bool) -> InputEvent {
        InputEvent::PressEnter {
            secondary: false,
            shift,
        }
    }

    #[test]
    fn enter_sends_trimmed_text() {
        assert_eq!(
            submitted_text(&press(false), "  hello \n"),
            Some("hello".into())
        );
    }

    #[test]
    fn shift_enter_never_sends() {
        assert_eq!(submitted_text(&press(true), "hello"), None);
    }

    #[test]
    fn blank_text_never_sends() {
        assert_eq!(submitted_text(&press(false), " \n "), None);
    }

    #[test]
    fn other_events_never_send() {
        assert_eq!(submitted_text(&InputEvent::Change, "hello"), None);
    }

    #[test]
    fn at_sign_opens_a_mention_query() {
        assert_eq!(active_mention("hi @Ma", 6, &[]), Some((3..6, "Ma".into())));
        assert_eq!(active_mention("@", 1, &[]), Some((0..1, String::new())));
    }

    #[test]
    fn names_with_a_space_keep_the_query_open() {
        assert_eq!(
            active_mention("@Mara Lin", 9, &[]),
            Some((0..9, "Mara Lin".into()))
        );
    }

    #[test]
    fn mail_addresses_and_closed_queries_do_not_open() {
        assert_eq!(active_mention("a@b", 3, &[]), None);
        assert_eq!(active_mention("@Mara ", 6, &[]), None);
        assert_eq!(active_mention("@ x", 3, &[]), None);
        assert_eq!(active_mention("@Ma\nrest", 8, &[]), None);
    }

    #[test]
    fn text_inside_a_token_does_not_reopen() {
        let token = 0..15;
        assert_eq!(
            active_mention("@Mara Lindqvist", 15, std::slice::from_ref(&token)),
            None
        );
        assert_eq!(
            active_mention("@Mara Lindqvist @Pr", 19, std::slice::from_ref(&token)),
            Some((16..19, "Pr".into()))
        );
    }
}
