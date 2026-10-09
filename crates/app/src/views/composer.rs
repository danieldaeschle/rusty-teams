use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{
        Backspace, Copy, Cut, Delete, Enter, Escape, IndentInline, InlineToken, InputContent,
        InputEvent, InputState, MoveDown, MoveUp, OutdentInline, RangeDecorationCollection, Redo,
        TextDecorationCollection, Textarea, TextareaMode, TextareaState, Undo,
    },
    popover::Popover,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use teams_core::{
    Draft, DraftLine, Edit, FileReference, FormatState, HostedImage, LineKind, MarkKind,
    LinkPreview, MentionCandidate, MentionInput, MessageExtras, OBJECT_MARK, SizeStep, TypingStyle,
    UploadedFile, changed_span, has_markdown, link_url, map_offset, reverse_edits,
};

mod drafts;
mod schedule;

use super::attachment_tray::{
    AttachmentTray, DoneFile, JobKind, LoadedFile, MAX_ATTACHMENTS, OutgoingFile, OutgoingImage,
    PasteAction, UploadJob, UploadResult, discard_uploaded, inline_preview, names_of, paste_action,
    pasted_image_name, prepare_pasted_image, read_attachment, render_tray,
};
use super::avatar::{person_avatar, square_avatar};
use super::draft_style::draft_style;
use super::emoji_popup::{self, EmojiPopup};
use super::format_toolbar::{self, FormatButton};
use super::link_preview::{ComposeLink, draft_link, link_preview_card};
use super::fun_picker::{FunPicker, FunPickerEvent};
use super::widgets::{icon, symbol};
use crate::app_state::AppState;
use crate::emoji;
use crate::remote_image::RemoteImage;
use crate::runtime;
use crate::scheduled_rows::scheduled_label;
use crate::theme;
use crate::typing::OutgoingTyping;

const MIN_ROWS: usize = 1;
const MAX_ROWS: usize = 8;
const MENTION_DEBOUNCE: Duration = Duration::from_millis(150);
const MENTION_LIMIT: usize = 8;
const LINK_PREVIEW_DEBOUNCE: Duration = Duration::from_millis(600);
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
const IMAGE_TOKEN_PREFIX: &str = "image-";

actions!(
    composer,
    [
        ToggleBold,
        ToggleItalic,
        ToggleUnderline,
        ToggleStrike,
        ToggleSuperscript,
        ToggleSubscript,
        ToggleCode,
        EditLink,
        ScheduleSend
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-b", ToggleBold, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-i", ToggleItalic, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-u", ToggleUnderline, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-x", ToggleStrike, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-=", ToggleSuperscript, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-=", ToggleSubscript, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-c", ToggleCode, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-k", EditLink, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-enter", ScheduleSend, Some(KEY_CONTEXT)),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyPreview {
    pub message_id: String,
    pub author: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPreview {
    pub message_id: String,
    pub excerpt: String,
    pub scheduled: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub draft: Draft,
    pub mentions: Vec<MentionInput>,
    pub reply: Option<ReplyPreview>,
    pub edit: Option<EditPreview>,
    pub images: Vec<OutgoingImage>,
    pub files: Vec<OutgoingFile>,
    pub link_preview: Option<LinkPreview>,
}

impl Outgoing {
    /// Without list and quote markers, for previews.
    pub fn text(&self) -> String {
        self.draft.plain_text()
    }

    /// The body before mentions become `<at>` tags.
    pub fn html(&self) -> String {
        let mut html = String::new();
        let mut objects = self.images.iter();
        let mut uploaded = 0;
        for character in self.draft.to_html().chars() {
            if character != OBJECT_MARK {
                html.push(character);
                continue;
            }
            match objects.next() {
                Some(OutgoingImage::Inline(_)) => {
                    uploaded += 1;
                    html.push_str(&format!("<img src=\"../hostedContents/{uploaded}/$value\">"));
                }
                Some(OutgoingImage::Remote(remote)) => html.push_str(&remote.html()),
                None => {}
            }
        }
        html
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
                .filter_map(|outgoing| match outgoing {
                    OutgoingImage::Inline(inline) => Some(HostedImage {
                        content_type: inline.image.format.mime_type().to_owned(),
                        bytes: Arc::new(inline.image.bytes.clone()),
                    }),
                    OutgoingImage::Remote(_) => None,
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
    Schedule {
        outgoing: Box<Outgoing>,
        send_at: DateTime<Utc>,
    },
    EditLast,
    Typing(bool),
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
    Typing { at: usize, after_space: bool },
    Deleting { start: usize, end: usize },
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
    typing: OutgoingTyping,
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
    draft_save: Option<Task<()>>,
    saved_images: Option<drafts::ImageFingerprint>,
    schedule: Option<schedule::SchedulePopover>,
    scheduling: Option<String>,
    pending_anchors: HashMap<u64, usize>,
    link: ComposeLink,
    fun_picker: Entity<FunPicker>,
    _subscriptions: [Subscription; 4],
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
            at: cursor - typed.len_utf8(),
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
        StepKind::Deleting {
            start: cursor,
            end: cursor + removed,
        }
    } else {
        StepKind::Other
    }
}

fn continues(last: &HistoryStep, kind: StepKind) -> bool {
    let caret = &last.selection;
    match (last.kind, kind) {
        (
            StepKind::Typing { after_space, .. },
            StepKind::Typing {
                at,
                after_space: space,
            },
        ) => (!after_space || space) && caret.is_empty() && caret.start == at,
        (StepKind::Deleting { .. }, StepKind::Deleting { start, end }) => {
            caret.is_empty() && (caret.start == end || caret.start == start)
        }
        _ => false,
    }
}

fn looks_like_url(text: &str) -> bool {
    ["https://", "http://", "www."]
        .iter()
        .any(|prefix| text.starts_with(prefix))
        && !text.contains(char::is_whitespace)
}

fn image_token(id: u64) -> InlineToken {
    InlineToken::new(format!("{IMAGE_TOKEN_PREFIX}{id}"), OBJECT_MARK.to_string()).block()
}

fn image_id(token: &InlineToken) -> Option<u64> {
    token.id().strip_prefix(IMAGE_TOKEN_PREFIX)?.parse().ok()
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
        let image_observer = cx.observe(&app, |this, _, cx| {
            if this.link_image_pending(cx) {
                cx.notify();
            }
        });
        let fun_picker = cx.new(|cx| FunPicker::new(app.clone(), window, cx));
        let picker_subscription = cx.subscribe_in(&fun_picker, window, Self::on_fun_pick);
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
            typing: OutgoingTyping::default(),
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
            draft_save: None,
            saved_images: None,
            schedule: None,
            scheduling: None,
            pending_anchors: HashMap::new(),
            link: ComposeLink::default(),
            fun_picker,
            _subscriptions: [subscription, observer, image_observer, picker_subscription],
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
            self.update_link_preview(cx);
            self.schedule_draft_save(cx);
        }
        let value = input.read(cx).value();
        if submitted_text(event, &value).is_some()
            || (is_plain_enter(event) && self.has_attachments(cx))
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

    fn report_typing(&mut self, has_text: bool, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        if let Some(active) = self.typing.on_text(has_text, Instant::now()) {
            cx.emit(ComposerEvent::Typing(active));
        }
    }

    pub fn stop_typing(&mut self) -> bool {
        self.typing.stop()
    }

    fn on_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let (value, input_cursor) = (state.value().to_string(), state.cursor());
        self.report_typing(!value.trim().is_empty(), cx);
        let previous = std::mem::replace(&mut self.previous_value, value.clone());
        if previous != value {
            let (start, old_end, new_end) = changed_span(&previous, &value);
            self.shift_anchors(start, old_end, new_end);
        }
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
        if matches!(typed, Some(' ' | '\n')) && self.convert_link(cursor, window, cx) {
            return;
        }
        self.convert_emoji(typed, cursor, window, cx);
    }

    fn convert_link(&mut self, cursor: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let before = self.draft.clone();
        if !self.draft.autolink_typed(cursor) {
            return false;
        }
        self.finish_conversion(before, cursor, window, cx);
        true
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
        let changed_text = self.draft.text().get(start..cursor) != Some(pasted.as_str());
        self.commit(Some(cursor), window, cx);
        if !changed_text {
            return true;
        }
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
        self.schedule_draft_save(cx);
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
        for anchor in self.pending_anchors.values_mut() {
            *anchor = map_offset(&edits, *anchor);
        }
        let value = self.input.read(cx).value().to_string();
        if value != self.draft.text() {
            self.draft.apply_edit(&value, selection.end, None);
            self.draft.take_edits();
        }
        self.previous_value = value;
        self.refresh_style(cx);
    }

    fn shift_anchors(&mut self, start: usize, old_end: usize, new_end: usize) {
        for anchor in self.pending_anchors.values_mut() {
            *anchor = if *anchor <= start {
                *anchor
            } else if *anchor >= old_end {
                *anchor - old_end + new_end
            } else {
                new_end
            };
        }
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
            && self
                .history
                .last()
                .is_some_and(|last| continues(last, kind));
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
        let source_selection = self.history[self.history_index].selection.clone();
        self.history_index = target;
        let step = &self.history[target];
        let (draft, tokens) = (step.draft.clone(), step.tokens.clone());
        let current = self.input.read(cx).value().to_string();
        let (start, old_end, new_end) = changed_span(&current, draft.text());
        let selection = match (start == old_end && start == new_end, back) {
            (false, _) => new_end..new_end,
            (true, true) => source_selection,
            (true, false) => step.selection.clone(),
        };
        if start != old_end || start != new_end {
            self.shift_anchors(start, old_end, new_end);
        }
        self.draft = draft.clone();
        self.previous_value = draft.text().to_owned();
        self.input.update(cx, |state, cx| {
            if start != old_end || start != new_end {
                state.apply_edits(
                    &[(start..old_end, draft.text()[start..new_end].to_owned())],
                    window,
                    cx,
                );
            }
            for (range, token) in tokens {
                let present = state.tokens().iter().any(|span| span.range() == range);
                if !present && range.start >= start && range.end <= new_end {
                    state
                        .replace_range_with_token(range, token, window, cx)
                        .ok();
                }
            }
            state.set_selected_range(selection, cx);
        });
        self.undone_value = Some(self.input.read(cx).value().to_string());
        self.pending_style = None;
        self.conversion = None;
        self.paste_hint = None;
        self.refresh_style(cx);
        self.update_emoji(cx);
        self.update_mention(cx);
        self.schedule_draft_save(cx);
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
        self.pending_anchors.clear();
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
            self.schedule_draft_save(cx);
        }
        cx.notify();
    }

    fn set_size(&mut self, size: Option<SizeStep>, cx: &mut Context<Self>) {
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
            pending.set_size(size, &self.draft.style_at(cursor));
            self.pending_style = (!pending.is_empty()).then_some(pending);
        } else {
            self.draft.set_size(selection, size);
            self.draft.take_edits();
            self.record(StepKind::Other, cx);
            self.refresh_style(cx);
            self.schedule_draft_save(cx);
        }
        cx.notify();
    }

    fn normal_size_state(&self, range: Range<usize>) -> FormatState {
        let sized = [SizeStep::Small, SizeStep::Large]
            .into_iter()
            .any(|step| self.draft.state(range.clone(), &MarkKind::Size(step)) != FormatState::Off);
        if sized {
            FormatState::Off
        } else {
            FormatState::On
        }
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
            FormatButton::Superscript => self.toggle_mark(MarkKind::Superscript, cx),
            FormatButton::Subscript => self.toggle_mark(MarkKind::Subscript, cx),
            FormatButton::SizeSmall => self.set_size(Some(SizeStep::Small), cx),
            FormatButton::SizeNormal => self.set_size(None, cx),
            FormatButton::SizeLarge => self.set_size(Some(SizeStep::Large), cx),
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
            FormatButton::Superscript => self.draft.state(range, &MarkKind::Superscript),
            FormatButton::Subscript => self.draft.state(range, &MarkKind::Subscript),
            FormatButton::SizeSmall => self.draft.state(range, &MarkKind::Size(SizeStep::Small)),
            FormatButton::SizeLarge => self.draft.state(range, &MarkKind::Size(SizeStep::Large)),
            FormatButton::SizeNormal => self.normal_size_state(range),
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
                self.draft.link_word_ending_at(selection.start);
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
        let selected = &self.draft.text()[selection.clone()];
        let text = selected.replace(OBJECT_MARK, "");
        let lines = if selected.contains(OBJECT_MARK) {
            self.draft.slice_without_objects(selection)
        } else {
            self.draft.slice(selection)
        };
        if text.is_empty() {
            return true;
        }
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
        let store = &self.app.read(cx).store;
        self.recent_emoji.reload(store);
        self.recent_emoji.push(glyph);
        self.recent_emoji.save(store);
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
        let matches = emoji::search(&query, &self.recent_emoji.glyphs(), emoji_popup::LIMIT);
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

    fn update_link_preview(&mut self, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            self.link.reset();
            return;
        }
        let draft = self.current_draft(cx);
        if let Some(url) = self.link.observe(draft_link(&draft), draft.is_blank()) {
            self.fetch_link_preview(url, cx);
        }
        cx.notify();
    }

    fn fetch_link_preview(&mut self, url: String, cx: &mut Context<Self>) {
        let (demo, engine) = {
            let state = self.app.read(cx);
            (state.mode.demo, state.engine.clone())
        };
        if demo {
            let preview = crate::demo::link_preview(&url);
            self.finish_link_preview(&url, preview, cx);
            return;
        }
        let Some(engine) = engine else {
            return;
        };
        self.link.lookup = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(LINK_PREVIEW_DEBOUNCE)
                .await;
            let task_url = url.clone();
            let receiver =
                runtime::spawn(async move { engine.link_preview_for(&task_url).await });
            if let Ok(Ok(preview)) = receiver.await {
                this.update(cx, |this, cx| this.finish_link_preview(&url, preview, cx))
                    .ok();
            }
        }));
    }

    fn finish_link_preview(&mut self, url: &str, preview: Option<LinkPreview>, cx: &mut Context<Self>) {
        let image = preview.as_ref().and_then(LinkPreview::image);
        if !self.link.apply(url, preview) {
            return;
        }
        if let Some(image) = image {
            self.app
                .update(cx, |state, cx| state.request_images(vec![image], cx));
        }
        cx.notify();
    }

    fn link_image_pending(&self, cx: &App) -> bool {
        self.link
            .shown()
            .and_then(LinkPreview::image)
            .is_some_and(|image| self.app.read(cx).directory.image(&image.url).is_none())
    }

    fn render_link_preview(&self, cx: &mut Context<Self>) -> Option<Div> {
        let preview = self.link.shown()?;
        let composer = cx.weak_entity();
        let close: super::link_preview::CloseHandler = Rc::new(move |_, cx| {
            composer
                .update(cx, |this, cx| {
                    this.link.dismiss();
                    cx.notify();
                })
                .ok();
        });
        Some(
            div().w_full().mb(px(6.)).child(link_preview_card(
                preview,
                "composer-link-preview".to_owned(),
                &self.app.read(cx).directory,
                true,
                Some(close),
            )),
        )
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
            self.save_draft_now(cx);
            self.draft_save = None;
            self.saved_images = None;
            let carry = self.carry_images_into.take().as_deref() == Some(conversation_id);
            self.conversation_id = Some(conversation_id.to_owned());
            self.cancel_uploads();
            let dropped = if carry {
                self.tray.keep_images_only()
            } else {
                self.tray.clear()
            };
            self.discard_uploaded(dropped, cx);
            self.remove_orphan_image_tokens(window, cx);
            self.reply = None;
            self.link.reset();
            if self.editing.take().is_some() {
                self.load_draft(Draft::default(), InputContent::new(""), window, cx);
            }
            self.mention_inputs.clear();
            self.close_popup();
            self.schedule = None;
            if !carry {
                self.load_stored_draft(conversation_id, window, cx);
            }
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
        self.schedule_draft_save(cx);
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
        self.can_send(cx).then(|| self.compose(cx))
    }

    fn compose(&self, cx: &App) -> Outgoing {
        let state = self.input.read(cx);
        let mut draft = self.current_draft(cx);
        let image_spans: Vec<(Range<usize>, u64)> = state
            .tokens()
            .iter()
            .filter_map(|span| Some((span.range(), image_id(span.token())?)))
            .collect();
        let image_ids: Vec<u64> = image_spans
            .iter()
            .filter(|(_, id)| self.tray.image(*id).is_some())
            .map(|(_, id)| *id)
            .collect();
        for (range, _) in image_spans
            .iter()
            .rev()
            .filter(|(_, id)| self.tray.image(*id).is_none())
        {
            draft.replace(range.clone(), "");
        }
        draft.take_edits();
        let mut draft = draft.trimmed();
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
        Outgoing {
            draft,
            mentions,
            reply: self.reply.clone(),
            edit: self.editing.clone(),
            link_preview: self.link.shown().filter(|_| attachments_apply).cloned(),
            images: if attachments_apply {
                self.tray.images_in(&image_ids)
            } else {
                Vec::new()
            },
            files: if attachments_apply {
                self.tray.outgoing_files()
            } else {
                Vec::new()
            },
        }
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
        (has_text || self.has_attachments(cx)) && !self.tray.blocks_send()
    }

    fn has_attachments(&self, cx: &App) -> bool {
        self.tray.has_chips()
            || self
                .input
                .read(cx)
                .tokens()
                .iter()
                .any(|span| image_id(span.token()).is_some_and(|id| self.tray.image(id).is_some()))
    }

    fn image_token_range(&self, id: u64, cx: &App) -> Option<Range<usize>> {
        self.input
            .read(cx)
            .tokens()
            .iter()
            .find(|span| image_id(span.token()) == Some(id))
            .map(|span| span.range())
    }

    fn remove_orphan_image_tokens(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edits: Vec<Edit> = self
            .input
            .read(cx)
            .tokens()
            .iter()
            .filter(|span| image_id(span.token()).is_some_and(|id| self.tray.image(id).is_none()))
            .map(|span| (span.range(), String::new()))
            .collect();
        if !edits.is_empty() {
            self.input
                .update(cx, |state, cx| state.apply_edits(&edits, window, cx));
        }
    }

    fn remove_image_token(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self.image_token_range(id, cx) else {
            return;
        };
        self.input.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.replace("", window, cx);
        });
    }

    fn insert_image_token(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let value = state.value().to_string();
        let selection = state.selected_range();
        let mut offset = self
            .pending_anchors
            .remove(&id)
            .unwrap_or(selection.end)
            .min(value.len());
        while !value.is_char_boundary(offset) {
            offset -= 1;
        }
        if value == self.draft.text() && self.draft.in_code(offset) {
            offset = value.len();
        }
        if let Some(span) = state
            .tokens()
            .iter()
            .find(|span| span.range().start < offset && offset < span.range().end)
        {
            offset = span.range().end;
        }
        let length = OBJECT_MARK.len_utf8();
        let after = |position: usize| {
            if position >= offset {
                position + length
            } else {
                position
            }
        };
        let inserted = self.input.update(cx, |state, cx| {
            let inserted = state
                .replace_range_with_token(offset..offset, image_token(id), window, cx)
                .is_ok();
            if inserted {
                state.set_selected_range(after(selection.start)..after(selection.end), cx);
            }
            inserted
        });
        if !inserted {
            return;
        }
        for (other, anchor) in self.pending_anchors.iter_mut() {
            if *anchor > offset || (*anchor == offset && *other > id) {
                *anchor += length;
            }
        }
        self.previous_value = self.input.read(cx).value().to_string();
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

    pub fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                this.update_in(cx, |this, window, cx| this.add_paths(paths, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn add_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() || paths.is_empty() {
            return;
        }
        let ids = self.add_pending(&names_of(&paths), cx);
        for (id, path) in ids.into_iter().zip(paths) {
            cx.spawn_in(window, async move |this, cx| {
                let loaded = cx
                    .background_executor()
                    .spawn(async move { read_attachment(&path) })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.finish_reading(id, loaded, window, cx)
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// Images that no longer sit in the text give way to new attachments.
    fn make_room(&mut self, needed: usize, cx: &App) {
        if self.tray.items().len() + needed > MAX_ATTACHMENTS {
            let referenced: Vec<u64> = self
                .input
                .read(cx)
                .tokens()
                .iter()
                .filter_map(|span| image_id(span.token()))
                .collect();
            self.tray.discard_images_except(&referenced);
        }
    }

    fn add_pending(&mut self, names: &[String], cx: &App) -> Vec<u64> {
        self.make_room(names.len(), cx);
        let ids = self.tray.add_pending(names);
        let anchor = self.selection(cx).end;
        self.pending_anchors
            .extend(ids.iter().map(|id| (*id, anchor)));
        ids
    }

    fn add_pasted_image(&mut self, pasted: Image, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.add_pending(&[pasted_image_name()], cx);
        cx.notify();
        let Some(&id) = ids.first() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { prepare_pasted_image(pasted) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.finish_reading(id, loaded, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn on_fun_pick(
        &mut self,
        _: &Entity<FunPicker>,
        event: &FunPickerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FunPickerEvent::Emoji(glyph) => self.insert_emoji(glyph, window, cx),
            FunPickerEvent::Image(image) => self.insert_remote_image(image.clone(), window, cx),
        }
    }

    fn insert_emoji(&mut self, glyph: &str, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.selection(cx);
        let cursor = range.end;
        self.replace_text(range, glyph, cursor, window, cx);
        self.remember_emoji(glyph, cx);
        cx.notify();
    }

    fn insert_remote_image(
        &mut self,
        image: RemoteImage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.make_room(1, cx);
        if let Some(id) = self.tray.add_remote(image) {
            self.insert_image_token(id, window, cx);
        }
        self.focus(window, cx);
        cx.notify();
    }

    fn paste(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.editing.is_none() {
            match paste_action(item) {
                PasteAction::Text => {}
                PasteAction::Files(paths) => {
                    self.add_paths(paths, window, cx);
                    return true;
                }
                PasteAction::Image(pasted) => {
                    self.add_pasted_image(pasted, window, cx);
                    return true;
                }
            }
        }
        let Some(text) = item.text() else {
            return false;
        };
        if text.contains(OBJECT_MARK) {
            let cleaned = text.replace(OBJECT_MARK, "");
            self.input
                .update(cx, |state, cx| state.replace(cleaned, window, cx));
            return true;
        }
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match loaded {
            Ok(loaded) => {
                let files_allowed = self.files_allowed();
                let job = self.tray.finish_reading(id, loaded, files_allowed);
                if self.tray.image(id).is_some() {
                    self.insert_image_token(id, window, cx);
                } else {
                    self.pending_anchors.remove(&id);
                }
                if let Some(job) = job {
                    self.start_upload(job, cx);
                }
            }
            Err(message) => {
                self.pending_anchors.remove(&id);
                self.tray.fail_reading(id, message);
            }
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
        self.schedule_draft_save(cx);
        cx.notify();
    }

    fn retry_attachment(&mut self, id: u64, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(job) = self.tray.retry(id) {
            self.start_upload(job, cx);
        }
        cx.notify();
    }

    pub fn submit_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scheduling.is_some() {
            return;
        }
        let Some(outgoing) = self.outgoing(cx) else {
            return;
        };
        self.typing.reset();
        self.load_draft(Draft::default(), InputContent::new(""), window, cx);
        self.mention_inputs.clear();
        if self.editing.is_none() {
            self.tray.clear();
            self.clear_stored_draft(cx);
        }
        self.reply = None;
        self.editing = None;
        self.link.reset();
        self.close_popup();
        cx.emit(ComposerEvent::Submit(Box::new(outgoing)));
        cx.notify();
    }

    pub fn restore(&mut self, outgoing: &Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        self.mention_inputs.clear();
        let image_ids = if outgoing.edit.is_none() {
            self.cancel_uploads();
            self.tray.restore(&outgoing.images, &outgoing.files)
        } else {
            Vec::new()
        };
        let text = outgoing.draft.text();
        let mut content = InputContent::new(text.to_owned());
        for ((offset, _), id) in text.match_indices(OBJECT_MARK).zip(image_ids) {
            let range = offset..offset + OBJECT_MARK.len_utf8();
            if let Ok(next) = content.clone().with_token(range, image_token(id)) {
                content = next;
            }
        }
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
        self.link.restore(outgoing.link_preview.clone());
        self.close_popup();
        self.schedule_draft_save(cx);
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
        self.update_link_preview(cx);
        cx.notify();
    }

    pub fn is_empty(&self, cx: &App) -> bool {
        self.draft.is_blank() && !self.has_attachments(cx)
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
                .child(match edit.scheduled {
                    Some(_) => symbol("schedule_send", 14., theme::accent_text()).into_any_element(),
                    None => icon(IconName::Pencil, 14., theme::accent_text()).into_any_element(),
                })
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
                                .child(match edit.scheduled {
                                    Some(send_at) => format!(
                                        "Editing scheduled message - {}",
                                        scheduled_label(send_at, Utc::now())
                                    ),
                                    None => "Editing message".to_owned(),
                                }),
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
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.open_schedule_menu(cx)),
            )
            .when(!can_send, |button| button.opacity(0.4))
            .when(can_send, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(white().opacity(0.1)))
                    .on_click(cx.listener(|this, _, window, cx| this.submit_current(window, cx)))
            });
        let fun = can_attach.then(|| {
            let picker = self.fun_picker.clone();
            let reset_picker = self.fun_picker.clone();
            Popover::new("composer-fun-popover")
                .anchor(Anchor::BottomLeft)
                .offset(px(10.))
                .p_0()
                .trigger(
                    Button::new("composer-fun")
                        .ghost()
                        .size(px(32.))
                        .tooltip("Emoji, GIFs und Sticker")
                        .child(symbol("mood", 20., theme::text_muted())),
                )
                .on_open_change(move |open, window, cx| {
                    if *open {
                        reset_picker.update(cx, |picker, cx| picker.reset(window, cx));
                    }
                })
                .content(move |_, _, cx| {
                    let popover = cx.entity().downgrade();
                    picker.update(cx, |picker, _| picker.set_popover(popover));
                    picker.clone()
                })
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
                .on_click(cx.listener(|this, _, window, cx| this.open_picker(window, cx)))
        });
        let composer = cx.weak_entity();
        let tokens_composer = composer.clone();
        let input = Textarea::new(&self.input)
            .appearance(false)
            .bordered(false)
            .on_paste(move |item, window, cx| {
                composer
                    .update(cx, |this, cx| this.paste(item, window, cx))
                    .unwrap_or(false)
            })
            .token(move |context, _, cx| {
                if let Some(id) = context
                    .is_block()
                    .then(|| image_id(context.token()))
                    .flatten()
                {
                    let preview = tokens_composer
                        .read_with(cx, |this, _| this.tray.image(id))
                        .ok()
                        .flatten();
                    let remove_composer = tokens_composer.clone();
                    return inline_preview(
                        id,
                        preview,
                        context.available_width(),
                        context.is_selected(),
                        Rc::new(move |window, cx| {
                            remove_composer
                                .update(cx, |this, cx| this.remove_image_token(id, window, cx))
                                .ok();
                        }),
                    );
                }
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
                    .into_any_element()
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
            .on_action(cx.listener(|this, _: &ToggleSuperscript, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Superscript, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSubscript, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Subscript, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleCode, _, cx| {
                if this.link_editor.is_none() {
                    this.toggle_mark(MarkKind::Code, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &EditLink, window, cx| this.edit_link(window, cx)))
            .on_action(cx.listener(|this, _: &ScheduleSend, _, cx| this.open_schedule_picker(cx)))
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
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.link_editor.is_some() {
                } else if this.schedule.is_some() {
                    this.move_schedule_highlight(-1, window, cx);
                    cx.stop_propagation();
                } else if this.popup_is_open() {
                    this.move_highlight(-1, cx);
                    cx.stop_propagation();
                } else if this.editing.is_none() && this.is_empty(cx) {
                    cx.emit(ComposerEvent::EditLast);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.link_editor.is_some() {
                } else if this.schedule.is_some() {
                    this.move_schedule_highlight(1, window, cx);
                    cx.stop_propagation();
                } else if this.popup_is_open() {
                    this.move_highlight(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                if this.link_editor.is_some() {
                    this.apply_link(window, cx);
                    cx.stop_propagation();
                } else if this.schedule.is_some() {
                    this.activate_schedule_row(window, cx);
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
                if this.schedule.is_some() {
                    this.close_schedule(window, cx);
                    cx.stop_propagation();
                    return;
                }
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
            .children(self.render_link_preview(cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .children(self.render_popup(cx))
                    .children(self.render_emoji_popup(window, cx))
                    .children(self.render_toolbar(focused, cx))
                    .children(self.render_schedule(cx))
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
                                    .children(fun)
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

        use super::super::attachment_tray::InlineImage;
        use super::{Outgoing, OutgoingFile, OutgoingImage};

        let outgoing = Outgoing {
            draft: teams_core::Draft::default(),
            mentions: Vec::new(),
            link_preview: None,
            reply: None,
            edit: None,
            images: vec![OutgoingImage::Inline(InlineImage {
                name: "pasted-image.png".into(),
                image: Arc::new(Image::from_bytes(ImageFormat::Png, vec![1, 2, 3])),
                dimensions: Some((1, 1)),
            })],
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

    fn placed_image(width: u32) -> super::OutgoingImage {
        use std::sync::Arc;

        use gpui_kit::{Image, ImageFormat};

        super::OutgoingImage::Inline(super::super::attachment_tray::InlineImage {
            name: "pasted-image.png".into(),
            image: Arc::new(Image::from_bytes(ImageFormat::Png, vec![width as u8])),
            dimensions: Some((width, width)),
        })
    }

    fn remote_gif(title: &str) -> super::OutgoingImage {
        super::OutgoingImage::Remote(super::RemoteImage {
            kind: crate::remote_image::RemoteKind::Gif,
            url: "https://media0.giphy.com/media/a/giphy.gif".into(),
            title: title.into(),
            width: 200,
            height: 100,
        })
    }

    fn outgoing_with(text: &str, images: Vec<super::OutgoingImage>) -> super::Outgoing {
        super::Outgoing {
            draft: teams_core::Draft::plain(text),
            mentions: Vec::new(),
            link_preview: None,
            reply: None,
            edit: None,
            images,
            files: Vec::new(),
        }
    }

    #[test]
    fn an_image_is_placed_in_the_html_where_its_mark_sits() {
        let outgoing = outgoing_with("a \u{FFFC} b", vec![placed_image(2)]);
        assert_eq!(
            outgoing.html(),
            "a <img src=\"../hostedContents/1/$value\"> b"
        );
        assert_eq!(outgoing.text(), "a b");
    }

    #[test]
    fn images_keep_the_order_of_their_marks() {
        let outgoing = outgoing_with("\u{FFFC}x\u{FFFC}", vec![placed_image(2), placed_image(4)]);
        assert_eq!(
            outgoing.html(),
            "<img src=\"../hostedContents/1/$value\">x<img src=\"../hostedContents/2/$value\">"
        );
    }

    #[test]
    fn gifs_sit_in_place_and_only_uploaded_images_are_numbered() {
        let outgoing = outgoing_with(
            "\u{FFFC}a\u{FFFC}b\u{FFFC}",
            vec![placed_image(2), remote_gif("Wave"), placed_image(4)],
        );
        assert_eq!(
            outgoing.html(),
            "<img src=\"../hostedContents/1/$value\">a<img src=\"https://media0.giphy.com/media/a/giphy.gif\" width=\"200\" height=\"100\" alt=\"Wave\" itemtype=\"http://schema.skype.com/Giphy\">b<img src=\"../hostedContents/2/$value\">"
        );
        assert_eq!(outgoing.extras().images.len(), 2);
    }

    #[test]
    fn a_gif_title_is_escaped_in_the_html() {
        let outgoing = outgoing_with("\u{FFFC}", vec![remote_gif("a \"b\" <c>")]);
        assert!(outgoing.html().contains("alt=\"a &quot;b&quot; &lt;c&gt;\""));
    }

    #[test]
    fn a_sticker_carries_the_sticker_itemtype() {
        let sticker = super::RemoteImage::sticker(crate::stickers::popular()[0]);
        let outgoing = outgoing_with("\u{FFFC}", vec![super::OutgoingImage::Remote(sticker)]);
        assert!(outgoing.html().contains("itemtype=\"http://schema.skype.com/Sticker\""));
        assert!(outgoing.html().contains("width=\"250\" height=\"250\""));
    }

    #[test]
    fn a_mark_without_an_image_leaves_no_trace() {
        assert_eq!(outgoing_with("a\u{FFFC}b", Vec::new()).html(), "ab");
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

        fn png_bytes(side: u32) -> Vec<u8> {
            let mut bytes = Vec::new();
            image::RgbaImage::new(side, side)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .unwrap();
            bytes
        }

        fn paste_image(typist: &mut Typist<'_>, side: u32) {
            typist.cx.write_to_clipboard(gpui_kit::ClipboardItem {
                entries: vec![gpui_kit::ClipboardEntry::Image(
                    gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, png_bytes(side)),
                )],
            });
            typist.press("ctrl-v");
        }

        fn image_tokens(typist: &mut Typist<'_>) -> Vec<(std::ops::Range<usize>, bool)> {
            let composer = typist.composer.clone();
            typist.cx.update(|cx| {
                composer
                    .read(cx)
                    .input
                    .read(cx)
                    .tokens()
                    .iter()
                    .map(|span| (span.range(), span.token().is_block()))
                    .collect()
            })
        }

        #[gpui_kit::test]
        fn a_pasted_image_sits_at_the_cursor_and_is_sent_there(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("ab");
            typist.press("left");
            paste_image(&mut typist, 2);
            assert_eq!(typist.value(), "a\u{FFFC}b");
            assert_eq!(image_tokens(&mut typist), vec![(1..4, true)]);
            let outgoing = typist.outgoing();
            assert_eq!(outgoing.images.len(), 1);
            assert_eq!(
                outgoing.html(),
                "a<img src=\"../hostedContents/1/$value\">b"
            );
            let composer = typist.composer.clone();
            assert!(!typist.cx.update(|cx| composer.read(cx).tray.has_chips()));
        }

        fn wide_png_bytes() -> Vec<u8> {
            let mut bytes = Vec::new();
            image::RgbaImage::new(600, 300)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .unwrap();
            bytes
        }

        fn caret_is_visible(typist: &mut Typist<'_>) -> bool {
            let composer = typist.composer.clone();
            typist.cx.update(|cx| {
                let state = composer.read(cx).input.read(cx);
                let cursor = state.cursor();
                let caret = state.range_to_bounds(&(cursor..cursor)).unwrap();
                let viewport = state.input_bounds();
                caret.origin.y >= viewport.origin.y && caret.bottom() <= viewport.bottom()
            })
        }

        #[gpui_kit::test]
        fn typing_after_a_tall_image_keeps_the_caret_in_view(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("look at this ");
            typist.cx.write_to_clipboard(gpui_kit::ClipboardItem {
                entries: vec![gpui_kit::ClipboardEntry::Image(
                    gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, wide_png_bytes()),
                )],
            });
            typist.press("ctrl-v");
            typist.act(|window, _| window.refresh());
            typist.type_text("and then more text");
            typist.act(|window, _| window.refresh());
            assert!(caret_is_visible(&mut typist));
        }

        #[gpui_kit::test]
        fn deleting_the_token_drops_the_image_and_undo_brings_it_back(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("ab");
            typist.press("left");
            paste_image(&mut typist, 2);
            typist.press("backspace");
            assert_eq!(typist.value(), "ab");
            assert!(image_tokens(&mut typist).is_empty());
            let outgoing = typist.outgoing();
            assert!(outgoing.images.is_empty());
            assert_eq!(outgoing.html(), "ab");
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "a\u{FFFC}b");
            assert_eq!(image_tokens(&mut typist), vec![(1..4, true)]);
            assert_eq!(typist.outgoing().images.len(), 1);
        }

        #[gpui_kit::test]
        fn the_remove_button_drops_the_image_and_one_undo_brings_it_back(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("ab");
            paste_image(&mut typist, 2);
            typist.type_text("cd");
            let composer = typist.composer.clone();
            let id = typist.cx.update(|cx| {
                let state = composer.read(cx).input.read(cx);
                super::super::image_id(state.tokens()[0].token()).unwrap()
            });
            typist.act(|window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.remove_image_token(id, window, cx)
                });
            });
            assert_eq!(typist.value(), "abcd");
            assert!(image_tokens(&mut typist).is_empty());
            assert!(typist.outgoing().images.is_empty());
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "ab\u{FFFC}cd");
            assert_eq!(image_tokens(&mut typist), vec![(2..5, true)]);
            assert_eq!(typist.outgoing().images.len(), 1);
        }

        #[gpui_kit::test]
        fn images_are_sent_in_the_order_they_appear_in_the_text(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("x");
            paste_image(&mut typist, 2);
            typist.type_text("y");
            typist.press("ctrl-home");
            paste_image(&mut typist, 4);
            assert_eq!(typist.value(), "\u{FFFC}x\u{FFFC}y");
            let outgoing = typist.outgoing();
            let dimensions: Vec<_> = outgoing
                .images
                .iter()
                .map(|image| match image {
                    super::super::OutgoingImage::Inline(inline) => inline.dimensions,
                    super::super::OutgoingImage::Remote(_) => None,
                })
                .collect();
            assert_eq!(dimensions, [Some((4, 4)), Some((2, 2))]);
            assert_eq!(
                outgoing.html(),
                "<img src=\"../hostedContents/1/$value\">x<img src=\"../hostedContents/2/$value\">y"
            );
        }

        #[gpui_kit::test]
        fn an_image_alone_can_be_sent_and_never_leaks_into_copied_text(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            paste_image(&mut typist, 2);
            let composer = typist.composer.clone();
            assert!(typist.cx.update(|cx| composer.read(cx).can_send(cx)));
            let outgoing = typist.outgoing();
            assert_eq!(outgoing.text(), "");
            assert_eq!(outgoing.images.len(), 1);
            typist.type_text("hi");
            typist.press("ctrl-a ctrl-c");
            let copied = typist.cx.read_from_clipboard().and_then(|item| item.text());
            assert_eq!(copied.as_deref(), Some("hi"));
        }

        #[gpui_kit::test]
        fn an_inserted_gif_alone_can_be_sent_and_undo_removes_it(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            let composer = typist.composer.clone();
            assert!(!typist.cx.update(|cx| composer.read(cx).can_send(cx)));
            typist.act(|window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.insert_remote_image(
                        super::super::RemoteImage {
                            kind: crate::remote_image::RemoteKind::Gif,
                            url: "https://media0.giphy.com/media/a/giphy.gif".into(),
                            title: "Wave".into(),
                            width: 200,
                            height: 100,
                        },
                        window,
                        cx,
                    )
                });
            });
            assert!(typist.cx.update(|cx| composer.read(cx).can_send(cx)));
            assert_eq!(typist.value(), "\u{FFFC}");
            let outgoing = typist.outgoing();
            assert!(outgoing.html().starts_with("<img src=\"https://media0.giphy.com/"));
            assert!(outgoing.extras().images.is_empty());
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "");
            assert!(!typist.cx.update(|cx| composer.read(cx).can_send(cx)));
        }

        #[gpui_kit::test]
        fn an_inserted_emoji_lands_at_the_cursor(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("ab");
            typist.press("left");
            let composer = typist.composer.clone();
            typist.act(|window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.insert_emoji("\u{1F44D}", window, cx)
                });
            });
            assert_eq!(typist.value(), "a\u{1F44D}b");
        }

        #[gpui_kit::test]
        fn pasted_text_loses_object_marks(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist
                .cx
                .write_to_clipboard(gpui_kit::ClipboardItem::new_string("a\u{FFFC}b".to_owned()));
            typist.press("ctrl-v");
            assert_eq!(typist.value(), "ab");
        }

        #[gpui_kit::test]
        fn a_restored_outgoing_gets_its_image_tokens_back(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            let outgoing = super::outgoing_with("x\u{FFFC}y", vec![super::placed_image(3)]);
            let composer = typist.composer.clone();
            typist.act(|window, cx| {
                composer.update(cx, |composer, cx| composer.restore(&outgoing, window, cx));
            });
            assert_eq!(typist.value(), "x\u{FFFC}y");
            assert_eq!(image_tokens(&mut typist), vec![(1..4, true)]);
            let again = typist.outgoing();
            assert_eq!(again.images, outgoing.images);
            assert_eq!(again.html(), "x<img src=\"../hostedContents/1/$value\">y");
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
        fn typed_url_is_linked_and_backspace_takes_the_link_back(cx: &mut TestAppContext) {
            use teams_core::{Mark, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("see https://a.b ");
            assert_eq!(typist.value(), "see https://a.b ");
            assert_eq!(
                typist.draft().marks(),
                [Mark {
                    range: 4..15,
                    kind: MarkKind::Link("https://a.b".into())
                }]
            );
            typist.press("backspace");
            assert_eq!(typist.value(), "see https://a.b ");
            assert!(typist.draft().marks().is_empty());
        }

        #[gpui_kit::test]
        fn ctrl_z_takes_an_autolink_back(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("https://a.b ");
            assert_eq!(typist.draft().marks().len(), 1);
            typist.press("ctrl-z");
            assert!(typist.draft().marks().is_empty());
        }

        #[gpui_kit::test]
        fn pasted_url_is_linked_and_ctrl_z_restores_the_raw_text(cx: &mut TestAppContext) {
            use teams_core::{Mark, MarkKind};

            let mut typist = typist(cx);
            typist
                .cx
                .write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                    "x https://a.b y".to_owned(),
                ));
            typist.press("ctrl-v");
            assert_eq!(typist.value(), "x https://a.b y");
            assert_eq!(
                typist.draft().marks(),
                [Mark {
                    range: 2..13,
                    kind: MarkKind::Link("https://a.b".into())
                }]
            );
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "x https://a.b y");
            assert!(typist.draft().marks().is_empty());
        }

        #[gpui_kit::test]
        fn the_link_editor_changes_and_removes_an_autolink(cx: &mut TestAppContext) {
            use teams_core::{Mark, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("https://a.b x");
            typist.press("home");
            typist.press("shift-right ".repeat(11).trim_end());
            typist.press("ctrl-k");
            typist.type_text("other.de");
            typist.press("enter");
            assert_eq!(
                typist.draft().marks(),
                [Mark {
                    range: 0..11,
                    kind: MarkKind::Link("https://other.de".into())
                }]
            );
            typist.press("ctrl-k backspace enter");
            assert!(typist.draft().marks().is_empty());
            assert_eq!(typist.value(), "https://a.b x");
        }

        #[gpui_kit::test]
        fn enter_in_a_list_links_the_url_before_the_cursor(cx: &mut TestAppContext) {
            use teams_core::{Mark, MarkKind};

            let mut typist = typist(cx);
            typist.type_text("- https://a.b");
            typist.press("enter");
            assert_eq!(typist.value(), "• https://a.b\n• ");
            assert_eq!(
                typist.draft().marks(),
                [Mark {
                    range: 4..15,
                    kind: MarkKind::Link("https://a.b".into())
                }]
            );
        }

        #[gpui_kit::test]
        fn a_url_only_paste_links_without_the_paste_hint(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist
                .cx
                .write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                    "x https://a.b y".to_owned(),
                ));
            typist.press("ctrl-v");
            assert_eq!(typist.draft().marks().len(), 1);
            let composer = typist.composer.clone();
            assert!(
                typist
                    .cx
                    .update(|cx| composer.read(cx).paste_hint.is_none())
            );
            typist.press("ctrl-z");
            assert!(typist.draft().marks().is_empty());
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
        fn script_shortcuts_exclude_each_other_and_send_teams_html(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("x2");
            typist.press("shift-left ctrl-shift-=");
            assert_eq!(typist.outgoing().html(), "x<sup>2</sup>");
            typist.press("ctrl-=");
            assert_eq!(typist.outgoing().html(), "x<sub>2</sub>");
        }

        #[gpui_kit::test]
        fn the_toolbar_state_follows_the_selection(cx: &mut TestAppContext) {
            use teams_core::FormatState;

            use super::super::FormatButton;

            let mut typist = typist(cx);
            typist.type_text("big small");
            typist.press("shift-left shift-left shift-left shift-left shift-left");
            let composer = typist.composer.clone();
            typist.act(|window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.press_format(FormatButton::SizeSmall, window, cx)
                })
            });
            let states = |typist: &mut Typist<'_>, range: std::ops::Range<usize>| {
                let composer = typist.composer.clone();
                typist.cx.update(|cx| {
                    let composer = composer.read(cx);
                    [
                        FormatButton::SizeSmall,
                        FormatButton::SizeNormal,
                        FormatButton::SizeLarge,
                    ]
                    .map(|button| composer.format_state(button, &range))
                })
            };
            assert_eq!(
                states(&mut typist, 4..9),
                [FormatState::On, FormatState::Off, FormatState::Off]
            );
            assert_eq!(
                states(&mut typist, 0..9),
                [FormatState::Mixed, FormatState::Off, FormatState::Off]
            );
            assert_eq!(
                states(&mut typist, 0..3),
                [FormatState::Off, FormatState::On, FormatState::Off]
            );
            assert_eq!(
                typist.outgoing().html(),
                "big <span style=\"font-size:xx-small;\">small</span>"
            );
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

        fn cursor(typist: &mut Typist<'_>) -> usize {
            let composer = typist.composer.clone();
            typist
                .cx
                .update(|cx| composer.read(cx).input.read(cx).cursor())
        }

        #[gpui_kit::test]
        fn typing_elsewhere_is_its_own_undo_step_and_undo_puts_the_caret_there(
            cx: &mut TestAppContext,
        ) {
            let mut typist = typist(cx);
            typist.type_text("hello");
            typist.press("home");
            typist.type_text("X");
            typist.press("end ctrl-z");
            assert_eq!(typist.value(), "hello");
            assert_eq!(cursor(&mut typist), 0);
            typist.press("ctrl-y");
            assert_eq!(typist.value(), "Xhello");
            assert_eq!(cursor(&mut typist), 1);
        }

        #[gpui_kit::test]
        fn undo_keeps_the_scroll_position(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            for line in 0..14 {
                typist.type_text(&format!("line {line}"));
                typist.press("shift-enter");
            }
            typist.type_text("end");
            let composer = typist.composer.clone();
            let scroll = |typist: &mut Typist<'_>| {
                typist
                    .cx
                    .update(|cx| composer.read(cx).input.read(cx).scroll_offset().y)
            };
            typist.act(|window, cx| window.render_frame(cx));
            let before = scroll(&mut typist);
            assert!(before < gpui_kit::px(0.));
            typist.press("ctrl-z");
            typist.act(|window, cx| window.render_frame(cx));
            assert_eq!(scroll(&mut typist), before);
        }

        #[gpui_kit::test]
        fn a_smiley_taken_back_with_ctrl_z_is_sent_as_typed(cx: &mut TestAppContext) {
            let mut typist = typist(cx);
            typist.type_text("ok :) ");
            assert!(typist.value().contains('🙂'));
            typist.press("ctrl-z");
            assert_eq!(typist.value(), "ok :) ");
            assert_eq!(typist.outgoing().text(), "ok :)");
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
                    composer.add_paths(vec![path], window, cx);
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
                    composer.add_paths(vec![path], window, cx);
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
            cx.update(|cx| assert!(composer.read(cx).tray.items().is_empty()));
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
                    composer.add_paths(vec![path], window, cx);
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
                        link_preview: None,
                        reply: None,
                        edit: Some(EditPreview {
                            message_id: "m1".into(),
                            excerpt: "old".into(),
                            scheduled: None,
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
                        composer.add_paths(vec![path.clone()], window, cx);
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
        fn each_conversation_keeps_its_own_draft(cx: &mut TestAppContext) {
            let (handle, composer) = demo_composer(cx);
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_conversation("chat-a", "A", window, cx);
                    composer.set_text("**hello** there", window, cx);
                    composer.set_conversation("chat-b", "B", window, cx);
                });
            })
            .unwrap();
            cx.update(|cx| {
                let composer = composer.read(cx);
                assert!(composer.draft.is_blank());
                let stored = composer
                    .app
                    .read(cx)
                    .store
                    .draft("chat-a")
                    .unwrap()
                    .unwrap();
                assert_eq!(stored.preview, "hello there");
            });
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_text("other", window, cx);
                    composer.set_conversation("chat-a", "A", window, cx);
                });
            })
            .unwrap();
            cx.update(|cx| {
                let composer = composer.read(cx);
                assert_eq!(composer.draft.text(), "hello there");
                assert!(composer.draft.to_html().contains("<b>"));
                assert!(
                    composer
                        .app
                        .read(cx)
                        .store
                        .draft("chat-b")
                        .unwrap()
                        .is_some()
                );
            });
        }

        #[gpui_kit::test]
        fn sending_clears_the_stored_draft_and_a_blank_composer_deletes_it(
            cx: &mut TestAppContext,
        ) {
            let (handle, composer) = demo_composer(cx);
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.set_conversation("chat-a", "A", window, cx);
                    composer.set_text("hi", window, cx);
                    composer.save_draft_now(cx);
                });
            })
            .unwrap();
            cx.update(|cx| {
                assert!(
                    composer
                        .read(cx)
                        .app
                        .read(cx)
                        .store
                        .draft("chat-a")
                        .unwrap()
                        .is_some()
                );
            });
            cx.update_window(handle, |_, window, cx| {
                composer.update(cx, |composer, cx| composer.submit_current(window, cx));
            })
            .unwrap();
            cx.update(|cx| {
                assert!(
                    composer
                        .read(cx)
                        .app
                        .read(cx)
                        .store
                        .draft("chat-a")
                        .unwrap()
                        .is_none()
                );
            });
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
                    composer.add_paths(vec![png, pdf], window, cx);
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
