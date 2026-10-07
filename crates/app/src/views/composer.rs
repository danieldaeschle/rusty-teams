use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    h_flex,
    input::{
        Backspace, Enter, Escape, IndentInline, InlineToken, InputContent, InputEvent, MoveDown,
        MoveUp, Textarea, TextareaState,
    },
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{
    FileReference, HostedImage, MentionCandidate, MentionInput, MessageExtras, UploadedFile,
};

use super::attachment_tray::{
    AttachmentTray, DoneFile, JobKind, LoadedFile, OutgoingFile, OutgoingImage, PasteAction,
    UploadJob, UploadResult, discard_uploaded, names_of, paste_action, pasted_image_name,
    prepare_pasted_image, read_attachment, render_tray,
};
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
const DEMO_UPLOAD_STEPS: u8 = 10;
const DEMO_UPLOAD_STEP: Duration = Duration::from_millis(120);
const DEMO_FAILURE_STEP: u8 = 6;

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
    pub images: Vec<OutgoingImage>,
    pub files: Vec<OutgoingFile>,
}

impl Outgoing {
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
    tray: AttachmentTray,
    uploads: HashMap<u64, UploadHandle>,
    demo_failed: HashSet<u64>,
    carry_images_into: Option<String>,
    _subscription: Subscription,
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
            tray: AttachmentTray::default(),
            uploads: HashMap::new(),
            demo_failed: HashSet::new(),
            carry_images_into: None,
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
                self.input
                    .update(cx, |state, cx| state.set_value("", window, cx));
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
        let mut text = state.value().trim().to_owned();
        if !self.can_send(cx) {
            return None;
        }
        let attachments_apply = self.editing.is_none();
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

    fn can_send(&self, cx: &App) -> bool {
        let has_text = !self.input.read(cx).value().trim().is_empty();
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

    fn paste(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
        if self.editing.is_some() {
            return false;
        }
        match paste_action(item) {
            PasteAction::Text => false,
            PasteAction::Files(paths) => {
                self.add_paths(paths, cx);
                true
            }
            PasteAction::Image(pasted) => {
                self.add_pasted_image(pasted, cx);
                true
            }
        }
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
        self.input
            .update(cx, |state, cx| state.set_value("", window, cx));
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
        if outgoing.edit.is_none() {
            self.cancel_uploads();
            self.tray.restore(&outgoing.images, &outgoing.files);
        }
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
        self.input.read(cx).value().trim().is_empty() && self.tray.is_empty()
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
            .on_paste(move |item, _, cx| {
                composer
                    .update(cx, |this, cx| this.paste(item, cx))
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
            .children(notice)
            .children(self.render_reply_strip(cx))
            .children(self.render_edit_strip(cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .children(self.render_popup(cx))
                    .children(self.render_emoji_popup(window, cx))
                    .child(
                        v_flex()
                            .w_full()
                            .rounded(px(10.))
                            .bg(theme::surface())
                            .border_1()
                            .border_color(if focused {
                                theme::accent()
                            } else {
                                theme::border_strong()
                            })
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
                                    .child(div().flex_1().min_w_0().child(input))
                                    .child(send),
                            ),
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

    #[test]
    fn extras_carry_image_bytes_with_their_mime_type_and_the_file_references() {
        use std::sync::Arc;

        use gpui_kit::{Image, ImageFormat};
        use teams_core::FileKind;

        use super::{Outgoing, OutgoingFile, OutgoingImage};

        let outgoing = Outgoing {
            text: String::new(),
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
                assert_eq!(outgoing.text, "");
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
                        text: "old".into(),
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
                assert_eq!(outgoing.text, "old");
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
