use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, h_flex,
    input::{
        Backspace, Copy, Cut, Delete, Enter, Escape, IndentInline, InlineToken, InputContent,
        InputEvent, InputState, MoveDown, MoveUp, OutdentInline, RangeDecorationCollection, Redo,
        TextDecorationCollection, Textarea, TextareaMode, TextareaState, Undo,
    },
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{
    Draft, DraftLine, Edit, FileReference, FormatState, HostedImage, LineKind, MarkKind,
    MentionCandidate, MentionInput, MessageExtras, TypingStyle, UploadedFile, has_markdown,
    link_url, map_offset, reverse_edits,
};

use super::attachment_tray::{
    AttachmentTray, DoneFile, JobKind, LoadedFile, OutgoingFile, OutgoingImage, PasteAction,
    UploadJob, UploadResult, discard_uploaded, names_of, paste_action, pasted_image_name,
    prepare_pasted_image, read_attachment, render_tray,
};
use super::avatar::{person_avatar, square_avatar};
use super::draft_style::draft_style;
use super::emoji_popup::{self, EmojiPopup};
use super::format_toolbar::{self, FormatButton};
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
const DEMO_UPLOAD_STEPS: u8 = 10;
const DEMO_UPLOAD_STEP: Duration = Duration::from_millis(120);
const DEMO_FAILURE_STEP: u8 = 6;
const HISTORY_LIMIT: usize = 200;
const PASTE_HINT_DURATION: Duration = Duration::from_secs(4);
const KEY_CONTEXT: &str = "Composer";

actions!(
    composer,
    [
        ToggleBold,
        ToggleItalic,
        ToggleUnderline,
        ToggleStrike,
        ToggleCode,
        EditLink
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-b", ToggleBold, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-i", ToggleItalic, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-u", ToggleUnderline, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-x", ToggleStrike, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-c", ToggleCode, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-k", EditLink, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-b", ToggleBold, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-i", ToggleItalic, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-u", ToggleUnderline, Some(KEY_CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-k", EditLink, Some(KEY_CONTEXT)),
    ]);
}

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
    pub draft: Draft,
    pub mentions: Vec<MentionInput>,
    pub reply: Option<ReplyPreview>,
    pub edit: Option<EditPreview>,
    pub images: Vec<OutgoingImage>,
    pub files: Vec<OutgoingFile>,
}

impl Outgoing {
    /// Without list and quote markers, for previews.
    pub fn text(&self) -> String {
        self.draft.plain_text()
    }

    /// The body before mentions become `<at>` tags.
    pub fn html(&self) -> String {
        self.draft.to_html()
    }

    pub fn has_attachments(&self) -> bool {
        !self.images.is_empty() || !self.files.is_empty()
    }

    pub fn extras(&self) -> MessageExtras {
        MessageExtras {
            kept: Vec::new(),
            images: self
                .images
                .iter()
                .map(|outgoing| HostedImage {
                    content_type: outgoing.image.format.mime_type().to_owned(),
                    bytes: Arc::new(outgoing.image.bytes.clone()),
                })
                .collect(),
            files: self
                .files
                .iter()
                .map(|file| file.reference.clone())
                .collect(),
        }
    }
}

enum UploadEvent {
    Progress(u8),
    Uploaded(UploadedFile),
    Finished(UploadResult),
}

enum UploadHandle {
    Network(tokio::task::AbortHandle),
    Demo(Task<()>),
}

impl UploadHandle {
    fn cancel(self) {
        match self {
            UploadHandle::Network(handle) => handle.abort(),
            UploadHandle::Demo(task) => drop(task),
        }
    }
}

pub enum ComposerEvent {
    Submit(Box<Outgoing>),
    EditLast,
}

struct MentionPopup {
    range: Range<usize>,
    query: String,
    candidates: Vec<MentionCandidate>,
    highlighted: usize,
}

/// An automatic edit Backspace takes back while nothing changed since.
struct Conversion {
    value: String,
    cursor: usize,
    before: Draft,
    restore: Vec<Edit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepKind {
    Typing { after_space: bool },
    Deleting,
    Other,
}

/// One state of the composer's own undo history: text, tokens, formatting and selection.
struct HistoryStep {
    draft: Draft,
    tokens: Vec<(Range<usize>, InlineToken)>,
    selection: Range<usize>,
    kind: StepKind,
}

struct LinkEditor {
    range: Range<usize>,
    field: Entity<InputState>,
    refused: bool,
    _edits: Subscription,
}

type DraftDecorations = (
    TextDecorationCollection<TextareaMode>,
    RangeDecorationCollection<TextareaMode>,
);

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
    draft: Draft,
    decorations: DraftDecorations,
    pending_style: Option<TypingStyle>,
    history: Vec<HistoryStep>,
    history_index: usize,
    pasted_markdown: Option<String>,
    paste_hint: Option<Task<()>>,
    copied: Option<(String, Vec<DraftLine>)>,
    toolbar_dismissed: Option<Range<usize>>,
    mouse_selecting: bool,
    link_editor: Option<LinkEditor>,
    composer_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    tray: AttachmentTray,
    uploads: HashMap<u64, UploadHandle>,
    demo_failed: HashSet<u64>,
    carry_images_into: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<ComposerEvent> for Composer {}

pub fn is_plain_enter(event: &InputEvent) -> bool {
    matches!(event, InputEvent::PressEnter { shift: false, .. })
}

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

fn step_kind(previous: &str, value: &str, cursor: usize) -> StepKind {
    if let Some(typed) = emoji::typed_char(previous, value, cursor).filter(|typed| *typed != '\n') {
        return StepKind::Typing {
            after_space: typed.is_whitespace(),
        };
    }
    let removed = previous.len().saturating_sub(value.len());
    let one_char_removed = removed > 0
        && previous.is_char_boundary(cursor)
        && previous
            .get(cursor..cursor + removed)
            .is_some_and(|gone| gone.chars().count() == 1 && gone != "\n")
        && previous.get(..cursor) == value.get(..cursor)
        && previous.get(cursor + removed..) == value.get(cursor..);
    if one_char_removed {
        StepKind::Deleting
    } else {
        StepKind::Other
    }
}

fn looks_like_url(text: &str) -> bool {
    ["https://", "http://", "www."]
        .iter()
        .any(|prefix| text.starts_with(prefix))
        && !text.contains(char::is_whitespace)
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
        let observer = cx.observe(&input, |_, _, cx| cx.notify());
        let decorations = input.update(cx, |state, cx| {
            (
                state.create_decorations_collection(Vec::new(), cx),
                state.create_range_decorations_collection(Vec::new(), cx),
            )
        });
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
            draft: Draft::default(),
            decorations,
            pending_style: None,
            history: vec![HistoryStep {
                draft: Draft::default(),
                tokens: Vec::new(),
                selection: 0..0,
                kind: StepKind::Other,
            }],
            history_index: 0,
            pasted_markdown: None,
            paste_hint: None,
            copied: None,
            toolbar_dismissed: None,
            mouse_selecting: false,
            link_editor: None,
            composer_bounds: Rc::new(Cell::new(None)),
            tray: AttachmentTray::default(),
            uploads: HashMap::new(),
            demo_failed: HashSet::new(),
            carry_images_into: None,
            _subscriptions: [subscription, observer],
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
            self.on_change(window, cx);
            self.update_emoji(cx);
            self.update_mention(cx);
        }
        let value = input.read(cx).value();
        if submitted_text(event, &value).is_some()
            || (is_plain_enter(event) && !self.tray.is_empty())
        {
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

    fn on_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let (value, input_cursor) = (state.value().to_string(), state.cursor());
        let previous = std::mem::replace(&mut self.previous_value, value.clone());
        if value == self.draft.text() {
            self.refresh_style(cx);
            return;
        }
        let kind = step_kind(&previous, &value, input_cursor);
        let typing = self.pending_style.take();
        let cursor = self.draft.apply_edit(&value, input_cursor, typing.as_ref());
        let edits = self.draft.take_edits();
        self.apply_to_input(edits, cursor..cursor, window, cx);
        self.record(kind, cx);
        if let Some(pasted) = self.pasted_markdown.take()
            && self.convert_paste(&pasted, cursor, window, cx)
        {
            return;
        }
        let typed = emoji::typed_char(&previous, self.draft.text(), cursor);
        if typed.is_some() && self.convert_markdown(cursor, window, cx) {
            return;
        }
        self.convert_emoji(typed, cursor, window, cx);
    }

    fn convert_markdown(
        &mut self,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let before = self.draft.clone();
        let Some((cursor, style)) = self.draft.convert_typed(cursor) else {
            self.draft.take_edits();
            return false;
        };
        self.finish_conversion(before, cursor, window, cx);
        self.pending_style = (!style.is_empty()).then_some(style);
        true
    }

    fn convert_emoji(
        &mut self,
        typed: Option<char>,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft.in_code(cursor) {
            return;
        }
        let value = self.draft.text();
        let found = match typed {
            Some(':') => emoji::closing_code(value, cursor)
                .and_then(|(range, code)| Some((range, emoji::lookup(code)?))),
            Some(' ') => emoji::smiley_before_space(value, cursor),
            _ => None,
        };
        let Some((range, glyph)) = found else {
            return;
        };
        let before = self.draft.clone();
        self.draft.replace(range.clone(), glyph);
        self.finish_conversion(before, cursor + glyph.len() - range.len(), window, cx);
        self.remember_emoji(glyph, cx);
    }

    fn finish_conversion(
        &mut self,
        before: Draft,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let edits = self.draft.take_edits();
        let restore = reverse_edits(&edits, before.text());
        self.apply_to_input(edits, cursor..cursor, window, cx);
        self.record(StepKind::Other, cx);
        self.conversion = Some(Conversion {
            value: self.draft.text().to_owned(),
            cursor,
            before,
            restore,
        });
    }

    fn convert_paste(
        &mut self,
        pasted: &str,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pasted = pasted.replace("\r\n", "\n");
        let Some(start) = cursor
            .checked_sub(pasted.len())
            .filter(|start| self.draft.text().get(*start..cursor) == Some(pasted.as_str()))
        else {
            return false;
        };
        let lines = Draft::from_markdown(&pasted).slice(0..usize::MAX);
        let cursor = self.draft.insert_lines(start..cursor, &lines);
        self.commit(Some(cursor), window, cx);
        self.paste_hint = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PASTE_HINT_DURATION).await;
            this.update(cx, |this, cx| {
                this.paste_hint = None;
                cx.notify();
            })
            .ok();
        }));
        true
    }

    /// Puts the draft's pending edits into the input, then places the selection.
    fn commit(&mut self, cursor: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let edits = self.draft.take_edits();
        let selection = self.input.read(cx).selected_range();
        let selection = match cursor {
            Some(cursor) => cursor..cursor,
            None if edits.is_empty() => selection,
            None => map_offset(&edits, selection.start)..map_offset(&edits, selection.end),
        };
        self.apply_to_input(edits, selection, window, cx);
        self.record(StepKind::Other, cx);
    }

    fn apply_to_input(
        &mut self,
        edits: Vec<Edit>,
        selection: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input.update(cx, |state, cx| {
            let edited = !edits.is_empty();
            if edited {
                state.apply_edits(&edits, window, cx);
            }
            if edited || state.selected_range() != selection {
                state.set_selected_range(selection.clone(), cx);
            }
        });
        let value = self.input.read(cx).value().to_string();
        if value != self.draft.text() {
            self.draft.apply_edit(&value, selection.end, None);
            self.draft.take_edits();
        }
        self.previous_value = value;
        self.refresh_style(cx);
    }

    /// Adds the current state as an undo step. Typing and deleting runs merge into one step;
    /// a typed space ends a typing step.
    fn record(&mut self, kind: StepKind, cx: &App) {
        let state = self.input.read(cx);
        let step = HistoryStep {
            draft: self.draft.clone(),
            tokens: state
                .tokens()
                .iter()
                .map(|span| (span.range(), span.token().clone()))
                .collect(),
            selection: state.selected_range(),
            kind,
        };
        let at_end = self.history_index + 1 == self.history.len();
        let merges = at_end
            && match (self.history.last().map(|last| last.kind), kind) {
                (Some(StepKind::Typing { after_space: false }), StepKind::Typing { .. }) => true,
                (
                    Some(StepKind::Typing { after_space: true }),
                    StepKind::Typing { after_space },
                ) => after_space,
                (Some(StepKind::Deleting), StepKind::Deleting) => true,
                _ => false,
            };
        if merges {
            if let Some(last) = self.history.last_mut() {
                *last = step;
            }
            return;
        }
        self.history.truncate(self.history_index + 1);
        self.history.push(step);
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.history_index = self.history.len() - 1;
    }

    /// Ctrl+Z / Ctrl+Y move through the composer's own history; the input's is never used.
    fn step_history(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let target = if back {
            self.history_index.checked_sub(1)
        } else {
            Some(self.history_index + 1).filter(|index| *index < self.history.len())
        };
        let Some(target) = target else {
            return;
        };
        self.history_index = target;
        let step = &self.history[target];
        let mut content = InputContent::new(step.draft.text().to_owned());
        for (range, token) in &step.tokens {
            if let Ok(next) = content.clone().with_token(range.clone(), token.clone()) {
                content = next;
            }
        }
        let (draft, selection) = (step.draft.clone(), step.selection.clone());
        self.input.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            state.set_selected_range(selection, cx);
        });
        self.previous_value = draft.text().to_owned();
        self.draft = draft;
        self.pending_style = None;
        self.conversion = None;
        self.paste_hint = None;
        self.refresh_style(cx);
        self.update_emoji(cx);
        self.update_mention(cx);
        cx.notify();
    }

    fn refresh_style(&mut self, cx: &mut Context<Self>) {
        let style = draft_style(&self.draft, cx.theme().mono_font_family.clone());
        let (text, ranges) = &self.decorations;
        text.set(style.text, cx);
        ranges.set(style.ranges, cx);
        self.input.update(cx, |state, cx| {
            state.set_line_indents(style.indents, cx);
            state.set_hanging_indents(style.markers, cx);
        });
    }

    fn load_draft(
        &mut self,
        draft: Draft,
        content: InputContent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let end = draft.text().len();
        self.input.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            state.set_selected_range(end..end, cx);
        });
        self.previous_value = draft.text().to_owned();
        self.draft = draft;
        self.pending_style = None;
        self.conversion = None;
        self.pasted_markdown = None;
        self.paste_hint = None;
        self.link_editor = None;
        self.toolbar_dismissed = None;
        self.history.clear();
        self.history_index = 0;
        self.record(StepKind::Other, cx);
        self.refresh_style(cx);
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
        let cursor = map_offset(&conversion.restore, conversion.cursor);
        self.draft = conversion.before;
        self.pending_style = None;
        self.apply_to_input(conversion.restore, cursor..cursor, window, cx);
        self.record(StepKind::Other, cx);
        self.undone_value = Some(self.input.read(cx).value().to_string());
        self.update_emoji(cx);
        true
    }

    fn selection(&self, cx: &App) -> Range<usize> {
        self.input.read(cx).selected_range()
    }

    fn toggle_mark(&mut self, kind: MarkKind, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        if selection.is_empty() {
            let cursor = selection.start;
            let mut pending = self
                .pending_style
                .take()
                .filter(|pending| pending.at == cursor)
                .unwrap_or(TypingStyle {
                    at: cursor,
                    ..TypingStyle::default()
                });
            pending.toggle(kind, &self.draft.style_at(cursor));
            self.pending_style = (!pending.is_empty()).then_some(pending);
        } else {
            self.draft.toggle(selection, kind);
            self.draft.take_edits();
            self.record(StepKind::Other, cx);
            self.refresh_style(cx);
        }
        cx.notify();
    }

    fn toggle_lines(&mut self, kind: LineKind, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selection(cx);
        self.draft.toggle_lines(selection, kind);
        self.commit(None, window, cx);
        cx.notify();
    }

    fn press_format(&mut self, button: FormatButton, window: &mut Window, cx: &mut Context<Self>) {
        match button {
            FormatButton::Bold => self.toggle_mark(MarkKind::Bold, cx),
            FormatButton::Italic => self.toggle_mark(MarkKind::Italic, cx),
            FormatButton::Underline => self.toggle_mark(MarkKind::Underline, cx),
            FormatButton::Strike => self.toggle_mark(MarkKind::Strike, cx),
            FormatButton::Code => self.toggle_mark(MarkKind::Code, cx),
            FormatButton::Link => self.open_link_editor(window, cx),
            FormatButton::Bulleted => self.toggle_lines(LineKind::Bullet(0), window, cx),
            FormatButton::Numbered => self.toggle_lines(LineKind::Numbered(0), window, cx),
            FormatButton::Quote => self.toggle_lines(LineKind::Quote, window, cx),
        }
    }

    fn format_state(&self, button: FormatButton, selection: &Range<usize>) -> FormatState {
        let range = selection.clone();
        match button {
            FormatButton::Bold => self.draft.state(range, &MarkKind::Bold),
            FormatButton::Italic => self.draft.state(range, &MarkKind::Italic),
            FormatButton::Underline => self.draft.state(range, &MarkKind::Underline),
            FormatButton::Strike => self.draft.state(range, &MarkKind::Strike),
            FormatButton::Code => self.draft.state(range, &MarkKind::Code),
            FormatButton::Link => self.draft.state(range, &MarkKind::Link(String::new())),
            FormatButton::Bulleted => self.draft.line_state(range, &LineKind::Bullet(0)),
            FormatButton::Numbered => self.draft.line_state(range, &LineKind::Numbered(0)),
            FormatButton::Quote => self.draft.line_state(range, &LineKind::Quote),
        }
    }

    fn edit_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.link_editor.is_some() {
            return;
        }
        if self.draft.is_blank() && self.selection(cx).is_empty() {
            window.dispatch_action(Box::new(super::shell::OpenSwitcher), cx);
            return;
        }
        self.open_link_editor(window, cx);
    }

    fn open_link_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.selection(cx);
        let prefill = self
            .draft
            .link_in(range.clone())
            .or_else(|| {
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .map(|text| text.trim().to_owned())
                    .filter(|text| looks_like_url(text))
            })
            .unwrap_or_default();
        let field = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste or type a link")
                .default_value(prefill)
        });
        field.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        let edits = cx.subscribe(&field, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(editor) = this.link_editor.as_mut()
            {
                editor.refused = false;
                cx.notify();
            }
        });
        self.link_editor = Some(LinkEditor {
            range,
            field,
            refused: false,
            _edits: edits,
        });
        cx.notify();
    }

    fn apply_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.link_editor.take() else {
            return;
        };
        let typed = editor.field.read(cx).value().trim().to_owned();
        let url = if typed.is_empty() {
            String::new()
        } else if let Some(url) = link_url(&typed) {
            url
        } else {
            self.link_editor = Some(LinkEditor {
                refused: true,
                ..editor
            });
            cx.notify();
            return;
        };
        self.focus(window, cx);
        if editor.range.is_empty() {
            if !url.is_empty() {
                let end = self.draft.insert_link(editor.range.start, &url, &url);
                self.commit(Some(end), window, cx);
            }
        } else {
            self.draft.set_link(editor.range.clone(), &url);
            self.draft.take_edits();
            self.refresh_style(cx);
            self.input.update(cx, |state, cx| {
                state.set_selected_range(editor.range.clone(), cx)
            });
            self.record(StepKind::Other, cx);
            self.toolbar_dismissed = Some(editor.range);
        }
        cx.notify();
    }

    fn close_link_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.link_editor.take() else {
            return;
        };
        self.focus(window, cx);
        self.input
            .update(cx, |state, cx| state.set_selected_range(editor.range, cx));
        cx.notify();
    }

    fn break_line(&mut self, shift: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let selection = self.selection(cx);
        if !selection.is_empty() {
            return false;
        }
        match self.draft.break_line(selection.start, shift) {
            Some(cursor) => {
                self.commit(Some(cursor), window, cx);
                true
            }
            None => false,
        }
    }

    fn backspace_at_start(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let selection = self.selection(cx);
        if !selection.is_empty() {
            return false;
        }
        match self.draft.backspace_at_start(selection.start) {
            Some(cursor) => {
                self.commit(Some(cursor), window, cx);
                true
            }
            None => false,
        }
    }

    fn delete_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let selection = self.selection(cx);
        if !selection.is_empty() {
            return false;
        }
        match self.draft.delete_forward(selection.start) {
            Some(cursor) => {
                self.commit(Some(cursor), window, cx);
                true
            }
            None => false,
        }
    }

    fn indent(&mut self, outdent: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let selection = self.selection(cx);
        if !self.draft.indent(selection, outdent) {
            return false;
        }
        self.commit(None, window, cx);
        true
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let selection = self.selection(cx);
        if selection.is_empty() || self.draft.text().get(selection.clone()).is_none() {
            return false;
        }
        let text = self.draft.text()[selection.clone()].to_owned();
        let lines = self.draft.slice(selection);
        cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(
            text.clone(),
            lines.clone(),
        ));
        self.copied = Some((text, lines));
        true
    }

    fn copied_lines(&self, item: &ClipboardItem, text: &str) -> Option<Vec<DraftLine>> {
        item.entries()
            .iter()
            .find_map(|entry| match entry {
                ClipboardEntry::String(string) if string.text() == text => {
                    string.metadata_json::<Vec<DraftLine>>()
                }
                _ => None,
            })
            .or_else(|| {
                let normalized = text.replace("\r\n", "\n");
                self.copied
                    .as_ref()
                    .filter(|(copied, _)| *copied == normalized)
                    .map(|(_, lines)| lines.clone())
            })
    }

    fn toolbar_requested(&self, cx: &App) -> bool {
        let selection = self.selection(cx);
        !selection.is_empty()
            && !self.mouse_selecting
            && self.toolbar_dismissed.as_ref() != Some(&selection)
            && !self.popup_is_open()
    }

    fn remember_emoji(&mut self, glyph: &str, cx: &App) {
        self.recent_emoji.push(glyph);
        self.recent_emoji.save(&self.app.read(cx).store);
    }

    fn update_emoji(&mut self, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let found = emoji::active_query(&state.value(), state.cursor())
            .filter(|_| !self.draft.in_code(state.cursor()));
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
            let carry = self.carry_images_into.take().as_deref() == Some(conversation_id);
            self.conversation_id = Some(conversation_id.to_owned());
            self.cancel_uploads();
            let dropped = if carry {
                self.tray.keep_images_only()
            } else {
                self.tray.clear()
            };
            self.discard_uploaded(dropped, cx);
            self.reply = None;
            if self.editing.take().is_some() {
                self.load_draft(Draft::default(), InputContent::new(""), window, cx);
            }
            self.mention_inputs.clear();
            self.close_popup();
            cx.notify();
        }
    }

    /// The draft's images follow the user into this chat; any other switch clears the tray.
    pub fn carry_images_into(&mut self, conversation_id: String) {
        self.carry_images_into = Some(conversation_id);
    }

    fn discard_uploaded(&self, files: Vec<UploadedFile>, cx: &App) {
        discard_uploaded(self.app.read(cx).engine.clone(), files);
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
        if !self.can_send(cx) {
            return None;
        }
        let mut draft = self.current_draft(cx).trimmed();
        let attachments_apply = self.editing.is_none();
        if self.undone_value.as_deref() != Some(state.value().as_ref())
            && let Some((range, glyph)) = emoji::trailing_smiley(draft.text())
            && !draft.in_code(range.end - 1)
        {
            draft.replace(range, glyph);
            draft.take_edits();
        }
        let mentions = state
            .tokens()
            .iter()
            .filter_map(|span| self.mention_inputs.get(span.token().id().as_ref()))
            .cloned()
            .collect();
        Some(Outgoing {
            draft,
            mentions,
            reply: self.reply.clone(),
            edit: self.editing.clone(),
            images: if attachments_apply {
                self.tray.outgoing_images()
            } else {
                Vec::new()
            },
            files: if attachments_apply {
                self.tray.outgoing_files()
            } else {
                Vec::new()
            },
        })
    }

    /// The draft, or the plain input text while a change is still on its way to the draft.
    fn current_draft(&self, cx: &App) -> Draft {
        let value = self.input.read(cx).value();
        if self.draft.text() == value.as_ref() {
            self.draft.clone()
        } else {
            Draft::plain(&value)
        }
    }

    fn can_send(&self, cx: &App) -> bool {
        let has_text = !self.current_draft(cx).is_blank();
        if self.editing.is_some() {
            return has_text;
        }
        (has_text || !self.tray.is_empty()) && !self.tray.blocks_send()
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    fn files_allowed(&self) -> bool {
        self.conversation_id
            .as_deref()
            .is_some_and(|conversation_id| !conversation_id.is_empty())
    }

    fn cancel_uploads(&mut self) {
        for (_, handle) in self.uploads.drain() {
            handle.cancel();
        }
    }

    pub fn open_picker(&mut self, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                this.update(cx, |this, cx| this.add_paths(paths, cx)).ok();
            }
        })
        .detach();
    }

    pub fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.editing.is_some() || paths.is_empty() {
            return;
        }
        let ids = self.tray.add_pending(&names_of(&paths));
        for (id, path) in ids.into_iter().zip(paths) {
            cx.spawn(async move |this, cx| {
                let loaded = cx
                    .background_executor()
                    .spawn(async move { read_attachment(&path) })
                    .await;
                this.update(cx, |this, cx| this.finish_reading(id, loaded, cx))
                    .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn add_pasted_image(&mut self, pasted: Image, cx: &mut Context<Self>) {
        let ids = self.tray.add_pending(&[pasted_image_name()]);
        cx.notify();
        let Some(&id) = ids.first() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { prepare_pasted_image(pasted) })
                .await;
            this.update(cx, |this, cx| this.finish_reading(id, loaded, cx))
                .ok();
        })
        .detach();
    }

    fn paste(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.editing.is_none() {
            match paste_action(item) {
                PasteAction::Text => {}
                PasteAction::Files(paths) => {
                    self.add_paths(paths, cx);
                    return true;
                }
                PasteAction::Image(pasted) => {
                    self.add_pasted_image(pasted, cx);
                    return true;
                }
            }
        }
        let Some(text) = item.text() else {
            return false;
        };
        if self.draft.in_code(self.selection(cx).start) {
            return false;
        }
        if let Some(lines) = self.copied_lines(item, &text) {
            let cursor = self.draft.insert_lines(self.selection(cx), &lines);
            self.commit(Some(cursor), window, cx);
            return true;
        }
        self.pasted_markdown = has_markdown(&text).then_some(text);
        false
    }

    fn finish_reading(
        &mut self,
        id: u64,
        loaded: Result<LoadedFile, String>,
        cx: &mut Context<Self>,
    ) {
        match loaded {
            Ok(loaded) => {
                let files_allowed = self.files_allowed();
                if let Some(job) = self.tray.finish_reading(id, loaded, files_allowed) {
                    self.start_upload(job, cx);
                }
            }
            Err(message) => self.tray.fail_reading(id, message),
        }
        cx.notify();
    }

    fn start_upload(&mut self, job: UploadJob, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.conversation_id.clone().filter(|id| !id.is_empty()) else {
            self.tray.finish_upload(job.id, UploadResult::Failed);
            return;
        };
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        if demo {
            self.simulate_upload(job, cx);
            return;
        }
        let Some(engine) = engine else {
            self.tray.finish_upload(job.id, UploadResult::Failed);
            return;
        };
        let id = job.id;
        let (event_sender, mut event_receiver) =
            tokio::sync::mpsc::unbounded_channel::<UploadEvent>();
        let finished_sender = event_sender.clone();
        let (_, handle) = runtime::spawn_abortable(async move {
            let result = async {
                let uploaded = match job.kind {
                    JobKind::Upload => {
                        let progress_sender = event_sender.clone();
                        let uploaded = engine
                            .upload_attachment_file(
                                &conversation_id,
                                &job.name,
                                &job.bytes,
                                move |percent| {
                                    let _ = progress_sender.send(UploadEvent::Progress(percent));
                                },
                            )
                            .await;
                        match uploaded {
                            Ok(uploaded) => {
                                let _ = event_sender.send(UploadEvent::Uploaded(uploaded.clone()));
                                uploaded
                            }
                            Err(_) => return UploadResult::Failed,
                        }
                    }
                    JobKind::Share(uploaded) => uploaded,
                };
                let Some(reference) = uploaded.reference() else {
                    return UploadResult::Failed;
                };
                match engine.share_attachment(&conversation_id, &uploaded).await {
                    Ok(()) => UploadResult::Done(DoneFile {
                        reference,
                        uploaded: Some(uploaded),
                    }),
                    Err(_) => UploadResult::ShareFailed(uploaded),
                }
            }
            .await;
            let _ = finished_sender.send(UploadEvent::Finished(result));
        });
        self.uploads.insert(id, UploadHandle::Network(handle));
        cx.spawn(async move |this, cx| {
            while let Some(event) = event_receiver.recv().await {
                let updated = this.update(cx, |this, cx| {
                    let orphan = match event {
                        UploadEvent::Progress(percent) => {
                            this.tray.set_progress(id, percent);
                            None
                        }
                        UploadEvent::Uploaded(uploaded) => this.tray.mark_uploaded(id, uploaded),
                        UploadEvent::Finished(result) => this.finish_upload(id, result),
                    };
                    this.discard_uploaded(orphan.into_iter().collect(), cx);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn simulate_upload(&mut self, job: UploadJob, cx: &mut Context<Self>) {
        let id = job.id;
        let fails = job.name.to_lowercase().contains("fail") && self.demo_failed.insert(id);
        let task = cx.spawn(async move |this, cx| {
            for step in 1..=DEMO_UPLOAD_STEPS {
                cx.background_executor().timer(DEMO_UPLOAD_STEP).await;
                if fails && step == DEMO_FAILURE_STEP {
                    this.update(cx, |this, cx| {
                        this.finish_upload_in_demo(id, UploadResult::Failed, cx)
                    })
                    .ok();
                    return;
                }
                let percent = (u32::from(step) * 100 / u32::from(DEMO_UPLOAD_STEPS)) as u8;
                this.update(cx, |this, cx| this.set_progress(id, percent, cx))
                    .ok();
            }
            let reference = FileReference {
                attachment_id: format!("demo-attachment-{id}"),
                content_url: format!("https://demo.invalid/files/{}", job.name),
                name: job.name,
            };
            let done = DoneFile {
                reference,
                uploaded: None,
            };
            this.update(cx, |this, cx| {
                this.finish_upload_in_demo(id, UploadResult::Done(done), cx)
            })
            .ok();
        });
        self.uploads.insert(id, UploadHandle::Demo(task));
    }

    fn set_progress(&mut self, id: u64, percent: u8, cx: &mut Context<Self>) {
        self.tray.set_progress(id, percent);
        cx.notify();
    }

    fn finish_upload(&mut self, id: u64, result: UploadResult) -> Option<UploadedFile> {
        self.uploads.remove(&id);
        self.tray.finish_upload(id, result)
    }

    fn finish_upload_in_demo(&mut self, id: u64, result: UploadResult, cx: &mut Context<Self>) {
        self.finish_upload(id, result);
        cx.notify();
    }

    fn remove_attachment(&mut self, id: u64, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.uploads.remove(&id) {
            handle.cancel();
        }
        let uploaded = self.tray.remove(id);
        self.discard_uploaded(uploaded.into_iter().collect(), cx);
        cx.notify();
    }

    fn retry_attachment(&mut self, id: u64, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(job) = self.tray.retry(id) {
            self.start_upload(job, cx);
        }
        cx.notify();
    }

    pub fn submit_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(outgoing) = self.outgoing(cx) else {
            return;
        };
        self.load_draft(Draft::default(), InputContent::new(""), window, cx);
        self.mention_inputs.clear();
        if self.editing.is_none() {
            self.tray.clear();
        }
        self.reply = None;
        self.editing = None;
        self.close_popup();
        cx.emit(ComposerEvent::Submit(Box::new(outgoing)));
        cx.notify();
    }

    pub fn restore(&mut self, outgoing: &Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        self.mention_inputs.clear();
        let text = outgoing.draft.text();
        let mut content = InputContent::new(text.to_owned());
        let mut cursor = 0;
        for mention in &outgoing.mentions {
            let needle = format!("@{}", mention.text);
            let Some(offset) = text[cursor..].find(&needle) else {
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
        self.load_draft(outgoing.draft.clone(), content, window, cx);
        self.reply = outgoing.reply.clone();
        self.editing = outgoing.edit.clone();
        if outgoing.edit.is_none() {
            self.cancel_uploads();
            self.tray.restore(&outgoing.images, &outgoing.files);
        }
        self.close_popup();
        cx.notify();
    }

    /// Markdown in `text` arrives formatted.
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let draft = Draft::from_markdown(text);
        let content = InputContent::new(draft.text().to_owned());
        self.load_draft(draft, content, window, cx);
        self.mention_inputs.clear();
        self.update_emoji(cx);
        self.update_mention(cx);
        cx.notify();
    }

    pub fn is_empty(&self, _: &App) -> bool {
        self.draft.is_blank() && self.tray.is_empty()
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

    fn render_toolbar(&self, focused: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let composer = cx.entity().downgrade();
        if self.link_editor.is_none() && !(focused && self.toolbar_requested(cx)) {
            return None;
        }
        let state = self.input.read(cx);
        let range = self
            .link_editor
            .as_ref()
            .map_or_else(|| state.selected_range(), |editor| editor.range.clone());
        let start = state.range_to_bounds(&(range.start..range.start))?;
        let end = state.range_to_bounds(&(range.end..range.end))?;
        let input_bounds = state.input_bounds();
        let center_x = if start.origin.y == end.origin.y {
            (start.origin.x + end.origin.x) / 2.
        } else {
            input_bounds.center().x
        };
        let edges = self
            .composer_bounds
            .get()
            .map_or((input_bounds.left(), input_bounds.right()), |bounds| {
                (bounds.left(), bounds.right())
            });
        let width = match self.link_editor {
            Some(_) => format_toolbar::link_width(),
            None => format_toolbar::bar_width(),
        };
        let placement = format_toolbar::place(center_x, start.origin.y, edges, width);
        let element = match &self.link_editor {
            Some(editor) => format_toolbar::render(
                placement,
                format_toolbar::Mode::Link {
                    field: &editor.field,
                    refused: editor.refused,
                    on_apply: Rc::new(move |window, cx| {
                        composer
                            .update(cx, |this, cx| this.apply_link(window, cx))
                            .ok();
                    }),
                },
            ),
            None => {
                let state_of = |button| self.format_state(button, &range);
                format_toolbar::render(
                    placement,
                    format_toolbar::Mode::Bar {
                        state: &state_of,
                        on_press: Rc::new(move |button, window, cx| {
                            composer
                                .update(cx, |this, cx| this.press_format(button, window, cx))
                                .ok();
                        }),
                    },
                )
            }
        };
        Some(element)
    }

    fn render_paste_hint(&self) -> Option<Div> {
        self.paste_hint.as_ref()?;
        Some(
            h_flex()
                .mt(px(6.))
                .gap(px(6.))
                .items_center()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child("Formatted from Markdown")
                .child(
                    div()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(theme::border_strong())
                        .bg(theme::surface_raised())
                        .text_size(px(11.))
                        .text_color(theme::text())
                        .child("Ctrl+Z"),
                )
                .child("plain text"),
        )
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
        let can_send = self.can_send(cx);
        let can_attach = self.editing.is_none();
        let send = div()
            .id("composer-send")
            .size(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .child(symbol("keyboard_return", 20., white()))
            .when(!can_send, |button| button.opacity(0.4))
            .when(can_send, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(white().opacity(0.1)))
                    .on_click(cx.listener(|this, _, window, cx| this.submit_current(window, cx)))
            });
        let attach = can_attach.then(|| {
            div()
                .id("composer-attach")
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|button| button.bg(theme::row_hover()))
                .tooltip(|window, cx| Tooltip::new("Attach file").build(window, cx))
                .child(symbol("attach_file", 20., theme::text_muted()))
                .on_click(cx.listener(|this, _, _, cx| this.open_picker(cx)))
        });
        let composer = cx.weak_entity();
        let input = Textarea::new(&self.input)
            .appearance(false)
            .bordered(false)
            .on_paste(move |item, window, cx| {
                composer
                    .update(cx, |this, cx| this.paste(item, window, cx))
                    .unwrap_or(false)
            })
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
        let tray = can_attach
            .then(|| {
                render_tray(
                    &self.tray,
                    Self::remove_attachment,
                    Self::retry_attachment,
                    cx,
                )
            })
            .flatten();
        let notice = self.tray.notice().filter(|_| can_attach).map(|notice| {
            div()
                .w_full()
                .mb(px(6.))
                .text_size(px(12.))
                .text_color(theme::amber())
                .child(notice.to_owned())
        });
        let composer_bounds = self.composer_bounds.clone();
        v_flex()
            .w_full()
            .flex_none()
            .px(px(24.))
            .pt(px(8.))
            .pb(px(12.))
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|this, _: &ToggleBold, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Bold, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleItalic, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Italic, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleUnderline, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Underline, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleStrike, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Strike, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleCode, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Code, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &EditLink, window, cx| this.edit_link(window, cx)))
            .capture_action(cx.listener(|this, _: &Undo, window, cx| {
                if this.link_editor.is_none() {
                    this.step_history(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Redo, window, cx| {
                if this.link_editor.is_none() {
                    this.step_history(false, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Copy, _, cx| {
                if this.link_editor.is_none() && this.copy_selection(cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Cut, window, cx| {
                if this.link_editor.is_none() && this.copy_selection(cx) {
                    this.input
                        .update(cx, |state, cx| state.replace("", window, cx));
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                if this.link_editor.is_some() {
                } else if this.popup_is_open() {
                    this.move_highlight(-1, cx);
                    cx.stop_propagation();
                } else if this.editing.is_none() && this.is_empty(cx) {
                    cx.emit(ComposerEvent::EditLast);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                if this.link_editor.is_none() && this.popup_is_open() {
                    this.move_highlight(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                if this.link_editor.is_some() {
                    this.apply_link(window, cx);
                    cx.stop_propagation();
                } else if this.popup_is_open() {
                    this.accept_popup(window, cx);
                    cx.stop_propagation();
                } else if this.break_line(action.shift, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if this.link_editor.is_some() {
                } else if this.popup_is_open() {
                    this.accept_popup(window, cx);
                    cx.stop_propagation();
                } else if this.indent(false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &OutdentInline, window, cx| {
                if this.link_editor.is_none() && this.indent(true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Backspace, window, cx| {
                if this.link_editor.is_none()
                    && (this.undo_conversion(window, cx) || this.backspace_at_start(window, cx))
                {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Delete, window, cx| {
                if this.link_editor.is_none() && this.delete_forward(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.link_editor.is_some() {
                    this.close_link_editor(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.toolbar_requested(cx) {
                    this.toolbar_dismissed = Some(this.selection(cx));
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
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
            .children(notice)
            .children(self.render_reply_strip(cx))
            .children(self.render_edit_strip(cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .children(self.render_popup(cx))
                    .children(self.render_emoji_popup(window, cx))
                    .children(self.render_toolbar(focused, cx))
                    .child(
                        v_flex()
                            .relative()
                            .w_full()
                            .rounded(px(10.))
                            .bg(theme::surface())
                            .border_1()
                            .border_color(if focused {
                                theme::accent()
                            } else {
                                theme::border_strong()
                            })
                            .child(
                                canvas(
                                    move |bounds, _, _| composer_bounds.set(Some(bounds)),
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .children(tray)
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_end()
                                    .gap(px(6.))
                                    .py(px(6.))
                                    .pl(px(if can_attach { 6. } else { 12. }))
                                    .pr(px(6.))
                                    .children(attach)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .capture_any_mouse_down(cx.listener(
                                                |this, _: &MouseDownEvent, _, _| {
                                                    this.mouse_selecting = true;
                                                    this.toolbar_dismissed = None;
                                                    this.link_editor = None;
                                                },
                                            ))
                                            .capture_any_mouse_up(cx.listener(
                                                |this, _: &MouseUpEvent, _, cx| {
                                                    this.mouse_selecting = false;
                                                    cx.notify();
                                                },
                                            ))
                                            .on_mouse_up_out(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.mouse_selecting = false;
                                                    cx.notify();
                                                }),
                                            )
                                            .child(input),
                                    )
                                    .child(send),
                            ),
                    ),
            )
            .children(self.render_paste_hint())
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

    #[test]
    fn extras_carry_image_bytes_with_their_mime_type_and_the_file_references() {
        use std::sync::Arc;

        use gpui_kit::{Image, ImageFormat};
        use teams_core::FileKind;

        use super::{Outgoing, OutgoingFile, OutgoingImage};

        let outgoing = Outgoing {
            draft: teams_core::Draft::default(),
            mentions: Vec::new(),
            reply: None,
            edit: None,
            images: vec![OutgoingImage {
                name: "pasted-image.png".into(),
                image: Arc::new(Image::from_bytes(ImageFormat::Png, vec![1, 2, 3])),
                dimensions: Some((1, 1)),
            }],
            files: vec![OutgoingFile {
                name: "plan.pdf".into(),
                size: 9,
                kind: FileKind::Pdf,
                uploaded: None,
                reference: teams_core::FileReference {
                    attachment_id: "guid".into(),
                    content_url: "https://files.example/plan.pdf".into(),
                    name: "plan.pdf".into(),
                },
            }],
        };
        let extras = outgoing.extras();
        assert_eq!(extras.images[0].content_type, "image/png");
        assert_eq!(extras.images[0].bytes.as_slice(), [1, 2, 3]);
        assert_eq!(extras.files[0].attachment_id, "guid");
        assert!(outgoing.has_attachments());
    }

    #[test]
    fn enter_sends_attachments_even_without_text() {
        assert!(super::is_plain_enter(&press(false)));
        assert!(!super::is_plain_enter(&press(true)));
        assert!(!super::is_plain_enter(&InputEvent::Change));
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

        struct Typist<'a> {
            cx: &'a mut TestAppContext,
            handle: gpui_kit::AnyWindowHandle,
            composer: gpui_kit::Entity<Composer>,
        }

        impl Typist<'_> {
            fn act(&mut self, action: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App)) {
                self.cx
                    .update_window(self.handle, |_, window, cx| action(window, cx))
                    .unwrap();
                self.cx.run_until_parked();
            }

            fn type_text(&mut self, text: &str) {
                for character in text.chars() {
                    self.act(|window, cx| window.input(&character.to_string(), cx));
                }
            }

            fn press(&mut self, keys: &str) {
                for key in keys.split(' ') {
                    self.act(|window, cx| window.press(key, cx));
                }
            }

            fn value(&mut self) -> String {
                let composer = self.composer.clone();
                self.cx
                    .update(|cx| composer.read(cx).input.read(cx).value().to_string())
            }

            fn draft(&mut self) -> teams_core::Draft {
                let composer = self.composer.clone();
                self.cx.update(|cx| composer.read(cx).draft.clone())
            }

            fn outgoing(&mut self) -> super::super::Outgoing {
                let composer = self.composer.clone();
                self.cx
                    .update(|cx| composer.read(cx).outgoing(cx))
                    .expect("text to send")
            }
        }

        fn typist(cx: &mut TestAppContext) -> Typist<'_> {
            let (handle, composer) = demo_composer(cx);
            cx.update(super::super::bind_keys);
            let mut typist = Typist {
                cx,
                handle,
                composer: composer.clone(),
            };
            typist.act(|window, cx| {
                composer.update(cx, |composer, cx| composer.focus(window, cx));
                window.render_frame(cx);
            });
            typist
        }

        #[gpui_kit::test]
        fn typed_markdown_is_formatted_and_backspace_brings_it_back(cx: &mut TestAppContext) {
            use teams_core::{FormatState, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("a **bold**");
            assert_eq!(typist.value(), "a bold");
            assert_eq!(typist.draft().state(2..6, &MarkKind::Bold), FormatState::On);
            typist.press("backspace");
            assert_eq!(typist.value(), "a **bold**");
            assert!(typist.draft().marks().is_empty());
            let composer = typist.composer.clone();
            assert_eq!(
                typist
                    .cx
                    .update(|cx| composer.read(cx).input.read(cx).cursor()),
                10
            );
            typist.press("backspace");
            typist.type_text("* x");
            assert_eq!(typist.value(), "a bold x");
            assert_eq!(
                typist.draft().state(6..8, &MarkKind::Bold),
                FormatState::Off
            );
        }

        #[gpui_kit::test]
        fn enter_continues_a_list_instead_of_sending(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("1. one");
            typist.press("enter");
            typist.type_text("two");
            assert_eq!(typist.value(), "1. one\n2. two");
            typist.press("enter enter");
            assert_eq!(typist.value(), "1. one\n2. two\n");
            typist.type_text("after");
            typist.press("enter");
            assert_eq!(typist.value(), "");
        }

        #[gpui_kit::test]
        fn tab_nests_a_list_item(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("- a");
            typist.press("enter tab");
            typist.type_text("b");
            assert_eq!(typist.value(), "• a\n◦ b");
            typist.press("shift-tab");
            assert_eq!(typist.value(), "• a\n• b");
        }

        #[gpui_kit::test]
        fn ctrl_b_toggles_bold_on_a_selection_and_for_the_next_text(cx: &mut TestAppContext) {
            use teams_core::{FormatState, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("hello");
            typist.press("shift-left shift-left ctrl-b");
            assert_eq!(typist.draft().state(3..5, &MarkKind::Bold), FormatState::On);
            assert_eq!(
                typist.draft().state(0..5, &MarkKind::Bold),
                FormatState::Mixed
            );
            typist.press("ctrl-b");
            assert!(typist.draft().marks().is_empty());
            typist.press("end ctrl-i");
            typist.type_text("!");
            assert_eq!(typist.outgoing().html(), "hello<i>!</i>");
        }

        #[gpui_kit::test]
        fn pasted_markdown_converts_and_ctrl_z_restores_the_raw_text(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist
                .cx
                .write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                    "**hi** there".to_owned(),
                ));
            typist.press("ctrl-v");
            assert_eq!(typist.value(), "hi there");
            let composer = typist.composer.clone();
            assert!(
                typist
                    .cx
                    .update(|cx| composer.read(cx).paste_hint.is_some())
            );
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "**hi** there");
            assert!(typist.draft().marks().is_empty());
        }

        #[gpui_kit::test]
        fn copy_and_paste_inside_the_composer_keeps_formatting(cx: &mut TestAppContext) {
            use teams_core::{FormatState, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("`code` ");
            typist.press("ctrl-a ctrl-c end ctrl-v");
            assert_eq!(typist.value(), "code code ");
            assert_eq!(typist.draft().state(5..9, &MarkKind::Code), FormatState::On);
        }

        #[gpui_kit::test]
        fn undo_brings_the_formatting_back_with_the_text(cx: &mut TestAppContext) {
            use teams_core::{FormatState, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("~~old~~");
            typist.press("ctrl-a");
            typist.type_text("new");
            typist.press("ctrl-z ctrl-z");
            assert_eq!(typist.value(), "old");
            assert_eq!(
                typist.draft().state(0..3, &MarkKind::Strike),
                FormatState::On
            );
        }

        #[gpui_kit::test]
        fn ctrl_k_turns_the_selection_into_a_link(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("see docs");
            typist.press("shift-left shift-left shift-left shift-left ctrl-k");
            let composer = typist.composer.clone();
            assert!(
                typist
                    .cx
                    .update(|cx| composer.read(cx).link_editor.is_some())
            );
            typist.type_text("example.com");
            typist.press("enter");
            assert!(
                typist
                    .cx
                    .update(|cx| composer.read(cx).link_editor.is_none())
            );
            assert_eq!(
                typist.outgoing().html(),
                "see <a href=\"https://example.com\">docs</a>"
            );
        }

        #[gpui_kit::test]
        fn ctrl_z_takes_back_bold_before_the_typing_and_ctrl_y_redoes_it(cx: &mut TestAppContext) {
            use teams_core::{FormatState, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("hello");
            typist.press("shift-left shift-left ctrl-b");
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "hello");
            assert!(typist.draft().marks().is_empty());
            typist.press("ctrl-y");
            assert_eq!(typist.draft().state(3..5, &MarkKind::Bold), FormatState::On);
        }

        #[gpui_kit::test]
        fn undo_after_typing_undoes_the_typing_not_an_older_bold(cx: &mut TestAppContext) {
            use teams_core::{FormatState, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("hello");
            typist.press("shift-left shift-left ctrl-b end");
            typist.type_text(" x");
            typist.press("backspace backspace");
            assert_eq!(typist.value(), "hello");
            typist.press("ctrl-z");
            assert_ne!(typist.value(), "hello");
            assert_eq!(typist.draft().state(3..5, &MarkKind::Bold), FormatState::On);
        }

        #[gpui_kit::test]
        fn delete_at_a_code_line_end_joins_without_padding(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("```");
            typist.press("enter");
            typist.type_text("a");
            typist.press("enter");
            typist.type_text("b");
            typist.press("up end delete");
            assert_eq!(typist.outgoing().html(), "<pre>ab</pre>");
        }

        #[gpui_kit::test]
        fn copied_code_has_no_padding_in_the_plain_text(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("```");
            typist.press("enter");
            typist.type_text("  x");
            typist.press("ctrl-a ctrl-c");
            let text = typist.cx.read_from_clipboard().and_then(|item| item.text());
            assert_eq!(text.as_deref(), Some("  x"));
        }

        #[gpui_kit::test]
        fn a_refused_link_keeps_the_field_open_and_says_why(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("docs");
            typist.press("ctrl-a ctrl-k");
            typist.type_text("javascript:alert(1)");
            typist.press("enter");
            let composer = typist.composer.clone();
            assert!(typist.cx.update(|cx| {
                composer
                    .read(cx)
                    .link_editor
                    .as_ref()
                    .is_some_and(|editor| editor.refused)
            }));
            assert!(typist.draft().marks().is_empty());
            typist.type_text("x");
            assert!(typist.cx.update(|cx| {
                composer
                    .read(cx)
                    .link_editor
                    .as_ref()
                    .is_some_and(|editor| !editor.refused)
            }));
        }

        #[gpui_kit::test]
        fn undo_takes_back_a_list_toggle_then_the_bold_before_it(cx: &mut TestAppContext) {
            use teams_core::{FormatState, LineKind, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("hello");
            typist.press("ctrl-a ctrl-b");
            let composer = typist.composer.clone();
            typist.act(|window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.press_format(super::super::FormatButton::Bulleted, window, cx)
                })
            });
            assert_eq!(typist.value(), "• hello");
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "hello");
            assert_eq!(typist.draft().state(0..5, &MarkKind::Bold), FormatState::On);
            typist.press("ctrl-z");
            assert!(typist.draft().marks().is_empty());
            assert_eq!(typist.draft().lines(), [LineKind::Text]);
        }

        #[gpui_kit::test]
        fn redo_brings_back_typing_and_bold_in_order(cx: &mut TestAppContext) {
            use teams_core::{FormatState, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("hello");
            typist.press("ctrl-a ctrl-b ctrl-z ctrl-z");
            assert_eq!(typist.value(), "");
            typist.press("ctrl-y");
            assert_eq!(typist.value(), "hello");
            assert!(typist.draft().marks().is_empty());
            typist.press("ctrl-y");
            assert_eq!(typist.draft().state(0..5, &MarkKind::Bold), FormatState::On);
        }

        #[gpui_kit::test]
        fn an_undo_with_nothing_to_undo_leaves_conversions_working(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.press("ctrl-z");
            typist.type_text("**a**");
            assert_eq!(typist.value(), "a");
        }

        #[gpui_kit::test]
        fn code_blocks_take_markdown_and_smileys_literally(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("```");
            typist.press("enter");
            typist.type_text(":) ");
            typist
                .cx
                .write_to_clipboard(gpui_kit::ClipboardItem::new_string("**x**".to_owned()));
            typist.press("ctrl-v");
            assert_eq!(typist.value(), ":) **x**");
            let composer = typist.composer.clone();
            assert!(
                typist
                    .cx
                    .update(|cx| composer.read(cx).paste_hint.is_none())
            );
            let outgoing = typist.outgoing();
            assert_eq!(outgoing.html(), "<pre>:) **x**</pre>");
        }

        #[gpui_kit::test]
        fn quotes_continue_on_shift_enter_and_send_as_blockquote(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("> quoted");
            typist.press("shift-enter");
            typist.type_text("more");
            let outgoing = typist.outgoing();
            assert_eq!(outgoing.html(), "<blockquote>quoted<br>more</blockquote>");
            assert_eq!(outgoing.text(), "quoted\nmore");
        }

        #[gpui_kit::test]
        fn demo_upload_runs_to_done_and_a_file_alone_can_be_sent(cx: &mut TestAppContext) {
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
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("plan.pdf");
            std::fs::write(&path, b"%PDF-1.7").unwrap();
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_conversation("demo-chat", "Demo", window, cx);
                    composer.add_paths(vec![path], cx);
                });
            })
            .unwrap();
            cx.run_until_parked();
            cx.update(|cx| {
                let composer = composer.read(cx);
                assert!(composer.tray.blocks_send());
                assert!(!composer.can_send(cx));
            });
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(3));
            cx.run_until_parked();
            cx.update(|cx| {
                let composer = composer.read(cx);
                assert!(composer.can_send(cx));
                let outgoing = composer
                    .outgoing(cx)
                    .expect("an attachment alone is a message");
                assert_eq!(outgoing.text(), "");
                assert_eq!(outgoing.files.len(), 1);
                assert_eq!(outgoing.files[0].name, "plan.pdf");
            });
        }

        #[gpui_kit::test]
        fn attachments_are_dropped_when_the_conversation_changes(cx: &mut TestAppContext) {
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
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("plan.pdf");
            std::fs::write(&path, b"%PDF-1.7").unwrap();
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_conversation("demo-chat", "Demo", window, cx);
                    composer.add_paths(vec![path], cx);
                });
            })
            .unwrap();
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_conversation("other-chat", "Other", window, cx)
                });
            })
            .unwrap();
            cx.update(|cx| assert!(composer.read(cx).tray.is_empty()));
        }

        fn demo_composer(
            cx: &mut TestAppContext,
        ) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<Composer>) {
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
            (handle, composer)
        }

        fn small_png(directory: &std::path::Path) -> std::path::PathBuf {
            let path = directory.join("dot.png");
            image::RgbaImage::new(2, 2).save(&path).unwrap();
            path
        }

        #[gpui_kit::test]
        fn editing_keeps_the_attachments_and_hides_them_from_the_edit(cx: &mut TestAppContext) {
            use super::super::{EditPreview, Outgoing};

            let (handle, composer) = demo_composer(cx);
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("plan.pdf");
            std::fs::write(&path, b"%PDF-1.7").unwrap();
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_conversation("demo-chat", "Demo", window, cx);
                    composer.add_paths(vec![path], cx);
                });
            })
            .unwrap();
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(3));
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    let draft = Outgoing {
                        draft: teams_core::Draft::plain("old"),
                        mentions: Vec::new(),
                        reply: None,
                        edit: Some(EditPreview {
                            message_id: "m1".into(),
                            excerpt: "old".into(),
                        }),
                        images: Vec::new(),
                        files: Vec::new(),
                    };
                    composer.begin_edit(draft, window, cx);
                });
            })
            .unwrap();
            cx.update(|cx| {
                let composer = composer.read(cx);
                assert_eq!(composer.tray.items().len(), 1);
                let outgoing = composer.outgoing(cx).unwrap();
                assert!(outgoing.files.is_empty());
                assert_eq!(outgoing.text(), "old");
            });
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| composer.cancel_edit(window, cx));
            })
            .unwrap();
            cx.update(|cx| {
                let composer = composer.read(cx);
                assert_eq!(composer.tray.items().len(), 1);
                assert!(composer.can_send(cx));
            });
        }

        #[gpui_kit::test]
        fn draft_images_follow_only_into_the_chat_that_was_chosen(cx: &mut TestAppContext) {
            let (handle, composer) = demo_composer(cx);
            let directory = tempfile::tempdir().unwrap();
            let path = small_png(directory.path());
            for (carry, chosen, expected) in [
                (Some("chat-a"), "chat-a", 1),
                (None, "chat-b", 0),
                (Some("chat-a"), "chat-b", 0),
            ] {
                cx.update_window(handle, |_, window, cx| {
                    composer.update(cx, |composer, cx| {
                        composer.set_conversation("", "", window, cx);
                        composer.add_paths(vec![path.clone()], cx);
                    });
                })
                .unwrap();
                cx.run_until_parked();
                cx.update_window(handle, |_, window, cx| {
                    composer.update(cx, |composer, cx| {
                        if let Some(chat) = carry {
                            composer.carry_images_into(chat.to_owned());
                        }
                        composer.set_conversation(chosen, "Chat", window, cx);
                    });
                })
                .unwrap();
                cx.update(|cx| assert_eq!(composer.read(cx).tray.items().len(), expected));
            }
        }

        #[gpui_kit::test]
        fn a_draft_target_change_drops_the_files_of_the_old_target_and_keeps_images(
            cx: &mut TestAppContext,
        ) {
            let (handle, composer) = demo_composer(cx);
            let directory = tempfile::tempdir().unwrap();
            let png = small_png(directory.path());
            let pdf = directory.path().join("plan.pdf");
            std::fs::write(&pdf, b"%PDF-1.7").unwrap();
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_conversation("bob-chat", "Bob", window, cx);
                    composer.add_paths(vec![png, pdf], cx);
                });
            })
            .unwrap();
            cx.run_until_parked();
            cx.update(|cx| assert_eq!(composer.read(cx).tray.items().len(), 2));
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.carry_images_into(String::new());
                    composer.set_conversation("", "", window, cx);
                });
            })
            .unwrap();
            cx.update(|cx| {
                let composer = composer.read(cx);
                assert_eq!(composer.tray.items().len(), 1);
                assert_eq!(composer.tray.items()[0].name, "dot.png");
                assert!(!composer.files_allowed());
            });
        }
    }
}
