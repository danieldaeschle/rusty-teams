use std::collections::HashMap;
use std::ops::Range;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    h_flex,
    input::{
        Backspace, Enter, Escape, IndentInline, InlineToken, InputContent, InputEvent, MoveDown,
        MoveUp, Textarea, TextareaState,
    },
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{MentionCandidate, MentionInput};

use super::avatar::{person_avatar, square_avatar};
use super::emoji_popup::{self, EmojiPopup};
use super::widgets::{icon, symbol};
use crate::app_state::AppState;
use crate::emoji;
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
pub struct EditPreview {
    pub message_id: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub text: String,
    pub mentions: Vec<MentionInput>,
    pub reply: Option<ReplyPreview>,
    pub edit: Option<EditPreview>,
}

pub enum ComposerEvent {
    Submit(Outgoing),
    EditLast,
}

struct MentionPopup {
    range: Range<usize>,
    query: String,
    candidates: Vec<MentionCandidate>,
    highlighted: usize,
}

struct Conversion {
    value: String,
    cursor: usize,
    range: Range<usize>,
    original: String,
}

pub struct Composer {
    app: Entity<AppState>,
    input: Entity<TextareaState>,
    conversation_id: Option<String>,
    reply: Option<ReplyPreview>,
    editing: Option<EditPreview>,
    mention_inputs: HashMap<String, MentionInput>,
    mention_counter: usize,
    popup: Option<MentionPopup>,
    lookup: Option<Task<()>>,
    emoji_popup: Option<EmojiPopup>,
    emoji_dismissed_at: Option<usize>,
    recent_emoji: emoji::Recent,
    conversion: Option<Conversion>,
    undone_value: Option<String>,
    previous_value: String,
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
                .placeholder("Type a message")
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        let recent_emoji = emoji::Recent::load(&app.read(cx).store);
        Composer {
            app,
            input,
            conversation_id: None,
            reply: None,
            editing: None,
            mention_inputs: HashMap::new(),
            mention_counter: 0,
            popup: None,
            lookup: None,
            emoji_popup: None,
            emoji_dismissed_at: None,
            recent_emoji,
            conversion: None,
            undone_value: None,
            previous_value: String::new(),
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
            self.convert_typed(window, cx);
            self.update_emoji(cx);
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
            self.close_mention_popup();
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
        self.close_mention_popup();
        self.emoji_popup = None;
    }

    fn close_mention_popup(&mut self) {
        self.popup = None;
        self.lookup = None;
    }

    fn convert_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let (value, cursor) = (state.value().to_string(), state.cursor());
        let previous = std::mem::replace(&mut self.previous_value, value.clone());
        let found = match emoji::typed_char(&previous, &value, cursor) {
            Some(':') => emoji::closing_code(&value, cursor)
                .and_then(|(range, code)| Some((range, emoji::lookup(code)?))),
            Some(' ') => emoji::smiley_before_space(&value, cursor),
            _ => None,
        };
        let Some((range, glyph)) = found else {
            return;
        };
        let original = value[range.clone()].to_owned();
        let cursor = self.replace_text(range.clone(), glyph, cursor, window, cx);
        self.previous_value = self.input.read(cx).value().to_string();
        self.conversion = Some(Conversion {
            value: self.previous_value.clone(),
            cursor,
            range: range.start..range.start + glyph.len(),
            original,
        });
        self.remember_emoji(glyph, cx);
    }

    /// Returns the cursor, kept at the same spot relative to the text after `range`.
    fn replace_text(
        &mut self,
        range: Range<usize>,
        text: &str,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let cursor = cursor + text.len() - range.len();
        self.input.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.replace(text.to_owned(), window, cx);
            state.set_selected_range(cursor..cursor, cx);
        });
        cursor
    }

    fn undo_conversion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let state = self.input.read(cx);
        let unchanged = self.conversion.as_ref().is_some_and(|conversion| {
            conversion.value == state.value().as_ref() && conversion.cursor == state.cursor()
        });
        let Some(conversion) = self.conversion.take().filter(|_| unchanged) else {
            return false;
        };
        self.replace_text(
            conversion.range,
            &conversion.original,
            conversion.cursor,
            window,
            cx,
        );
        self.undone_value = Some(self.input.read(cx).value().to_string());
        self.update_emoji(cx);
        true
    }

    fn remember_emoji(&mut self, glyph: &str, cx: &App) {
        self.recent_emoji.push(glyph);
        self.recent_emoji.save(&self.app.read(cx).store);
    }

    fn update_emoji(&mut self, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let found = emoji::active_query(&state.value(), state.cursor());
        if found.as_ref().map(|(range, _)| range.start) != self.emoji_dismissed_at {
            self.emoji_dismissed_at = None;
        }
        let found = found.filter(|_| self.emoji_dismissed_at.is_none());
        let Some((range, query)) = found else {
            self.emoji_popup = None;
            return;
        };
        let unchanged = self
            .emoji_popup
            .as_ref()
            .is_some_and(|popup| popup.query == query && popup.range == range);
        if unchanged {
            return;
        }
        let matches = emoji::search(&query, self.recent_emoji.glyphs(), emoji_popup::LIMIT);
        self.emoji_popup = (!matches.is_empty()).then(|| EmojiPopup::new(range, query, matches));
    }

    fn accept_emoji(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.emoji_popup.take() else {
            return;
        };
        let Some(glyph) = popup.selected().map(|found| found.glyph) else {
            return;
        };
        let cursor = popup.range.end;
        self.replace_text(popup.range, glyph, cursor, window, cx);
        self.remember_emoji(glyph, cx);
        cx.notify();
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
        self.emoji_popup.is_some()
            || self
                .popup
                .as_ref()
                .is_some_and(|popup| !popup.candidates.is_empty())
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(popup) = self.emoji_popup.as_mut() {
            popup.move_highlight(delta);
            cx.notify();
        } else if let Some(popup) = self.popup.as_mut() {
            let last = popup.candidates.len().saturating_sub(1) as isize;
            popup.highlighted = (popup.highlighted as isize + delta).clamp(0, last) as usize;
            cx.notify();
        }
    }

    fn accept_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.emoji_popup.is_some() {
            self.accept_emoji(window, cx);
        } else {
            self.accept_mention(window, cx);
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

    pub fn set_placeholder(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_owned();
        self.input
            .update(cx, |state, cx| state.set_placeholder(text, window, cx));
    }

    pub fn set_conversation(
        &mut self,
        conversation_id: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_placeholder(&format!("Message {name}"), window, cx);
        if self.conversation_id.as_deref() != Some(conversation_id) {
            self.conversation_id = Some(conversation_id.to_owned());
            self.reply = None;
            if self.editing.take().is_some() {
                self.input
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
            self.mention_inputs.clear();
            self.close_popup();
            cx.notify();
        }
    }

    pub fn set_reply(&mut self, reply: Option<ReplyPreview>, cx: &mut Context<Self>) {
        self.reply = reply;
        if self.reply.is_some() {
            self.editing = None;
        }
        cx.notify();
    }

    pub fn begin_edit(&mut self, draft: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        self.restore(&draft, window, cx);
        self.focus(window, cx);
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.set_text("", window, cx);
    }

    fn outgoing(&self, cx: &App) -> Option<Outgoing> {
        let state = self.input.read(cx);
        let mut text = state.value().trim().to_owned();
        if text.is_empty() {
            return None;
        }
        if self.undone_value.as_deref() != Some(state.value().as_ref())
            && let Some((range, glyph)) = emoji::trailing_smiley(&text)
        {
            text.replace_range(range, glyph);
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
            edit: self.editing.clone(),
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
        self.editing = None;
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
        self.editing = outgoing.edit.clone();
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
        self.update_emoji(cx);
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

    fn render_emoji_popup(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let popup = self.emoji_popup.as_ref()?;
        let colon = popup.range.start..popup.range.start + 1;
        let anchor = self
            .input
            .read(cx)
            .range_to_bounds(&colon)
            .map(|bounds| bounds.origin);
        if anchor.is_none() {
            window.request_animation_frame();
        }
        let composer = cx.entity().downgrade();
        Some(emoji_popup::render(
            popup,
            anchor,
            move |index, window, cx| {
                composer
                    .update(cx, |this, cx| {
                        if let Some(popup) = this.emoji_popup.as_mut() {
                            popup.highlighted = index;
                        }
                        this.accept_emoji(window, cx);
                    })
                    .ok();
            },
        ))
    }

    fn render_edit_strip(&self, cx: &mut Context<Self>) -> Option<Div> {
        let edit = self.editing.as_ref()?;
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
                .child(icon(IconName::Pencil, 14., theme::accent_text()))
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
                                .child("Editing message"),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(edit.excerpt.clone()),
                        ),
                )
                .child(
                    div()
                        .id("edit-close")
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::row_hover()))
                        .child(icon(IconName::Close, 14., theme::text_muted()))
                        .on_click(cx.listener(|this, _, window, cx| this.cancel_edit(window, cx))),
                ),
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
                                .child(format!("Replying to {}", reply.author)),
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
                } else if this.editing.is_none() && this.is_empty(cx) {
                    cx.emit(ComposerEvent::EditLast);
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
                    this.accept_popup(window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if this.popup_is_open() {
                    this.accept_popup(window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Backspace, window, cx| {
                if this.undo_conversion(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if let Some(popup) = &this.emoji_popup {
                    this.emoji_dismissed_at = Some(popup.range.start);
                }
                if this.popup.is_some() || this.emoji_popup.is_some() {
                    this.close_popup();
                    cx.notify();
                    cx.stop_propagation();
                } else if this.editing.is_some() {
                    this.cancel_edit(window, cx);
                    cx.stop_propagation();
                }
            }))
            .children(self.render_reply_strip(cx))
            .children(self.render_edit_strip(cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .children(self.render_popup(cx))
                    .children(self.render_emoji_popup(window, cx))
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

    mod keys {
        use std::sync::Arc;

        use gpui_kit::test::TestWindowExt as _;
        use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
        use store::Store;

        use super::super::Composer;
        use crate::app_state::{AppState, Mode};

        #[gpui_kit::test]
        fn ctrl_a_selects_the_whole_draft(cx: &mut TestAppContext) {
            cx.update(gpui_kit::init);
            let store = Arc::new(Store::open_in_memory().unwrap());
            let mode = Mode {
                demo: true,
                read_only: false,
                demo_sync: None,
            };
            let (handle, composer) = cx.update(|cx| {
                let app = cx.new(|_| AppState::new(store, mode));
                gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                    cx.new(|cx| Composer::new(app, window, cx))
                })
                .unwrap()
            });
            let value = cx
                .update_window(handle, |_, window, cx| {
                    composer.update(cx, |composer, cx| composer.focus(window, cx));
                    window.render_frame(cx);
                    window.input("first line", cx);
                    window.press("shift-enter", cx);
                    window.input("second line", cx);
                    window.press("ctrl-a", cx);
                    window.input("X", cx);
                    composer.read(cx).input.read(cx).value()
                })
                .unwrap();
            assert_eq!(value, "X");
        }
    }
}
