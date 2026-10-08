use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Escape,
    message_scroller::{MessageScroller, MessageScrollerState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::MessageRecord;
use teams_core::{FileCard, ImageRef};

use super::attachments::FileActions;
use super::avatar::{member_stack, person_avatar, spec_avatar, square_avatar, with_presence};
use super::composer::{Composer, ComposerEvent, EditPreview, Outgoing, ReplyPreview};
use super::message_actions::{Action, MessageMenu};
use super::message_row::{RowActions, render_message_row, render_skeleton_row};
use super::new_chat::{NewChatDraft, NewChatEvent, composer_placeholder, existing_one_on_one};
use super::reaction_picker::{PickHandler, ReactionPicker};
use super::reaction_pills::{ReactionControls, ReactionPopover};
use super::widgets::{icon, symbol};
use crate::app_state::{AppEvent, AppState, Selection, selection_title};
use crate::backend::Engine;
use crate::data::{is_one_on_one, others};
use crate::downloads::{self, ClickAction, DownloadKey, Downloads, PartFile, RevealTarget};
use crate::reaction_model::UNKNOWN_REACTOR;
use crate::read_state::{ReadTrigger, plan_read};
use crate::render::layout_blocks;
use crate::rows::{
    Delivery, LocalImage, MessageRow, Receipt, Row, RowContext, Series, StartInfo, api_reaction,
    assign_series, changed_indices, diff_keys, flat_rows, message_draft, message_text,
    placeholder_rows, reaction_glyph, reaction_type_for, reply_excerpt, thread_list_rows,
    thread_rows, trailing_skeleton,
};
use crate::runtime;
use crate::sidebar_model::{AvatarSpec, Face};
use crate::theme;

const CHAT_OPEN_LIMIT: usize = 60;
const CHANNEL_OPEN_LIMIT: usize = 300;
const GROWTH_HEADROOM: usize = 20;
const META_USER_ID: &str = "me_user_id";
const PENDING_KEY_PREFIX: &str = "pending-";
const MAX_NOTICE_CHARS: usize = 140;
const DEMO_DOWNLOAD_STEPS: u8 = 10;
const DEMO_DOWNLOAD_STEP: Duration = Duration::from_millis(100);
const JUMP_SCAN_LIMIT: usize = 5000;
const JUMP_CONTEXT_MESSAGES: usize = 12;
const HIGHLIGHT_DURATION: Duration = Duration::from_secs(2);
const PROGRESS_DELAY: Duration = Duration::from_millis(400);
const SKELETON_DELAY: Duration = Duration::from_millis(1000);
const SLOW_AFTER: Duration = Duration::from_secs(8);
const PROGRESS_MIN_VISIBLE: Duration = Duration::from_millis(500);
const PROGRESS_BAR_PERIOD: Duration = Duration::from_millis(1400);
const PROGRESS_BAR_WIDTH: f32 = 0.35;

actions!(teams, [ReplyToHovered]);

#[derive(Clone, Copy)]
enum HoverSlot {
    Row,
    Toolbar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ViewMode {
    Flat,
    ThreadList,
    Thread(String),
}

#[derive(Default)]
struct SyncProgress {
    messages: bool,
    receipts: bool,
    empty_cache: bool,
    newest_cached: Option<DateTime<Utc>>,
    shown_at: Option<Instant>,
    trailing_skeleton: bool,
    slow: bool,
    failed: bool,
}

impl SyncProgress {
    fn running(&self) -> bool {
        self.messages || self.receipts
    }
}

struct Current {
    selection: Selection,
    title: String,
    mode: ViewMode,
    loaded_limit: usize,
    loaded_count: usize,
    has_older: bool,
    loading_older: bool,
    older_blocked: bool,
    fetched: bool,
    first_unread: Option<String>,
    sync: SyncProgress,
}

struct ReactionDetails {
    message_key: String,
    anchor_glyph: String,
    tab: Option<String>,
}

pub struct ConversationView {
    app: Entity<AppState>,
    scroller: Entity<MessageScrollerState>,
    composer: Entity<Composer>,
    draft: Entity<NewChatDraft>,
    draft_active: bool,
    draft_created: Option<(Vec<String>, String)>,
    draft_error: Option<String>,
    rows: Rc<RefCell<Vec<Row>>>,
    current: Option<Current>,
    pending: Vec<MessageRow>,
    pending_counter: usize,
    pending_outgoing: HashMap<String, Outgoing>,
    downloads: Downloads,
    hovered_message: Option<String>,
    toolbar_hovered: Option<String>,
    toolbar_pinned: Option<String>,
    picker: Entity<ReactionPicker>,
    reaction_details: Option<ReactionDetails>,
    highlighted_message: Option<String>,
    notice: Option<String>,
    drag_file_count: usize,
    sync_generation: u64,
    window_active: bool,
    _subscriptions: Vec<Subscription>,
}

fn open_limit(selection: &Selection) -> usize {
    match selection {
        Selection::Chat(_) => CHAT_OPEN_LIMIT,
        Selection::Channel(_) => CHANNEL_OPEN_LIMIT,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropTarget {
    Chat,
    Channel,
    NewChat,
}

fn drop_overlay_text(title: &str, target: DropTarget, count: usize) -> (String, String) {
    let files = if count == 1 {
        "1 file".to_owned()
    } else {
        format!("{count} files")
    };
    match target {
        DropTarget::Chat => (
            format!("Drop to attach to {title}"),
            format!("{files}. Images go into the message, other files to your OneDrive."),
        ),
        DropTarget::Channel => (
            format!("Drop to attach to {title}"),
            format!("{files}. Images go into the message, other files to the channel's Files."),
        ),
        DropTarget::NewChat => (
            "Drop to attach to the new chat".to_owned(),
            format!(
                "{files}. Images go into the message. Files can be added once the chat exists."
            ),
        ),
    }
}

fn short_error(error: &dyn std::fmt::Display) -> String {
    error.to_string().chars().take(MAX_NOTICE_CHARS).collect()
}

enum DownloadEvent {
    Progress(u8),
    Finished(Result<PathBuf, String>),
}

async fn save_file(
    engine: &Engine,
    card: &FileCard,
    events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
) -> Result<PathBuf, String> {
    let mut part = PartFile::create(&downloads::downloads_directory(), &card.name)
        .map_err(|error| short_error(&error))?;
    engine
        .download_file(
            &card.open_url,
            |bytes| part.write(bytes),
            |percent| {
                let _ = events.send(DownloadEvent::Progress(percent));
            },
        )
        .await
        .map_err(|error| short_error(&error))?;
    part.finish().map_err(|error| short_error(&error))
}

impl ConversationView {
    fn file_actions(
        &self,
        message_key: &str,
        files: &[FileCard],
        view: WeakEntity<Self>,
    ) -> FileActions {
        let conversation_id = self.conversation_id().unwrap_or_default();
        let (key, cards) = (message_key.to_owned(), files.to_vec());
        let open_urls: Vec<&str> = files.iter().map(|card| card.open_url.as_str()).collect();
        FileActions {
            states: self
                .downloads
                .states_for(&conversation_id, message_key, &open_urls),
            activate: Rc::new(move |index, cx| {
                let Some(card) = cards.get(index).cloned() else {
                    return;
                };
                let key = key.clone();
                let conversation_id = conversation_id.clone();
                view.update(cx, |this, cx| {
                    this.activate_file(&conversation_id, &key, card, cx)
                })
                .ok();
            }),
        }
    }

    fn activate_file(
        &mut self,
        conversation_id: &str,
        message_key: &str,
        card: FileCard,
        cx: &mut Context<Self>,
    ) {
        let key = DownloadKey::new(conversation_id, message_key, &card.open_url);
        match downloads::click_action(self.downloads.state(&key)) {
            ClickAction::Ignore => {}
            ClickAction::Start => self.start_download(key, card, cx),
            ClickAction::Reveal(path) => {
                let folder = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(downloads::downloads_directory);
                match downloads::reveal_target(&path, path.exists(), folder) {
                    RevealTarget::File(path) => downloads::reveal_in_file_manager(&path),
                    RevealTarget::Folder(folder) => cx.open_with_system(&folder),
                }
            }
        }
    }

    fn start_download(&mut self, key: DownloadKey, card: FileCard, cx: &mut Context<Self>) {
        if !self.downloads.begin(&key) {
            return;
        }
        cx.notify();
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DownloadEvent>();
        if demo {
            cx.spawn(async move |this, cx| {
                for step in 1..=DEMO_DOWNLOAD_STEPS {
                    cx.background_executor().timer(DEMO_DOWNLOAD_STEP).await;
                    let percent = (u32::from(step) * 100 / u32::from(DEMO_DOWNLOAD_STEPS)) as u8;
                    let progressed = this.update(cx, |this, cx| {
                        this.downloads.set_progress(&key, percent);
                        cx.notify();
                    });
                    if progressed.is_err() {
                        return;
                    }
                }
                let outcome =
                    downloads::write_demo_file(&card.name).map_err(|error| short_error(&error));
                this.update(cx, |this, cx| {
                    this.downloads.finish(&key, outcome);
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        let Some(engine) = engine else {
            self.downloads.finish(&key, Err("not connected".to_owned()));
            return;
        };
        drop(runtime::spawn(async move {
            let outcome = save_file(&engine, &card, &sender).await;
            let _ = sender.send(DownloadEvent::Finished(outcome));
        }));
        cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.recv().await {
                let updated = this.update(cx, |this, cx| {
                    match event {
                        DownloadEvent::Progress(percent) => {
                            this.downloads.set_progress(&key, percent)
                        }
                        DownloadEvent::Finished(outcome) => this.downloads.finish(&key, outcome),
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let composer = cx.new(|cx| Composer::new(app.clone(), window, cx));
        let picker = cx.new(|cx| ReactionPicker::new(app.clone(), window, cx));
        let draft = cx.new(|cx| NewChatDraft::new(app.clone(), window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&app, window, Self::on_app_event),
            cx.subscribe_in(&composer, window, Self::on_composer_event),
            cx.subscribe_in(&draft, window, Self::on_draft_event),
            cx.observe_window_activation(window, Self::on_window_activation),
            cx.observe_keystrokes(|this, event, _, cx| {
                if event.keystroke.key == "escape" {
                    this.close_reaction_details(cx);
                }
            }),
        ];
        let mut view = ConversationView {
            app,
            scroller,
            composer,
            draft,
            draft_active: false,
            draft_created: None,
            draft_error: None,
            rows: Rc::new(RefCell::new(Vec::new())),
            current: None,
            pending: Vec::new(),
            pending_counter: 0,
            pending_outgoing: HashMap::new(),
            downloads: Downloads::default(),
            hovered_message: None,
            toolbar_hovered: None,
            toolbar_pinned: None,
            picker,
            reaction_details: None,
            highlighted_message: None,
            notice: None,
            drag_file_count: 0,
            sync_generation: 0,
            window_active: window.is_window_active(),
            _subscriptions: subscriptions,
        };
        if let Some(selection) = view.app.read(cx).selection.clone() {
            view.open(selection, window, cx);
        }
        view
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn composer_is_empty(&self, cx: &App) -> bool {
        self.composer.read(cx).is_empty(cx)
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn send_in_flight(&self) -> bool {
        self.pending
            .iter()
            .any(|row| matches!(row.delivery, Delivery::Sending))
    }

    fn on_window_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window_active = window.is_window_active();
        self.mark_read(ReadTrigger::Activation, cx);
    }

    fn on_app_event(
        &mut self,
        _: &Entity<AppState>,
        event: &AppEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            AppEvent::Selection => {
                self.reaction_details = None;
                let (new_chat, selection) = {
                    let state = self.app.read(cx);
                    (state.new_chat, state.selection.clone())
                };
                if new_chat {
                    self.enter_draft(window, cx);
                    return;
                }
                if self.draft_active {
                    let created = self
                        .draft_created
                        .as_ref()
                        .map(|(_, chat_id)| chat_id.clone());
                    if let Some(chat_id) = created {
                        self.composer
                            .update(cx, |composer, _| composer.carry_images_into(chat_id));
                    }
                    self.leave_draft(window, cx);
                }
                match selection {
                    Some(selection) => self.open(selection, window, cx),
                    None => {
                        self.current = None;
                        self.rebuild(true, cx);
                    }
                }
            }
            AppEvent::Messages(conversation_id) => {
                let is_current = self
                    .current
                    .as_ref()
                    .is_some_and(|current| current.selection.conversation_id() == conversation_id);
                if is_current {
                    self.rebuild(false, cx);
                    self.mark_read(ReadTrigger::Incoming, cx);
                }
            }
            AppEvent::Images(keys) => self.remeasure_images(keys, cx),
            AppEvent::Jump => self.apply_pending_jump(cx),
            AppEvent::Status => {
                if let Some(current) = self.current.as_mut()
                    && current.sync.failed
                {
                    current.fetched = false;
                }
                self.start_fetch(cx)
            }
            AppEvent::Directory => {
                self.refresh_reactor_names(cx);
                cx.notify();
            }
            AppEvent::Sidebar => {
                self.mark_read(ReadTrigger::Incoming, cx);
                self.refresh_reactor_names(cx);
                if self.draft_active {
                    self.on_draft_changed(window, cx);
                } else if let Some(current) = self.current.as_mut() {
                    let title = selection_title(&self.app.read(cx).sidebar, &current.selection);
                    if current.title != title {
                        current.title = title.clone();
                        let conversation_id = current.selection.conversation_id().to_owned();
                        self.composer.update(cx, |composer, cx| {
                            composer.set_conversation(&conversation_id, &title, window, cx)
                        });
                        cx.notify();
                    }
                }
            }
        }
    }

    fn on_composer_event(
        &mut self,
        _: &Entity<Composer>,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Submit(outgoing) if self.draft_active => {
                self.send_new_chat((**outgoing).clone(), window, cx)
            }
            ComposerEvent::Submit(outgoing) if outgoing.edit.is_some() => {
                self.send_edit((**outgoing).clone(), window, cx)
            }
            ComposerEvent::Submit(outgoing) => self.send((**outgoing).clone(), window, cx),
            ComposerEvent::EditLast => self.edit_last_own(window, cx),
        }
    }

    fn open(&mut self, selection: Selection, window: &mut Window, cx: &mut Context<Self>) {
        self.toolbar_hovered = None;
        self.toolbar_pinned = None;
        let title = selection_title(&self.app.read(cx).sidebar, &selection);
        let mode = match selection {
            Selection::Chat(_) => ViewMode::Flat,
            Selection::Channel(_) => ViewMode::ThreadList,
        };
        let first_unread = self.first_unread_id(&selection, cx);
        self.current = Some(Current {
            loaded_limit: open_limit(&selection),
            selection,
            title,
            mode,
            loaded_count: 0,
            has_older: false,
            loading_older: false,
            older_blocked: false,
            fetched: false,
            first_unread,
            sync: SyncProgress::default(),
        });
        self.sync_generation += 1;
        self.clear_pending();
        self.notice = None;
        self.rebuild(true, cx);
        self.start_fetch(cx);
        self.mark_read(ReadTrigger::Open, cx);
        let target = self
            .current
            .as_ref()
            .map(|current| {
                (
                    current.selection.conversation_id().to_owned(),
                    current.title.clone(),
                )
            })
            .unwrap_or_default();
        self.composer.update(cx, |composer, cx| {
            composer.set_conversation(&target.0, &target.1, window, cx);
        });
        if !self.draft_active {
            self.composer
                .update(cx, |composer, cx| composer.focus(window, cx));
        }
        self.hovered_message = None;
        self.apply_pending_jump(cx);
    }

    fn enter_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.draft_active {
            self.draft_active = true;
            self.current = None;
            self.clear_pending();
            self.notice = None;
            self.draft_created = None;
            self.draft_error = None;
            self.toolbar_hovered = None;
            self.toolbar_pinned = None;
            self.hovered_message = None;
            self.sync_generation += 1;
            self.draft.update(cx, |draft, cx| draft.reset(window, cx));
            self.composer.update(cx, |composer, cx| {
                composer.set_conversation("", "", window, cx)
            });
            self.on_draft_changed(window, cx);
            self.rebuild(true, cx);
        }
        self.draft
            .update(cx, |draft, cx| draft.focus_query(window, cx));
        cx.notify();
    }

    fn leave_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_active = false;
        self.draft_created = None;
        self.draft_error = None;
        self.clear_pending();
        self.draft.update(cx, |draft, cx| draft.reset(window, cx));
    }

    fn on_draft_event(
        &mut self,
        _: &Entity<NewChatDraft>,
        event: &NewChatEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            NewChatEvent::Changed => self.on_draft_changed(window, cx),
            NewChatEvent::FocusComposer => self
                .composer
                .update(cx, |composer, cx| composer.focus(window, cx)),
            NewChatEvent::Close => self.app.update(cx, |state, cx| state.close_new_chat(cx)),
        }
    }

    fn draft_target_chat(&self, cx: &App) -> Option<String> {
        let chips = self.draft.read(cx).chips();
        let [chip] = chips else {
            return None;
        };
        let state = self.app.read(cx);
        existing_one_on_one(
            &state.sidebar.chats,
            state.directory.me.as_ref(),
            &chip.user_id,
        )
        .map(|chat| chat.id.clone())
    }

    fn on_draft_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.draft_active {
            return;
        }
        let target = self.draft_target_chat(cx);
        if target != self.conversation_id() {
            match target {
                Some(chat_id) => {
                    self.composer.update(cx, |composer, _| {
                        composer.carry_images_into(chat_id.clone())
                    });
                    self.open(Selection::Chat(chat_id), window, cx)
                }
                None => {
                    self.composer.update(cx, |composer, cx| {
                        composer.carry_images_into(String::new());
                        composer.set_conversation("", "", window, cx);
                    });
                    self.current = None;
                    self.sync_generation += 1;
                    self.rebuild(true, cx);
                }
            }
        }
        let placeholder = {
            let draft = self.draft.read(cx);
            composer_placeholder(draft.chips(), &draft.group_name(cx))
        };
        self.composer.update(cx, |composer, cx| {
            composer.set_placeholder(&placeholder, window, cx)
        });
        cx.notify();
    }

    fn remeasure_images(&mut self, keys: &[String], cx: &mut Context<Self>) {
        let indices: Vec<usize> = self
            .rows
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(index, row)| match row {
                Row::Message(message)
                    if message.images.iter().any(|image| keys.contains(&image.url)) =>
                {
                    Some(index)
                }
                _ => None,
            })
            .collect();
        self.scroller.update(cx, |scroller, cx| {
            for index in indices {
                scroller.remeasure_items(index..index + 1, cx);
            }
        });
        cx.notify();
    }

    fn apply_pending_jump(&mut self, cx: &mut Context<Self>) {
        let Some(conversation_id) = self
            .current
            .as_ref()
            .map(|current| current.selection.conversation_id().to_owned())
        else {
            return;
        };
        let jump = self
            .app
            .update(cx, |state, _| match state.pending_jump.take() {
                Some((target, message_id)) if target == conversation_id => Some(message_id),
                other => {
                    state.pending_jump = other;
                    None
                }
            });
        if let Some(message_id) = jump {
            self.jump_to_message(&conversation_id, &message_id, cx);
        }
    }

    fn jump_to_message(&mut self, conversation_id: &str, message_id: &str, cx: &mut Context<Self>) {
        let store = self.app.read(cx).store.clone();
        let Some(record) = store
            .messages_by_id(conversation_id, &[message_id.to_owned()])
            .ok()
            .and_then(|mut found| found.remove(message_id))
        else {
            return;
        };
        let messages_from_target = store
            .messages(conversation_id, None, JUMP_SCAN_LIMIT)
            .ok()
            .and_then(|all| {
                all.iter()
                    .position(|candidate| candidate.message_id == message_id)
                    .map(|position| all.len() - position)
            })
            .unwrap_or_default();
        let row_key = record
            .reply_to_id
            .clone()
            .unwrap_or_else(|| message_id.to_owned());
        let Some(current) = self.current.as_mut() else {
            return;
        };
        current.loaded_limit = current
            .loaded_limit
            .max(messages_from_target + JUMP_CONTEXT_MESSAGES);
        current.first_unread = None;
        if matches!(current.selection, Selection::Channel(_)) {
            current.mode = match record.reply_to_id {
                Some(root_id) => ViewMode::Thread(root_id),
                None => ViewMode::ThreadList,
            };
        }
        let target_key = if matches!(current.mode, ViewMode::Thread(_)) {
            message_id.to_owned()
        } else {
            row_key
        };
        self.clear_pending();
        self.rebuild(true, cx);
        let index = self
            .rows
            .borrow()
            .iter()
            .position(|row| row.key() == target_key);
        if let Some(index) = index {
            self.scroller
                .update(cx, |scroller, cx| scroller.scroll_to_item(index, cx));
            self.highlight(target_key, cx);
        }
    }

    fn highlight(&mut self, key: String, cx: &mut Context<Self>) {
        self.highlighted_message = Some(key.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT_DURATION).await;
            this.update(cx, |this, cx| {
                if this.highlighted_message.as_deref() == Some(key.as_str()) {
                    this.highlighted_message = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn set_composer_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.set_text(text, window, cx));
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
    }

    pub fn reply_to_latest_from_others(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.rows.borrow().iter().rev().find_map(|row| match row {
            Row::Message(message) if !message.own => Some(message.key.clone()),
            _ => None,
        });
        if let Some(key) = key {
            self.begin_reply(&key, window, cx);
        }
    }

    pub fn reply_to_hovered(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(message_id) = self.hovered_message.clone() {
            self.begin_reply(&message_id, window, cx);
        }
    }

    fn begin_reply(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conversation_id) = self
            .current
            .as_ref()
            .map(|current| current.selection.conversation_id().to_owned())
        else {
            return;
        };
        let store = self.app.read(cx).store.clone();
        let record = store
            .messages_by_id(&conversation_id, &[message_id.to_owned()])
            .ok()
            .and_then(|mut found| found.remove(message_id))
            .filter(|record| !record.deleted);
        let Some(record) = record else {
            return;
        };
        let preview = ReplyPreview {
            message_id: record.message_id.clone(),
            author: record
                .sender_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_owned()),
            excerpt: reply_excerpt(&record),
        };
        self.composer.update(cx, |composer, cx| {
            composer.set_reply(Some(preview), cx);
            composer.focus(window, cx);
        });
    }

    fn first_unread_id(&self, selection: &Selection, cx: &App) -> Option<String> {
        let Selection::Chat(chat_id) = selection else {
            return None;
        };
        let state = self.app.read(cx);
        if state.mode.demo {
            return crate::demo::first_unread(chat_id);
        }
        state.engine.as_ref()?.first_unread_message_id(chat_id)
    }

    fn row_context(&self, cx: &App) -> RowContext {
        let now = Local::now();
        let app = self.app.read(cx);
        let mut names: HashMap<String, String> = HashMap::new();
        let known_people = app
            .directory
            .me
            .iter()
            .map(|me| (me.user_id.clone(), me.display_name.clone()))
            .chain(
                app.sidebar
                    .chats
                    .iter()
                    .flat_map(|chat| chat.members.iter())
                    .filter_map(|member| {
                        Some((member.user_id.clone()?, member.display_name.clone()))
                    }),
            );
        for (user_id, name) in known_people {
            names.entry(user_id).or_insert(name);
        }
        RowContext {
            offset: now.offset().fix(),
            today: now.date_naive(),
            my_user_id: app.store.meta(META_USER_ID).ok().flatten(),
            names,
        }
    }

    fn unknown_reactor_ids(&self) -> Vec<String> {
        self.rows
            .borrow()
            .iter()
            .filter_map(|row| match row {
                Row::Message(message) => Some(message),
                _ => None,
            })
            .flat_map(|message| message.reactions.iter())
            .flat_map(|chip| chip.reactors.iter())
            .filter(|reactor| reactor.name == UNKNOWN_REACTOR)
            .filter_map(|reactor| reactor.user_id.clone())
            .collect()
    }

    fn refresh_reactor_names(&mut self, cx: &mut Context<Self>) {
        if self.current.is_none() {
            return;
        }
        let unknown = self.unknown_reactor_ids();
        if unknown.is_empty() {
            return;
        }
        let names = self.row_context(cx).names;
        if unknown.iter().any(|user_id| names.contains_key(user_id)) {
            self.rebuild(false, cx);
        }
    }

    fn drop_stale_reaction_details(&mut self) {
        let Some(details) = self.reaction_details.as_ref() else {
            return;
        };
        let present = self.rows.borrow().iter().any(|row| {
            matches!(row, Row::Message(message) if message.key == details.message_key
                && message.reactions.iter().any(|chip| chip.glyph() == details.anchor_glyph))
        });
        if !present {
            self.reaction_details = None;
        }
    }

    fn open_reaction_details(&mut self, message_key: &str, glyph: &str, cx: &mut Context<Self>) {
        let multiple_kinds = self.rows.borrow().iter().any(|row| {
            matches!(row, Row::Message(message) if message.key == message_key && message.reactions.len() > 1)
        });
        self.reaction_details = Some(ReactionDetails {
            message_key: message_key.to_owned(),
            anchor_glyph: glyph.to_owned(),
            tab: multiple_kinds.then(|| glyph.to_owned()),
        });
        cx.notify();
    }

    fn select_reaction_tab(&mut self, tab: Option<String>, cx: &mut Context<Self>) {
        if let Some(details) = self.reaction_details.as_mut() {
            details.tab = tab;
            cx.notify();
        }
    }

    fn close_reaction_details(&mut self, cx: &mut Context<Self>) {
        if self.reaction_details.take().is_some() {
            cx.notify();
        }
    }

    fn reaction_controls(&self, key: &str, view: WeakEntity<Self>) -> ReactionControls {
        let open = {
            let (view, key) = (view.clone(), key.to_owned());
            Rc::new(move |glyph: &str, cx: &mut App| {
                view.update(cx, |this, cx| this.open_reaction_details(&key, glyph, cx))
                    .ok();
            })
        };
        let select_tab = {
            let view = view.clone();
            Rc::new(move |tab: Option<String>, cx: &mut App| {
                view.update(cx, |this, cx| this.select_reaction_tab(tab, cx))
                    .ok();
            })
        };
        let close = Rc::new(move |cx: &mut App| {
            view.update(cx, |this, cx| this.close_reaction_details(cx))
                .ok();
        });
        let popover = self
            .reaction_details
            .as_ref()
            .filter(|details| details.message_key == key)
            .map(|details| ReactionPopover {
                anchor_glyph: details.anchor_glyph.clone(),
                tab: details.tab.clone(),
            });
        ReactionControls {
            open,
            select_tab,
            close,
            popover,
        }
    }

    fn start_info(&self, cx: &App) -> Option<StartInfo> {
        let current = self.current.as_ref()?;
        let app = self.app.read(cx);
        let user_id = match &current.selection {
            Selection::Chat(chat_id) => app
                .sidebar
                .chats
                .iter()
                .find(|chat| &chat.id == chat_id)
                .filter(|chat| is_one_on_one(chat))
                .and_then(|chat| others(chat, app.directory.me.as_ref()).into_iter().next())
                .and_then(|(user_id, _)| user_id),
            Selection::Channel(_) => None,
        };
        Some(StartInfo {
            title: current.title.clone(),
            user_id,
        })
    }

    fn rebuild(&mut self, reset: bool, cx: &mut Context<Self>) {
        let Some(current) = self.current.as_ref() else {
            let mut rows: Vec<Row> = self
                .pending
                .iter()
                .cloned()
                .map(|row| Row::Message(Box::new(row)))
                .collect();
            assign_series(&mut rows);
            self.apply_rows(rows, true, cx);
            return;
        };
        let conversation_id = current.selection.conversation_id().to_owned();
        let limit = if reset {
            current.loaded_limit
        } else {
            current
                .loaded_limit
                .max(current.loaded_count + GROWTH_HEADROOM)
        };
        let store = self.app.read(cx).store.clone();
        let records: Vec<MessageRecord> = store
            .messages(&conversation_id, None, limit)
            .unwrap_or_default();
        let sync_has_more = store
            .sync_state(&conversation_id)
            .ok()
            .flatten()
            .is_none_or(|state| state.has_more);
        let mut context = self.row_context(cx);
        for record in &records {
            if let (Some(user_id), Some(name)) = (&record.sender_id, &record.sender_name) {
                context
                    .names
                    .entry(user_id.clone())
                    .or_insert_with(|| name.clone());
            }
        }
        let can_load_older = self.app.read(cx).engine.is_some();
        let start = self.start_info(cx);
        let current = self.current.as_mut().expect("checked above");
        let first_unread = current.first_unread.clone();
        current.loaded_limit = limit;
        current.loaded_count = records.len();
        let has_older = sync_has_more && !current.older_blocked && can_load_older;
        let placeholders = current.sync.messages
            && records.is_empty()
            && !matches!(current.mode, ViewMode::Thread(_));
        let trailing = current.sync.trailing_skeleton && current.mode == ViewMode::Flat;
        let mut rows = match &current.mode {
            _ if placeholders => {
                current.has_older = false;
                placeholder_rows()
            }
            ViewMode::Flat => {
                current.has_older = has_older;
                let mut rows = flat_rows(&records, &context, has_older);
                if !has_older && let Some(start) = start {
                    rows.insert(0, Row::Start(start));
                }
                rows
            }
            ViewMode::ThreadList => {
                current.has_older = has_older;
                thread_list_rows(&records, &context, has_older)
            }
            ViewMode::Thread(root_id) => {
                current.has_older = false;
                thread_rows(&records, root_id, &context)
            }
        };
        if trailing && !placeholders {
            rows.push(trailing_skeleton());
        }
        rows.extend(
            self.pending
                .iter()
                .cloned()
                .map(|row| Row::Message(Box::new(row))),
        );
        let receipts = self.receipts(&records, cx);
        for row in rows.iter_mut() {
            if let Row::Message(message) = row
                && message.own
            {
                message.receipt = receipts.get(&message.key).copied().unwrap_or_default();
            }
        }
        if let Some(unread_id) = &first_unread {
            for row in rows.iter_mut() {
                if let Row::Message(message) = row
                    && &message.key == unread_id
                {
                    message.new_marker = true;
                }
            }
        }
        assign_series(&mut rows);
        self.apply_rows(rows, reset, cx);
    }

    fn apply_rows(&mut self, new_rows: Vec<Row>, reset: bool, cx: &mut Context<Self>) {
        let old_rows = std::mem::replace(&mut *self.rows.borrow_mut(), new_rows);
        self.drop_stale_reaction_details();
        let new_len = self.rows.borrow().len();
        if reset {
            let marker_index = self
                .rows
                .borrow()
                .iter()
                .position(|row| matches!(row, Row::Message(message) if message.new_marker));
            self.scroller.update(cx, |scroller, cx| {
                scroller.reset(new_len, cx);
                match marker_index {
                    Some(index) if !scroller.scroll_to_item(index, cx) => {
                        scroller.scroll_to_end(cx)
                    }
                    Some(_) => {}
                    None => scroller.scroll_to_end(cx),
                }
            });
            cx.notify();
            return;
        }
        let (splice, changed) = {
            let new_rows = self.rows.borrow();
            let old_keys: Vec<String> = old_rows.iter().map(|row| row.key().to_owned()).collect();
            let new_keys: Vec<String> = new_rows.iter().map(|row| row.key().to_owned()).collect();
            (
                diff_keys(&old_keys, &new_keys),
                changed_indices(&old_rows, &new_rows),
            )
        };
        self.scroller.update(cx, |scroller, cx| {
            if let Some(splice) = splice
                && !scroller.splice(splice.range, splice.count, cx)
            {
                scroller.reset(new_len, cx);
            }
            for index in changed {
                scroller.remeasure_items(index..index + 1, cx);
            }
        });
        cx.notify();
    }

    fn start_fetch(&mut self, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let (engine, demo_sync) = (state.engine.clone(), state.mode.demo_sync);
        if engine.is_none() && demo_sync.is_none() {
            return;
        }
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.fetched {
            return;
        }
        current.fetched = true;
        let is_chat = matches!(current.selection, Selection::Chat(_));
        let newest_cached = self.rows.borrow().iter().rev().find_map(|row| match row {
            Row::Message(message) if !message.key.starts_with(PENDING_KEY_PREFIX) => {
                Some(message.created_at)
            }
            _ => None,
        });
        let current = self.current.as_mut().expect("checked above");
        if current.sync.failed {
            self.notice = None;
        }
        current.sync = SyncProgress {
            messages: true,
            receipts: is_chat,
            empty_cache: newest_cached.is_none(),
            newest_cached,
            ..SyncProgress::default()
        };
        self.sync_generation += 1;
        let generation = self.sync_generation;
        let conversation_id = current.selection.conversation_id().to_owned();
        match (engine, demo_sync) {
            (Some(engine), _) => {
                let receiver =
                    runtime::spawn(async move { engine.fetch_newer(&conversation_id).await });
                cx.spawn(async move |this, cx| {
                    let result = receiver.await;
                    this.update(cx, |this, cx| this.finish_fetch(generation, result, cx))
                        .ok();
                })
                .detach();
            }
            (None, Some(delay)) => self.simulate_sync(generation, delay, cx),
            (None, None) => {}
        }
        self.schedule_progress(generation, cx);
        self.rebuild(false, cx);
        self.mark_read(ReadTrigger::Incoming, cx);
        self.refresh_receipts(generation, cx);
    }

    fn simulate_sync(&mut self, generation: u64, delay: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                if let Some(sync) = this.sync_of(generation) {
                    sync.receipts = false;
                }
                this.finish_fetch(generation, Ok(Ok(teams_core::Delta::default())), cx)
            })
            .ok();
        })
        .detach();
    }

    fn sync_of(&mut self, generation: u64) -> Option<&mut SyncProgress> {
        if generation != self.sync_generation {
            return None;
        }
        self.current.as_mut().map(|current| &mut current.sync)
    }

    fn schedule_progress(&mut self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let stages = [
                (PROGRESS_DELAY, Duration::ZERO),
                (SKELETON_DELAY, PROGRESS_DELAY),
                (SLOW_AFTER, SKELETON_DELAY),
            ];
            for (stage, (at, previous)) in stages.into_iter().enumerate() {
                cx.background_executor().timer(at - previous).await;
                let running = this
                    .update(cx, |this, cx| this.advance_progress(generation, stage, cx))
                    .unwrap_or(false);
                if !running {
                    return;
                }
            }
        })
        .detach();
    }

    fn advance_progress(&mut self, generation: u64, stage: usize, cx: &mut Context<Self>) -> bool {
        let Some(sync) = self.sync_of(generation) else {
            return false;
        };
        if !sync.running() {
            return false;
        }
        match stage {
            0 => sync.shown_at = Some(Instant::now()),
            1 if sync.messages => {
                sync.trailing_skeleton = true;
                self.rebuild(false, cx);
            }
            1 => {}
            _ => sync.slow = true,
        }
        cx.notify();
        true
    }

    fn finish_fetch(
        &mut self,
        generation: u64,
        result: Result<
            teams_core::Result<teams_core::Delta>,
            tokio::sync::oneshot::error::RecvError,
        >,
        cx: &mut Context<Self>,
    ) {
        let Some(sync) = self.sync_of(generation) else {
            return;
        };
        sync.messages = false;
        sync.trailing_skeleton = false;
        let newest_cached = sync.newest_cached;
        match result {
            Ok(Ok(_)) => self.mark_fetched_new(newest_cached, cx),
            Ok(Err(error)) => self.fail_sync(generation, short_error(&error)),
            Err(error) => self.fail_sync(generation, short_error(&error)),
        }
        self.rebuild(false, cx);
        self.settle_progress(generation, cx);
    }

    fn fail_sync(&mut self, generation: u64, error: String) {
        if let Some(sync) = self.sync_of(generation) {
            sync.failed = true;
            sync.receipts = false;
        }
        self.notice = Some(format!("Could not refresh: {error}"));
    }

    fn mark_fetched_new(&mut self, newest_cached: Option<DateTime<Utc>>, cx: &mut Context<Self>) {
        let Some(newest_cached) = newest_cached else {
            return;
        };
        let Some(my_user_id) = self.row_context(cx).my_user_id else {
            return;
        };
        let store = self.app.read(cx).store.clone();
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.first_unread.is_some() || current.mode != ViewMode::Flat {
            return;
        }
        let limit = current
            .loaded_limit
            .max(current.loaded_count + GROWTH_HEADROOM);
        current.first_unread = store
            .messages(current.selection.conversation_id(), None, limit)
            .unwrap_or_default()
            .into_iter()
            .find(|record| {
                record.created_at > newest_cached && record.sender_id.as_ref() != Some(&my_user_id)
            })
            .map(|record| record.message_id);
    }

    fn settle_progress(&mut self, generation: u64, cx: &mut Context<Self>) {
        let Some(sync) = self.sync_of(generation) else {
            return;
        };
        if sync.running() {
            return;
        }
        let remaining = sync
            .shown_at
            .map(|shown_at| PROGRESS_MIN_VISIBLE.saturating_sub(shown_at.elapsed()))
            .unwrap_or_default();
        if remaining.is_zero() {
            sync.shown_at = None;
            sync.slow = false;
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(remaining).await;
            this.update(cx, |this, cx| {
                if let Some(sync) = this.sync_of(generation) {
                    sync.shown_at = None;
                    sync.slow = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn retry_fetch(&mut self, cx: &mut Context<Self>) {
        if let Some(current) = self.current.as_mut() {
            current.fetched = false;
        }
        self.start_fetch(cx);
    }

    fn receipts(&self, records: &[MessageRecord], cx: &App) -> HashMap<String, Receipt> {
        let state = self.app.read(cx);
        let receipts_syncing = self
            .current
            .as_ref()
            .is_some_and(|current| current.sync.receipts);
        if state.mode.demo {
            let read_ids = crate::demo::read_message_ids(records);
            return records
                .iter()
                .map(|record| {
                    let receipt = if receipts_syncing {
                        Receipt::Pending
                    } else if read_ids.contains(&record.message_id) {
                        Receipt::Read
                    } else {
                        Receipt::Sent
                    };
                    (record.message_id.clone(), receipt)
                })
                .collect();
        }
        let Some(engine) = state.engine.as_ref() else {
            return HashMap::new();
        };
        let unknown = if receipts_syncing {
            Receipt::Pending
        } else {
            Receipt::Hidden
        };
        records
            .iter()
            .map(|record| {
                let receipt = match engine.receipt_state_for(record) {
                    teams_core::ReceiptState::Read { .. } => Receipt::Read,
                    teams_core::ReceiptState::Sent => Receipt::Sent,
                    teams_core::ReceiptState::Unknown => unknown,
                };
                (record.message_id.clone(), receipt)
            })
            .collect()
    }

    fn refresh_receipts(&mut self, generation: u64, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let (Some(engine), Some(Selection::Chat(chat_id))) = (
            state.engine.clone(),
            self.current.as_ref().map(|c| c.selection.clone()),
        ) else {
            return;
        };
        let receiver = runtime::spawn(async move { engine.refresh_receipts(&chat_id).await });
        cx.spawn(async move |this, cx| {
            receiver.await.ok();
            this.update(cx, |this, cx| {
                let Some(sync) = this.sync_of(generation) else {
                    return;
                };
                sync.receipts = false;
                this.rebuild(false, cx);
                this.settle_progress(generation, cx);
            })
            .ok();
        })
        .detach();
    }

    fn mark_read(&mut self, trigger: ReadTrigger, cx: &mut Context<Self>) {
        if self.draft_active {
            return;
        }
        let state = self.app.read(cx);
        let Some(Selection::Chat(chat_id)) = self.current.as_ref().map(|c| c.selection.clone())
        else {
            return;
        };
        if (state.engine.is_none() && !state.mode.demo) || state.mode.read_only {
            return;
        }
        let unread = state
            .sidebar
            .chats
            .iter()
            .any(|chat| chat.id == chat_id && chat.unread);
        let Some(plan) = plan_read(self.window_active, unread, trigger) else {
            return;
        };
        self.app
            .update(cx, |state, cx| state.mark_chat_read(&chat_id, cx));
        if plan.clear_divider
            && let Some(current) = self.current.as_mut()
            && current.first_unread.take().is_some()
        {
            self.rebuild(false, cx);
        }
    }

    fn request_older(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.app.read(cx).engine.clone() else {
            return;
        };
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.loading_older || current.older_blocked || !current.has_older {
            return;
        }
        current.loading_older = true;
        let conversation_id = current.selection.conversation_id().to_owned();
        let task_id = conversation_id.clone();
        let receiver = runtime::spawn(async move { engine.load_older(&task_id).await });
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            this.update(cx, |this, cx| {
                this.finish_older(&conversation_id, result, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_older(
        &mut self,
        conversation_id: &str,
        result: Result<
            teams_core::Result<Vec<MessageRecord>>,
            tokio::sync::oneshot::error::RecvError,
        >,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.selection.conversation_id() != conversation_id {
            return;
        }
        current.loading_older = false;
        match result {
            Ok(Ok(records)) if !records.is_empty() => {
                current.loaded_limit = current.loaded_count + records.len();
            }
            Ok(Ok(_)) => current.older_blocked = true,
            Ok(Err(error)) => {
                current.older_blocked = true;
                self.notice = Some(format!(
                    "Could not load older messages: {}",
                    short_error(&error)
                ));
            }
            Err(_) => current.older_blocked = true,
        }
        self.rebuild(false, cx);
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
        self.pending_outgoing.clear();
    }

    fn open_thread(&mut self, root_id: String, cx: &mut Context<Self>) {
        if let Some(current) = self.current.as_mut() {
            current.mode = ViewMode::Thread(root_id);
        }
        self.clear_pending();
        self.rebuild(true, cx);
    }

    fn back_to_threads(&mut self, cx: &mut Context<Self>) {
        if let Some(current) = self.current.as_mut() {
            current.mode = ViewMode::ThreadList;
        }
        self.clear_pending();
        self.rebuild(true, cx);
    }

    fn send(&mut self, outgoing: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let (mode, engine) = (state.mode, state.engine.clone());
        let Some(current) = self.current.as_ref() else {
            return;
        };
        if mode.read_only || mode.demo {
            self.restore_unsent("Read-only mode: nothing was sent", &outgoing, window, cx);
            return;
        }
        let Some(engine) = engine else {
            self.restore_unsent("Not connected yet: nothing was sent", &outgoing, window, cx);
            return;
        };
        let conversation_id = current.selection.conversation_id().to_owned();
        let view_mode = current.mode.clone();
        let key = self.push_pending(&outgoing, cx);
        self.notice = None;
        self.rebuild(false, cx);
        self.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_end(cx));

        let sent = outgoing.clone();
        let receiver = runtime::spawn(async move {
            let extras = sent.extras();
            let html = sent.html();
            let text = &html;
            let Outgoing {
                mentions, reply, ..
            } = &sent;
            match (&view_mode, reply) {
                (_, Some(reply)) => {
                    engine
                        .reply_to_with_extras(
                            &conversation_id,
                            &reply.message_id,
                            text,
                            mentions,
                            &extras,
                        )
                        .await
                }
                (ViewMode::Flat, None) => {
                    engine
                        .send_message_with_extras(&conversation_id, text, None, mentions, &extras)
                        .await
                }
                (ViewMode::ThreadList, None) => {
                    engine
                        .post_to_channel_with_extras(
                            &conversation_id,
                            text,
                            None,
                            mentions,
                            &extras,
                        )
                        .await
                }
                (ViewMode::Thread(root_id), None) => {
                    engine
                        .send_message_with_extras(
                            &conversation_id,
                            text,
                            Some(root_id),
                            mentions,
                            &extras,
                        )
                        .await
                }
            }
            .map(|_| ())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            this.update(cx, |this, cx| this.finish_send(&key, result, cx))
                .ok();
        })
        .detach();
    }

    fn restore_unsent(
        &mut self,
        message: &str,
        outgoing: &Outgoing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notice = Some(message.to_owned());
        self.composer
            .update(cx, |composer, cx| composer.restore(outgoing, window, cx));
        cx.notify();
    }

    fn push_pending(&mut self, outgoing: &Outgoing, cx: &App) -> String {
        self.pending_counter += 1;
        let key = format!("{PENDING_KEY_PREFIX}{}", self.pending_counter);
        let my_user_id = self.row_context(cx).my_user_id;
        self.pending_outgoing.insert(key.clone(), outgoing.clone());
        self.pending.push(MessageRow {
            key: key.clone(),
            author: "You".to_owned(),
            sender_id: my_user_id,
            created_at: Utc::now(),
            series: Series::default(),
            card: false,
            time: chrono::Local::now().format("%H:%M").to_string(),
            day_header: None,
            blocks: if outgoing.draft.is_blank() {
                Vec::new()
            } else {
                layout_blocks(&teams_core::html_to_spans(&outgoing.html()))
            },
            edited: false,
            deleted: false,
            reactions: Vec::new(),
            images: Vec::new(),
            local_images: outgoing
                .images
                .iter()
                .map(|image| LocalImage {
                    image: image.image.clone(),
                    size: image.dimensions,
                })
                .collect(),
            files: outgoing
                .files
                .iter()
                .map(|file| FileCard {
                    name: file.name.clone(),
                    kind: file.kind,
                    content_type: None,
                    size: Some(file.size),
                    open_url: file.reference.content_url.clone(),
                })
                .collect(),
            reply_count: None,
            new_marker: false,
            reply_faces: Vec::new(),
            last_reply_time: None,
            open_thread: None,
            is_reply: false,
            delivery: Delivery::Sending,
            receipt: Receipt::Hidden,
            own: true,
        });
        key
    }

    fn send_new_chat(&mut self, outgoing: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        let (recipients, topic) = {
            let draft = self.draft.read(cx);
            let topic = Some(draft.group_name(cx)).filter(|name| !name.is_empty());
            (draft.chips().to_vec(), topic)
        };
        if recipients.is_empty() {
            self.restore_unsent("Add at least one person first", &outgoing, window, cx);
            self.draft
                .update(cx, |draft, cx| draft.focus_query(window, cx));
            return;
        }
        if self
            .pending
            .iter()
            .any(|row| matches!(row.delivery, Delivery::Sending))
        {
            self.restore_unsent("Still creating the chat", &outgoing, window, cx);
            return;
        }
        let state = self.app.read(cx);
        let (mode, engine) = (state.mode, state.engine.clone());
        if mode.read_only {
            self.restore_unsent("Read-only mode: nothing was sent", &outgoing, window, cx);
            return;
        }
        if mode.demo && outgoing.has_attachments() {
            self.restore_unsent("Read-only mode: nothing was sent", &outgoing, window, cx);
            return;
        }
        if mode.demo {
            self.send_demo_new_chat(&outgoing, &recipients, topic.as_deref(), cx);
            return;
        }
        let Some(engine) = engine else {
            self.restore_unsent("Not connected yet: nothing was sent", &outgoing, window, cx);
            return;
        };
        let user_ids: Vec<String> = recipients.into_iter().map(|pick| pick.user_id).collect();
        let known_chat = self
            .draft_created
            .as_ref()
            .filter(|(created_for, _)| *created_for == user_ids)
            .map(|(_, chat_id)| chat_id.clone())
            .or_else(|| self.draft_target_chat(cx));
        self.pending.clear();
        self.pending_outgoing.clear();
        self.draft_error = None;
        self.notice = None;
        let key = self.push_pending(&outgoing, cx);
        self.rebuild(true, cx);
        let recipients_for_finish = user_ids.clone();
        let receiver = runtime::spawn(async move {
            let chat_id = match known_chat {
                Some(chat_id) => chat_id,
                None => {
                    let created = match user_ids.as_slice() {
                        [user_id] => engine.create_one_on_one(user_id).await,
                        _ => engine.create_group(&user_ids, topic.as_deref()).await,
                    };
                    match created {
                        Ok(chat_id) => chat_id,
                        Err(error) => return (None, Some(short_error(&error))),
                    }
                }
            };
            let sent = engine
                .send_message_with_extras(
                    &chat_id,
                    &outgoing.html(),
                    None,
                    &outgoing.mentions,
                    &outgoing.extras(),
                )
                .await;
            (Some(chat_id), sent.err().map(|error| short_error(&error)))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            this.update_in(cx, |this, _, cx| {
                this.finish_new_chat(&key, recipients_for_finish, result, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_new_chat(
        &mut self,
        key: &str,
        user_ids: Vec<String>,
        result: Result<(Option<String>, Option<String>), tokio::sync::oneshot::error::RecvError>,
        cx: &mut Context<Self>,
    ) {
        if !self.draft_active || !self.pending.iter().any(|row| row.key == key) {
            return;
        }
        let (chat_id, error) = result.unwrap_or((None, Some("cancelled".to_owned())));
        self.draft_created = chat_id.clone().map(|chat_id| (user_ids, chat_id));
        match (chat_id, error) {
            (Some(chat_id), None) => {
                self.app.update(cx, |state, cx| {
                    state.reload_sidebar(cx);
                    state.select(Selection::Chat(chat_id), cx);
                });
            }
            (created, error) => {
                if let Some(row) = self.pending.iter_mut().find(|row| row.key == key) {
                    row.delivery = Delivery::Failed(error.unwrap_or_default());
                }
                self.draft_error = Some(
                    if created.is_some() {
                        "Couldn't send the message."
                    } else {
                        "Couldn't create the chat."
                    }
                    .to_owned(),
                );
                self.rebuild(false, cx);
            }
        }
    }

    fn send_demo_new_chat(
        &mut self,
        outgoing: &Outgoing,
        recipients: &[super::new_chat::Pick],
        topic: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let (store, chats) = {
            let state = self.app.read(cx);
            (state.store.clone(), state.sidebar.chats.clone())
        };
        let people: Vec<(String, String)> = recipients
            .iter()
            .map(|pick| (pick.user_id.clone(), pick.name.clone()))
            .collect();
        let (chat, message) = match self.draft_target_chat(cx) {
            Some(chat_id) => {
                let Some(chat) = chats.iter().find(|chat| chat.id == chat_id) else {
                    return;
                };
                crate::demo::send_in_chat(chat, &outgoing.text(), &outgoing.html(), Utc::now())
            }
            None => crate::demo::new_chat(
                &chats,
                &people,
                topic,
                &outgoing.text(),
                &outgoing.html(),
                Utc::now(),
            ),
        };
        let _ = store.upsert_chats(std::slice::from_ref(&chat));
        let _ = store.upsert_messages(std::slice::from_ref(&message));
        self.app.update(cx, |state, cx| {
            state.reload_sidebar(cx);
            state.select(Selection::Chat(chat.id.clone()), cx);
        });
    }

    fn conversation_id(&self) -> Option<String> {
        self.current
            .as_ref()
            .map(|current| current.selection.conversation_id().to_owned())
    }

    fn own_record(&self, message_id: &str, cx: &App) -> Option<MessageRecord> {
        let conversation_id = self.conversation_id()?;
        let my_user_id = self.row_context(cx).my_user_id?;
        self.app
            .read(cx)
            .store
            .messages_by_id(&conversation_id, &[message_id.to_owned()])
            .ok()?
            .remove(message_id)
            .filter(|record| !record.deleted && record.sender_id.as_deref() == Some(&my_user_id))
    }

    fn set_hover(&mut self, slot: HoverSlot, key: &str, hovered: bool, cx: &mut Context<Self>) {
        let target = match slot {
            HoverSlot::Row => &mut self.hovered_message,
            HoverSlot::Toolbar => &mut self.toolbar_hovered,
        };
        if hovered {
            *target = Some(key.to_owned());
        } else if target.as_deref() == Some(key) {
            *target = None;
        }
        cx.notify();
    }

    fn set_pinned(&mut self, key: &str, open: bool, cx: &mut Context<Self>) {
        if open {
            self.toolbar_pinned = Some(key.to_owned());
        } else if self.toolbar_pinned.as_deref() == Some(key) {
            self.toolbar_pinned = None;
        }
        cx.notify();
    }

    fn toolbar_visible(&self, key: &str) -> bool {
        self.toolbar_pinned
            .as_deref()
            .or(self.toolbar_hovered.as_deref())
            .or(self.hovered_message.as_deref())
            == Some(key)
    }

    fn toggle_reaction(&mut self, message_id: &str, glyph: &str, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        let reaction_type = reaction_type_for(glyph);
        let wanted = reaction_glyph(glyph);
        let state = self.app.read(cx);
        let (mode, engine, store) = (state.mode, state.engine.clone(), state.store.clone());
        let my_user_id = self.row_context(cx).my_user_id;
        let Some(mut record) = store
            .messages_by_id(&conversation_id, &[message_id.to_owned()])
            .ok()
            .and_then(|mut found| found.remove(message_id))
        else {
            return;
        };
        let mut reactions = teams_core::reactions(&record);
        let mine = |reaction: &teams_core::ReactionInfo| {
            reaction_glyph(&reaction.reaction_type) == wanted
                && my_user_id.is_some()
                && reaction.user_id == my_user_id
        };
        let remove = reactions.iter().any(mine);
        if mode.demo {
            if remove {
                reactions.retain(|reaction| !mine(reaction));
            } else {
                reactions.push(teams_core::ReactionInfo {
                    reaction_type: reaction_type.clone(),
                    user_id: my_user_id.clone(),
                    user_name: Some("You".to_owned()),
                    created_at: Some(Utc::now()),
                });
            }
            record.reactions_json = serde_json::to_string(&reactions).unwrap_or_default();
            let _ = store.upsert_messages(std::slice::from_ref(&record));
            self.rebuild(false, cx);
            return;
        }
        let Some(engine) = engine.filter(|_| !mode.read_only) else {
            self.show_notice("Read-only mode: reaction not sent", cx);
            return;
        };
        let message_id = message_id.to_owned();
        let reaction_type = api_reaction(&wanted);
        let receiver = runtime::spawn(async move {
            if remove {
                engine
                    .unset_reaction(&conversation_id, &message_id, &reaction_type)
                    .await
            } else {
                engine
                    .set_reaction(&conversation_id, &message_id, &reaction_type)
                    .await
            }
        });
        self.report_failure(receiver, "Reaction failed", cx);
    }

    fn begin_edit(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(record) = self.own_record(message_id, cx) else {
            return;
        };
        let edit = EditPreview {
            message_id: record.message_id.clone(),
            excerpt: reply_excerpt(&record),
        };
        let in_chat = matches!(
            self.current.as_ref().map(|current| &current.selection),
            Some(Selection::Chat(_))
        );
        let mentions = if in_chat {
            teams_core::user_mention_inputs(&record)
        } else {
            Vec::new()
        };
        let draft = Outgoing {
            draft: message_draft(&record),
            mentions,
            reply: None,
            edit: Some(edit),
            images: Vec::new(),
            files: Vec::new(),
        };
        self.composer
            .update(cx, |composer, cx| composer.begin_edit(draft, window, cx));
    }

    fn edit_last_own(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft_active {
            return;
        }
        let key = self.rows.borrow().iter().rev().find_map(|row| match row {
            Row::Message(message)
                if message.own
                    && !message.deleted
                    && !message.key.starts_with(PENDING_KEY_PREFIX) =>
            {
                Some(message.key.clone())
            }
            _ => None,
        });
        if let Some(key) = key {
            self.begin_edit(&key, window, cx);
        }
    }

    fn send_edit(&mut self, outgoing: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = outgoing.edit.clone() else {
            return;
        };
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        let state = self.app.read(cx);
        let (mode, engine, store) = (state.mode, state.engine.clone(), state.store.clone());
        if mode.demo {
            if let Some(mut record) = self.own_record(&edit.message_id, cx) {
                record.body_html = outgoing.html();
                record.edited_at = Some(Utc::now());
                let _ = store.upsert_messages(std::slice::from_ref(&record));
                self.rebuild(false, cx);
            }
            return;
        }
        let Some(engine) = engine.filter(|_| !mode.read_only) else {
            self.notice = Some("Read-only mode: edit not saved".to_owned());
            self.composer
                .update(cx, |composer, cx| composer.restore(&outgoing, window, cx));
            cx.notify();
            return;
        };
        let sent = outgoing.clone();
        let receiver = runtime::spawn(async move {
            engine
                .edit_message_html(
                    &conversation_id,
                    &edit.message_id,
                    &sent.html(),
                    &sent.mentions,
                )
                .await
        });
        cx.spawn_in(window, async move |this, cx| {
            let failed = !matches!(receiver.await, Ok(Ok(())));
            this.update_in(cx, |this, window, cx| {
                if failed {
                    if this.composer.read(cx).is_empty(cx) {
                        this.notice =
                            Some("Edit failed: your text is back in the composer".to_owned());
                        this.composer
                            .update(cx, |composer, cx| composer.restore(&outgoing, window, cx));
                    } else {
                        this.notice = Some("Edit failed".to_owned());
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn delete(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let Some(mut record) = self.own_record(message_id, cx) else {
            return;
        };
        let state = self.app.read(cx);
        let (mode, engine, store) = (state.mode, state.engine.clone(), state.store.clone());
        if mode.demo {
            record.deleted = true;
            let _ = store.upsert_messages(std::slice::from_ref(&record));
            self.rebuild(false, cx);
            return;
        }
        let Some(engine) = engine.filter(|_| !mode.read_only) else {
            self.show_notice("Read-only mode: message not deleted", cx);
            return;
        };
        let conversation_id = record.conversation_id.clone();
        let receiver = runtime::spawn(async move {
            engine
                .soft_delete_message(&conversation_id, &record.message_id)
                .await
        });
        self.report_failure(receiver, "Delete failed", cx);
    }

    fn copy_text(&self, message_id: &str, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        let record = self
            .app
            .read(cx)
            .store
            .messages_by_id(&conversation_id, &[message_id.to_owned()])
            .ok()
            .and_then(|mut found| found.remove(message_id));
        if let Some(record) = record {
            cx.write_to_clipboard(ClipboardItem::new_string(message_text(&record)));
        }
    }

    fn show_notice(&mut self, message: &str, cx: &mut Context<Self>) {
        self.notice = Some(message.to_owned());
        cx.notify();
    }

    fn report_failure(
        &mut self,
        receiver: tokio::sync::oneshot::Receiver<teams_core::Result<()>>,
        label: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let error = match receiver.await {
                Ok(Ok(())) => return,
                Ok(Err(error)) => short_error(&error),
                Err(_) => "cancelled".to_owned(),
            };
            this.update(cx, |this, cx| {
                this.show_notice(&format!("{label}: {error}"), cx)
            })
            .ok();
        })
        .detach();
    }

    fn message_menu(&self, key: &str, own: bool, view: WeakEntity<Self>) -> MessageMenu {
        let action = |run: fn(&mut Self, &str, &mut Window, &mut Context<Self>)| -> Action {
            let (view, key) = (view.clone(), key.to_owned());
            Rc::new(move |window, cx| {
                view.update(cx, |this, cx| run(this, &key, window, cx)).ok();
            })
        };
        let react: PickHandler = {
            let (view, key) = (view.clone(), key.to_owned());
            Rc::new(move |glyph, _, cx| {
                view.update(cx, |this, cx| this.toggle_reaction(&key, glyph, cx))
                    .ok();
            })
        };
        let hover = {
            let (view, key) = (view.clone(), key.to_owned());
            Rc::new(move |hovered: bool, cx: &mut App| {
                view.update(cx, |this, cx| {
                    this.set_hover(HoverSlot::Toolbar, &key, hovered, cx)
                })
                .ok();
            })
        };
        let pin = {
            let (view, key) = (view.clone(), key.to_owned());
            Rc::new(move |open: bool, _: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| this.set_pinned(&key, open, cx))
                    .ok();
            })
        };
        MessageMenu {
            key: key.to_owned(),
            react,
            reply: Some(action(|this, key, window, cx| {
                this.begin_reply(key, window, cx)
            })),
            copy: action(|this, key, _, cx| this.copy_text(key, cx)),
            edit: own.then(|| action(|this, key, window, cx| this.begin_edit(key, window, cx))),
            delete: own.then(|| action(|this, key, _, cx| this.delete(key, cx))),
            pin,
            hover,
            picker: self.picker.clone(),
        }
    }

    fn retry(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(outgoing) = self.pending_outgoing.remove(key) else {
            return;
        };
        self.pending.retain(|row| row.key != key);
        if self.draft_active {
            self.send_new_chat(outgoing, window, cx);
        } else {
            self.send(outgoing, window, cx);
        }
    }

    fn finish_send(
        &mut self,
        key: &str,
        result: Result<teams_core::Result<()>, tokio::sync::oneshot::error::RecvError>,
        cx: &mut Context<Self>,
    ) {
        let failure = match result {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(short_error(&error)),
            Err(error) => Some(short_error(&error)),
        };
        match failure {
            None => {
                self.pending.retain(|row| row.key != key);
                self.pending_outgoing.remove(key);
            }
            Some(error) => {
                if let Some(row) = self.pending.iter_mut().find(|row| row.key == key) {
                    row.delivery = Delivery::Failed(error.clone());
                    self.notice = Some(format!("Not sent: {error}"));
                }
            }
        }
        self.rebuild(false, cx);
    }

    fn render_drop_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let target = match self.current.as_ref().map(|current| &current.selection) {
            Some(Selection::Chat(_)) => DropTarget::Chat,
            Some(Selection::Channel(_)) => DropTarget::Channel,
            None => DropTarget::NewChat,
        };
        let title = self
            .current
            .as_ref()
            .map(|current| current.title.as_str())
            .unwrap_or_default();
        let (heading, detail) = drop_overlay_text(title, target, self.drag_file_count.max(1));
        let composer = self.composer.clone();
        div()
            .id("drop-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .opacity(0.)
            .bg(theme::drop_background())
            .drag_over::<ExternalPaths>(move |style, _, _, cx| {
                if composer.read(cx).is_editing() {
                    style
                } else {
                    style.opacity(1.)
                }
            })
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let count = event.drag(cx).paths().len();
                    if this.drag_file_count != count {
                        this.drag_file_count = count;
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| {
                let paths = dropped.paths().to_vec();
                this.composer
                    .update(cx, |composer, cx| composer.add_paths(paths, cx));
            }))
            .child(
                div()
                    .absolute()
                    .inset(px(12.))
                    .rounded(px(12.))
                    .border_2()
                    .border_dashed()
                    .border_color(theme::accent())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        v_flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(28.))
                            .py(px(18.))
                            .rounded(px(12.))
                            .bg(black().opacity(0.55))
                            .child(symbol("upload", 28., theme::accent_text()))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_strong())
                                    .child(heading),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(detail),
                            ),
                    ),
            )
    }

    fn render_draft_error(&self, cx: &mut Context<Self>) -> Option<Div> {
        let message = self.draft_error.clone()?;
        let failed_key = self
            .pending
            .iter()
            .find(|row| matches!(row.delivery, Delivery::Failed(_)))
            .map(|row| row.key.clone())?;
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .px(px(24.))
                .py(px(4.))
                .gap(px(8.))
                .items_center()
                .text_size(px(12.))
                .text_color(theme::red_soft())
                .child(message)
                .child(
                    Button::new("retry-new-chat")
                        .ghost()
                        .compact()
                        .label("Retry")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.retry(&failed_key, window, cx)
                        })),
                ),
        )
    }

    fn render_header(&self, current: &Current, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.app.read(cx);
        let directory = &app.directory;
        let me = directory.me.as_ref();
        let mut lead: Option<AnyElement> = None;
        let mut subline = String::new();
        let mut stack: Option<Div> = None;
        match &current.selection {
            Selection::Chat(chat_id) => {
                if let Some(chat) = app.sidebar.chats.iter().find(|chat| &chat.id == chat_id) {
                    let faces = others(chat, me);
                    let face = |(user_id, name): (Option<String>, String)| Face { user_id, name };
                    let first = faces.first().cloned().map(face);
                    if is_one_on_one(chat) || faces.len() < 2 {
                        let name = first
                            .as_ref()
                            .map_or(current.title.as_str(), |face| face.name.as_str());
                        let user_id = first.as_ref().and_then(|face| face.user_id.as_deref());
                        let avatar = person_avatar(directory, user_id, name, 36.);
                        let presence = user_id.map(|id| directory.presence_of(id));
                        lead = Some(match presence {
                            Some(kind) => with_presence(avatar, kind, 36., theme::background())
                                .into_any_element(),
                            None => avatar,
                        });
                        subline = presence
                            .map(|presence| presence.kind().label().to_owned())
                            .unwrap_or_default();
                    } else {
                        let pair = AvatarSpec::Pair(face(faces[0].clone()), face(faces[1].clone()));
                        lead = Some(spec_avatar(directory, &pair, 36., theme::background()));
                        subline = format!("{} participants", chat.members.len());
                        let shown: Vec<Face> = faces.iter().cloned().map(face).collect();
                        stack = Some(member_stack(directory, &shown, shown.len()));
                    }
                }
            }
            Selection::Channel(channel_id) => {
                let team = app.sidebar.teams.iter().find(|entry| {
                    entry
                        .channels
                        .iter()
                        .any(|channel| &channel.id == channel_id)
                });
                let (name, key) = team
                    .map_or((current.title.as_str(), current.title.as_str()), |entry| {
                        (entry.team.name.as_str(), entry.team.id.as_str())
                    });
                lead = Some(square_avatar(name, key, 36., 9.).into_any_element());
            }
        }
        let mut header = h_flex()
            .w_full()
            .h(px(60.))
            .flex_none()
            .px(px(20.))
            .gap(px(12.))
            .items_center()
            .border_b_1()
            .border_color(theme::border());
        if matches!(current.mode, ViewMode::Thread(_)) {
            header = header.child(
                Button::new("back-to-threads")
                    .ghost()
                    .compact()
                    .label("Threads")
                    .on_click(cx.listener(|this, _, _, cx| this.back_to_threads(cx))),
            );
        }
        let channel_note = match current.mode {
            ViewMode::Flat => None,
            ViewMode::ThreadList => Some("Threads"),
            ViewMode::Thread(_) => Some("Thread"),
        };
        if let Some(note) = channel_note {
            subline = note.to_owned();
        }
        let sync = &current.sync;
        if sync.failed {
            subline = "Not up to date".to_owned();
        } else if sync.shown_at.is_some() {
            subline = if sync.slow {
                "Taking longer than usual ..."
            } else if sync.empty_cache {
                "Loading messages ..."
            } else {
                "Updating ..."
            }
            .to_owned();
        }
        header
            .relative()
            .when(sync.shown_at.is_some(), |header| {
                header.child(progress_bar())
            })
            .children(lead)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap(px(1.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(current.title.clone()),
                    )
                    .when(!subline.is_empty(), |column| {
                        column.child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(subline),
                        )
                    }),
            )
            .children(stack)
    }
}

impl Render for ConversationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = theme::background();
        let mut root = v_flex().flex_1().min_w_0().h_full().bg(background);
        let drafting = self.draft_active;
        if self.current.is_none() && !drafting {
            return root
                .items_center()
                .justify_center()
                .text_color(theme::text_muted())
                .child("Select a chat or channel")
                .into_any_element();
        }
        let rows = self.rows.clone();
        let view = cx.weak_entity();
        let app = self.app.clone();
        let highlighted = self.highlighted_message.clone();
        let scroller = MessageScroller::new(
            "messages",
            self.scroller.clone(),
            move |index, window, cx| {
                let rows = rows.borrow();
                match rows.get(index) {
                    Some(Row::Message(message)) => {
                        let open_thread = message.open_thread.clone().map(|root_id| {
                            let view = view.clone();
                            Box::new(move |cx: &mut App| {
                                let root_id = root_id.clone();
                                view.update(cx, |this, cx| this.open_thread(root_id, cx))
                                    .ok();
                            }) as Box<dyn Fn(&mut App)>
                        });
                        let retry = matches!(message.delivery, Delivery::Failed(_)).then(|| {
                            let (view, key, handle) =
                                (view.clone(), message.key.clone(), window.window_handle());
                            Box::new(move |cx: &mut App| {
                                let (view, key) = (view.clone(), key.clone());
                                handle
                                    .update(cx, move |_, window, cx| {
                                        view.update(cx, |this, cx| this.retry(&key, window, cx))
                                            .ok();
                                    })
                                    .ok();
                            }) as Box<dyn Fn(&mut App)>
                        });
                        let senders: Vec<String> = (!message.own && !message.series.has_prev)
                            .then(|| message.sender_id.clone())
                            .flatten()
                            .into_iter()
                            .chain(
                                message
                                    .reactions
                                    .iter()
                                    .flat_map(|chip| chip.reactors.iter())
                                    .filter_map(|reactor| reactor.user_id.clone()),
                            )
                            .filter(|sender| app.read(cx).directory.avatar(sender).is_none())
                            .collect();
                        if !senders.is_empty() {
                            let app = app.clone();
                            cx.defer(move |cx| {
                                app.update(cx, |state, cx| state.request_avatars(senders, cx));
                            });
                        }
                        let missing_images: Vec<ImageRef> = message
                            .images
                            .iter()
                            .filter(|image| app.read(cx).directory.image(&image.url).is_none())
                            .cloned()
                            .collect();
                        if !missing_images.is_empty() {
                            let app = app.clone();
                            cx.defer(move |cx| {
                                app.update(cx, |state, cx| {
                                    state.request_images(missing_images, cx)
                                });
                            });
                        }
                        let is_real = !drafting && !message.key.starts_with(PENDING_KEY_PREFIX);
                        let hovered = is_real.then(|| {
                            let (view, key) = (view.clone(), message.key.clone());
                            Rc::new(move |hovered: bool, cx: &mut App| {
                                view.update(cx, |this, cx| {
                                    this.set_hover(HoverSlot::Row, &key, hovered, cx)
                                })
                                .ok();
                            }) as Rc<dyn Fn(bool, &mut App)>
                        });
                        let actionable = is_real && !message.deleted && !message.card;
                        let (menu, react, reaction_controls) = view
                            .upgrade()
                            .filter(|_| actionable)
                            .map(|entity| {
                                let this = entity.read(cx);
                                let menu =
                                    this.message_menu(&message.key, message.own, view.clone());
                                let react = menu.react.clone();
                                let visible = this.toolbar_visible(&message.key);
                                let controls = (!message.reactions.is_empty())
                                    .then(|| this.reaction_controls(&message.key, view.clone()));
                                (visible.then_some(menu), Some(react), controls)
                            })
                            .unwrap_or((None, None, None));
                        let reply = (is_real && !message.deleted).then(|| {
                            let (view, key, handle) =
                                (view.clone(), message.key.clone(), window.window_handle());
                            Box::new(move |cx: &mut App| {
                                let (view, key) = (view.clone(), key.clone());
                                handle
                                    .update(cx, move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.begin_reply(&key, window, cx)
                                        })
                                        .ok();
                                    })
                                    .ok();
                            }) as Box<dyn Fn(&mut App)>
                        });
                        let files = (is_real && !message.deleted && !message.files.is_empty())
                            .then(|| view.upgrade())
                            .flatten()
                            .map(|entity| {
                                entity.read(cx).file_actions(
                                    &message.key,
                                    &message.files,
                                    view.clone(),
                                )
                            });
                        let state = app.read(cx);
                        render_message_row(
                            message,
                            index,
                            RowActions {
                                open_thread,
                                retry,
                                reply,
                                hovered,
                                menu,
                                react,
                                reaction_controls,
                                files,
                                highlighted: highlighted.as_deref() == Some(message.key.as_str()),
                            },
                            &state.directory,
                            cx,
                        )
                    }
                    Some(Row::Skeleton(skeleton)) => render_skeleton_row(skeleton, index),
                    Some(Row::LoadOlder) => {
                        let view = view.clone();
                        cx.defer(move |cx| {
                            view.update(cx, |this, cx| this.request_older(cx)).ok();
                        });
                        h_flex()
                            .w_full()
                            .py_2()
                            .gap(px(6.))
                            .justify_center()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child(icon(IconName::Loader, 12., theme::text_muted()))
                            .child("Loading older messages")
                            .into_any_element()
                    }
                    Some(Row::Start(start)) => {
                        let state = app.read(cx);
                        v_flex()
                            .w_full()
                            .items_center()
                            .gap(px(8.))
                            .pt(px(32.))
                            .pb(px(16.))
                            .child(person_avatar(
                                &state.directory,
                                start.user_id.as_deref(),
                                &start.title,
                                56.,
                            ))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child(start.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child("This is the beginning of the conversation"),
                            )
                            .into_any_element()
                    }
                    None => div().into_any_element(),
                }
            },
        )
        .with_row_style(StyleRefinement::default().px(px(0.)).pb(px(0.)))
        .with_bottom_fade(background);

        root = match self.current.as_ref().filter(|_| !drafting) {
            Some(current) => root.child(self.render_header(current, cx)),
            None => root.child(self.draft.clone()),
        };
        let sync_failed = self
            .current
            .as_ref()
            .is_some_and(|current| current.sync.failed);
        if let Some(notice) = &self.notice {
            root = root.child(
                h_flex()
                    .px_4()
                    .py_1()
                    .gap(px(8.))
                    .items_center()
                    .text_xs()
                    .bg(theme::amber().opacity(0.15))
                    .text_color(theme::amber())
                    .child(div().flex_1().min_w_0().child(notice.clone()))
                    .when(sync_failed, |bar| {
                        bar.child(
                            Button::new("retry-fetch")
                                .ghost()
                                .compact()
                                .label("Retry")
                                .on_click(cx.listener(|this, _, _, cx| this.retry_fetch(cx))),
                        )
                    }),
            );
        }
        let empty_channel = self
            .current
            .as_ref()
            .is_some_and(|current| current.mode == ViewMode::ThreadList)
            && self.rows.borrow().is_empty();
        let body = if drafting && self.current.is_none() && self.rows.borrow().is_empty() {
            let draft = self.draft.read(cx);
            let group_name = draft.group_name(cx);
            draft.render_empty(&self.app.read(cx).directory, &group_name)
        } else if empty_channel {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(theme::text_muted())
                .child("No posts in this channel yet")
                .into_any_element()
        } else {
            scroller.into_any_element()
        };
        root.child(
            v_flex()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .on_scroll_wheel(
                            cx.listener(|this, _, _, cx| this.close_reaction_details(cx)),
                        )
                        .child(body),
                )
                .children(self.render_draft_error(cx))
                .child(self.composer.clone())
                .child(self.render_drop_overlay(cx)),
        )
        .capture_action(cx.listener(|this, _: &Escape, _, cx| {
            if this.reaction_details.is_some() {
                this.close_reaction_details(cx);
                cx.stop_propagation();
            }
        }))
        .when(drafting, |root| {
            root.on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.draft.read(cx).has_focus(window, cx) {
                    this.draft.update(cx, |draft, cx| draft.escape(cx));
                }
            }))
        })
        .into_any_element()
    }
}

fn progress_bar() -> impl IntoElement {
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(-1.))
        .h(px(2.))
        .overflow_hidden()
        .bg(theme::border())
        .child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(PROGRESS_BAR_WIDTH))
                .bg(theme::own_read())
                .with_animation(
                    "sync-progress",
                    Animation::new(PROGRESS_BAR_PERIOD).repeat(),
                    |bar, delta| {
                        let travel = 1. + PROGRESS_BAR_WIDTH;
                        // The 0.3 phase keeps the bar on screen in the static reduce-motion frame (delta 0).
                        bar.left(relative(
                            (delta + 0.3).fract() * travel - PROGRESS_BAR_WIDTH,
                        ))
                    },
                ),
        )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{DropTarget, drop_overlay_text};

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
    use store::Store;

    use super::ConversationView;
    use crate::app_state::{AppState, Mode, Selection};

    #[test]
    fn chat_overlay_names_the_chat_and_the_onedrive() {
        assert_eq!(
            drop_overlay_text("Mara Lin", DropTarget::Chat, 3),
            (
                "Drop to attach to Mara Lin".to_owned(),
                "3 files. Images go into the message, other files to your OneDrive.".to_owned()
            )
        );
    }

    #[test]
    fn channel_overlay_points_to_the_channel_files() {
        let (_, detail) = drop_overlay_text("Squad / General", DropTarget::Channel, 1);
        assert_eq!(
            detail,
            "1 file. Images go into the message, other files to the channel's Files."
        );
    }

    #[test]
    fn new_chat_overlay_says_files_wait_for_the_chat() {
        let (heading, detail) = drop_overlay_text("", DropTarget::NewChat, 2);
        assert_eq!(heading, "Drop to attach to the new chat");
        assert_eq!(
            detail,
            "2 files. Images go into the message. Files can be added once the chat exists."
        );
    }

    #[gpui_kit::test]
    fn demo_draft_creates_a_chat_and_selects_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        crate::demo::seed(&store);
        let mode = Mode {
            demo: true,
            read_only: false,
            demo_sync: None,
        };
        let (handle, app, _view) = cx.update(|cx| {
            let app = cx.new(|_| {
                let mut state = AppState::new(store, mode);
                crate::demo::seed_directory(&mut state);
                state.selection = Some(crate::demo::first_selection());
                state
            });
            let shared = app.clone();
            let (handle, view) =
                gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                    cx.new(|cx| ConversationView::new(shared, window, cx))
                })
                .unwrap();
            (handle, app, view)
        });
        cx.update(|cx| app.update(cx, |state, cx| state.start_new_chat(cx)));
        let step = |cx: &mut TestAppContext, run: fn(&mut gpui_kit::Window, &mut gpui_kit::App)| {
            cx.update_window(handle, move |_, window, cx| {
                run(window, cx);
                window.render_frame(cx);
            })
            .unwrap();
        };
        step(cx, |window, cx| window.render_frame(cx));
        step(cx, |window, cx| window.input("lea", cx));
        step(cx, |window, cx| window.press("enter", cx));
        step(cx, |window, cx| window.press("enter", cx));
        step(cx, |window, cx| window.input("Hello Lea", cx));
        step(cx, |window, cx| window.press("enter", cx));
        cx.update(|cx| {
            let state = app.read(cx);
            assert!(!state.new_chat);
            assert_eq!(
                state.selection,
                Some(Selection::Chat("demo-chat-new-1".to_owned()))
            );
            let chat = state
                .sidebar
                .chats
                .iter()
                .find(|chat| chat.id == "demo-chat-new-1")
                .expect("new chat in the sidebar");
            assert_eq!(chat.last_message_preview.as_deref(), Some("Hello Lea"));
        });
    }

    #[gpui_kit::test]
    fn escape_closes_the_draft_and_keeps_the_selection(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        crate::demo::seed(&store);
        let mode = Mode {
            demo: true,
            read_only: false,
            demo_sync: None,
        };
        let (handle, app) = cx.update(|cx| {
            let app = cx.new(|_| {
                let mut state = AppState::new(store, mode);
                crate::demo::seed_directory(&mut state);
                state.selection = Some(crate::demo::first_selection());
                state
            });
            let shared = app.clone();
            let (handle, _) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ConversationView::new(shared, window, cx))
            })
            .unwrap();
            (handle, app)
        });
        cx.update(|cx| app.update(cx, |state, cx| state.start_new_chat(cx)));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
            window.render_frame(cx);
            window.press("escape", cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.update(|cx| {
            let state = app.read(cx);
            assert!(!state.new_chat);
            assert_eq!(state.selection, Some(crate::demo::first_selection()));
        });
    }
}
