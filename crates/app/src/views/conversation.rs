use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Escape, InputState},
    message_scroller::{MessageScroller, MessageScrollerState},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::{ChannelTabRecord, MessageRecord, OutboxState, OutboxTarget};
use teams_core::{ExecuteAction, FileCard, LinkPreview};

mod feed;
mod pins;
mod render_env;
mod scheduled;
mod shared;

use super::attachments::FileActions;
use super::avatar::{member_stack, person_avatar, spec_avatar, square_avatar, with_presence};
use super::call_controls::header_call_controls;
use super::channel_tabs::ChannelPane;
use super::composer::{Composer, ComposerEvent, EditPreview, Outgoing, ReplyPreview};
use super::message_actions::{Action, MessageMenu, QUICK_REACTION_COUNT};
use super::message_row::{render_message_row, render_skeleton_row};
use super::new_chat::{NewChatDraft, NewChatEvent, composer_placeholder, existing_one_on_one};
use super::post_card::render_post;
use super::profile_card::opens_profile;
use super::reaction_picker::{PickHandler, ReactionPicker};
use super::reaction_pills::{ReactionControls, ReactionPopover};
use super::widgets::{icon, symbol};
use crate::app_state::{AppEvent, AppState, Selection, selection_title};
use crate::backend::Engine;
use crate::card_state::CardScope;
use crate::channel_files::DownloadSource;
use crate::data::{is_one_on_one, others};
use crate::demo_library::DemoLibrary;
use crate::downloads::{self, ClickAction, DownloadKey, Downloads, PartFile, RevealTarget};
use crate::embedded_web::EmbeddedWeb;
use crate::emoji;
use crate::notice::short_error;
use crate::outbox::{deliver, new_outbox_id, outbox_record, outgoing_of};
use crate::pending_rows::pending_row;
use crate::people::{local_names, resolve_names};
use crate::reaction_model::UNKNOWN_REACTOR;
use crate::read_state::{ReadTrigger, plan_read};
use crate::rows::{
    Delivery, MessageRow, PendingReactions, Receipt, Row, RowContext, StartInfo, api_reaction,
    apply_pending_reactions, assign_series, changed_indices, diff_keys, flat_rows,
    has_own_reaction, message_draft, message_text, place_local_rows, placeholder_rows,
    reaction_glyph, reply_excerpt, set_own_reaction, thread_list_rows, thread_rows,
};
use crate::runtime;
use crate::translation::TranslationLine;
use crate::sidebar_model::{AvatarSpec, Face};
use crate::theme;
use crate::typing::typing_tooltip;
use feed::{FeedStash, subject_input};
use render_env::RenderEnv;
use shared::SharedState;

const CHAT_OPEN_LIMIT: usize = 60;
const CHANNEL_OPEN_LIMIT: usize = 300;
const GROWTH_HEADROOM: usize = 20;
const META_USER_ID: &str = "me_user_id";
const PENDING_KEY_PREFIX: &str = "pending-";
const DEMO_DOWNLOAD_STEPS: u8 = 10;
const DEMO_DOWNLOAD_STEP: Duration = Duration::from_millis(100);
const JUMP_SCAN_LIMIT: usize = 5000;
const JUMP_CONTEXT_MESSAGES: usize = 12;
const HIGHLIGHT_DURATION: Duration = Duration::from_secs(2);
const PROGRESS_DELAY: Duration = Duration::from_millis(400);
const SLOW_AFTER: Duration = Duration::from_secs(8);
const PROGRESS_MIN_VISIBLE: Duration = Duration::from_millis(500);
const PROGRESS_BAR_PERIOD: Duration = Duration::from_millis(1400);
const PROGRESS_BAR_WIDTH: f32 = 0.35;
const TYPING_LINE_HEIGHT: f32 = 22.;
const TYPING_DOT_SIZE: f32 = 5.;
const TYPING_DOT_COUNT: usize = 3;
const TYPING_PULSE_PERIOD: Duration = Duration::from_millis(1200);

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
    subject: Entity<InputState>,
    new_post_open: bool,
    reply_composer: Option<Entity<Composer>>,
    reply_root: Option<String>,
    feed_stash: Option<FeedStash>,
    feed_rows_seen: Rc<Cell<usize>>,
    feed_at_top: bool,
    draft: Entity<NewChatDraft>,
    draft_active: bool,
    draft_created: Option<(Vec<String>, String)>,
    draft_error: Option<String>,
    rows: Rc<RefCell<Vec<Row>>>,
    current: Option<Current>,
    pending: Vec<MessageRow>,
    pending_counter: usize,
    pending_outgoing: HashMap<String, Outgoing>,
    pending_reactions: PendingReactions,
    downloads: Downloads,
    hovered_message: Option<String>,
    toolbar_hovered: Option<String>,
    toolbar_pinned: Option<String>,
    toolbar_suppressed: Option<String>,
    recent: Entity<emoji::Recent>,
    picker: Entity<ReactionPicker>,
    reaction_details: Option<ReactionDetails>,
    highlighted_message: Option<String>,
    pin_index: usize,
    pin_previews: HashMap<String, pins::PinPreview>,
    notice: Option<String>,
    scheduled_failure: Option<scheduled::ScheduledFailure>,
    scheduled_in_flight: HashSet<String>,
    drag_file_count: usize,
    sync_generation: u64,
    window_active: bool,
    pane: ChannelPane,
    channel_tabs: Vec<ChannelTabRecord>,
    tab_links: HashMap<String, String>,
    web: Option<Entity<EmbeddedWeb>>,
    focus_handle: FocusHandle,
    web_subscription: Option<Subscription>,
    shared: SharedState,
    demo_library: DemoLibrary,
    new_folder_input: Option<Entity<InputState>>,
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
    Library,
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
        DropTarget::Library => (
            format!("Drop to upload to {title}"),
            format!("{files}. Files go into the folder you have open."),
        ),
        DropTarget::NewChat => (
            "Drop to attach to the new chat".to_owned(),
            format!(
                "{files}. Images go into the message. Files can be added once the chat exists."
            ),
        ),
    }
}

fn shown_in_view(mode: &ViewMode, record: &store::OutboxRecord) -> bool {
    match mode {
        ViewMode::Flat => record.target == OutboxTarget::Flat,
        ViewMode::ThreadList => matches!(record.target, OutboxTarget::Post | OutboxTarget::Thread),
        ViewMode::Thread(root_id) => {
            record.target == OutboxTarget::Thread
                && record.thread_root_id.as_deref() == Some(root_id.as_str())
        }
    }
}

enum DownloadEvent {
    Progress(u8),
    Finished(Result<PathBuf, String>),
}

async fn save_file(
    engine: &Engine,
    card: &FileCard,
    source: &DownloadSource,
    events: &tokio::sync::mpsc::UnboundedSender<DownloadEvent>,
) -> Result<PathBuf, String> {
    let mut part = PartFile::create(&downloads::downloads_directory(), &card.name)
        .map_err(|error| short_error(&error))?;
    let write = |bytes: &[u8]| part.write(bytes);
    let progress = |percent| {
        let _ = events.send(DownloadEvent::Progress(percent));
    };
    match source {
        DownloadSource::Share => engine.download_file(&card.open_url, write, progress).await,
        DownloadSource::Library(file) => engine.download_library_file(file, write, progress).await,
    }
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
                    this.activate_file(&conversation_id, &key, card, DownloadSource::Share, cx)
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
        source: DownloadSource,
        cx: &mut Context<Self>,
    ) {
        let key = DownloadKey::new(conversation_id, message_key, &card.open_url);
        match downloads::click_action(self.downloads.state(&key)) {
            ClickAction::Ignore => {}
            ClickAction::Start => self.start_download(key, card, source, cx),
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

    fn start_download(
        &mut self,
        key: DownloadKey,
        card: FileCard,
        source: DownloadSource,
        cx: &mut Context<Self>,
    ) {
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
            let outcome = save_file(&engine, &card, &source, &sender).await;
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
        let subject = subject_input(window, cx);
        let recent = cx.new(|cx| emoji::Recent::load(&app.read(cx).store));
        let picker = cx.new(|cx| ReactionPicker::new(app.clone(), recent.clone(), window, cx));
        let draft = cx.new(|cx| NewChatDraft::new(app.clone(), window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&app, window, Self::on_app_event),
            cx.subscribe_in(&composer, window, Self::on_composer_event),
            cx.subscribe_in(&draft, window, Self::on_draft_event),
            cx.subscribe_in(&subject, window, Self::on_subject_event),
            cx.observe_window_activation(window, Self::on_window_activation),
            cx.on_app_quit(|this, cx| {
                this.composer
                    .update(cx, |composer, cx| composer.save_draft_now(cx));
                async {}
            }),
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
            subject,
            new_post_open: false,
            reply_composer: None,
            reply_root: None,
            feed_stash: None,
            feed_rows_seen: Rc::new(Cell::new(usize::MAX)),
            feed_at_top: true,
            draft,
            draft_active: false,
            draft_created: None,
            draft_error: None,
            rows: Rc::new(RefCell::new(Vec::new())),
            current: None,
            pending: Vec::new(),
            pending_counter: 0,
            pending_outgoing: HashMap::new(),
            pending_reactions: HashMap::new(),
            downloads: Downloads::default(),
            hovered_message: None,
            toolbar_hovered: None,
            toolbar_pinned: None,
            toolbar_suppressed: None,
            recent,
            picker,
            reaction_details: None,
            highlighted_message: None,
            pin_index: 0,
            pin_previews: HashMap::new(),
            notice: None,
            scheduled_failure: None,
            scheduled_in_flight: HashSet::new(),
            drag_file_count: 0,
            sync_generation: 0,
            window_active: window.is_window_active(),
            pane: ChannelPane::Posts,
            channel_tabs: Vec::new(),
            tab_links: HashMap::new(),
            web: None,
            focus_handle: cx.focus_handle(),
            web_subscription: None,
            shared: SharedState::default(),
            demo_library: DemoLibrary::default(),
            new_folder_input: None,
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
                    if self.shared_active() {
                        self.refresh_shared_items(cx);
                    }
                    self.rebuild(false, cx);
                    self.mark_read(ReadTrigger::Incoming, cx);
                    self.on_pins_changed(conversation_id, cx);
                }
            }
            AppEvent::Images(keys) => self.remeasure_images(keys, cx),
            AppEvent::Cards(conversation_id) => self.refresh_cards(conversation_id, cx),
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
            AppEvent::TaskDialog
            | AppEvent::LocalPreviews
            | AppEvent::Forward
            | AppEvent::StatusMessage
            | AppEvent::NotificationSettings
            | AppEvent::Profile
            | AppEvent::Ring
            | AppEvent::MissedCall(_)
            | AppEvent::Call => {}
            AppEvent::LiveMeeting => cx.notify(),
            AppEvent::Saved => cx.notify(),
            AppEvent::Translation => self.rebuild(false, cx),
            AppEvent::Pins(conversation_id) => self.on_pins_changed(conversation_id, cx),
            AppEvent::Typing => cx.notify(),
            AppEvent::Scheduled => self.rebuild(false, cx),
            AppEvent::Outbox(conversation_id) => {
                let is_current = self
                    .current
                    .as_ref()
                    .is_some_and(|current| current.selection.conversation_id() == conversation_id);
                if is_current {
                    self.reload_pending(cx);
                    self.rebuild(false, cx);
                }
            }
            AppEvent::Sidebar => {
                self.reload_channel_tabs(cx);
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
            ComposerEvent::Submit(outgoing) => self.submit_main((**outgoing).clone(), window, cx),
            ComposerEvent::Schedule { outgoing, send_at } => {
                self.schedule((**outgoing).clone(), *send_at, window, cx)
            }
            ComposerEvent::EditLast => self.edit_last_own(window, cx),
            ComposerEvent::Typing(active) => self.send_typing(*active, cx),
        }
    }

    fn submit_main(&mut self, mut outgoing: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft_active {
            return self.send_new_chat(outgoing, window, cx);
        }
        let in_feed = self.in_feed();
        if in_feed && outgoing.edit.is_none() {
            outgoing.subject = self.take_subject(window, cx);
        }
        if in_feed {
            self.close_new_post(cx);
        }
        match &outgoing.edit {
            Some(edit) if edit.scheduled.is_some() => {
                self.send_scheduled_edit(outgoing, window, cx)
            }
            Some(_) => self.send_edit(outgoing, window, cx),
            None => self.send(outgoing, window, cx),
        }
    }

    fn typing_root(&self) -> Option<String> {
        match self.current.as_ref().map(|current| &current.mode) {
            Some(ViewMode::Thread(root_id)) => Some(root_id.clone()),
            _ => None,
        }
    }

    fn send_typing(&self, active: bool, cx: &App) {
        self.send_typing_in(self.typing_root(), active, cx);
    }

    fn send_typing_in(&self, thread_root_id: Option<String>, active: bool, cx: &App) {
        let state = self.app.read(cx);
        if state.mode.demo || state.mode.read_only || self.draft_active {
            return;
        }
        let (Some(engine), Some(conversation_id)) = (state.engine.clone(), self.conversation_id())
        else {
            return;
        };
        drop(runtime::spawn(async move {
            let sent = engine
                .send_typing(&conversation_id, thread_root_id.as_deref(), active)
                .await;
            if let Err(error) = sent
                && cfg!(debug_assertions)
            {
                eprintln!("typing indicator not sent: {error}");
            }
        }));
    }

    fn stop_typing(&mut self, cx: &mut Context<Self>) {
        if self
            .composer
            .update(cx, |composer, _| composer.stop_typing())
        {
            self.send_typing(false, cx);
        }
    }

    fn render_typing_line(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let faces = self
            .current
            .as_ref()
            .filter(|_| !self.draft_active)
            .map(|current| app.typing.faces(current.selection.conversation_id()))
            .unwrap_or_default();
        let tooltip_text = typing_tooltip(
            &faces
                .iter()
                .map(|face| face.name.clone())
                .collect::<Vec<_>>(),
        );
        h_flex()
            .h(px(TYPING_LINE_HEIGHT))
            .flex_none()
            .px(px(24.))
            .gap(px(6.))
            .items_center()
            .when(!faces.is_empty(), |line| {
                line.child(
                    div()
                        .id("typing-faces")
                        .tooltip(move |window, cx| {
                            Tooltip::new(tooltip_text.clone()).build(window, cx)
                        })
                        .child(member_stack(&app.directory, &faces, faces.len())),
                )
                .child(typing_dots())
            })
    }

    fn open(&mut self, selection: Selection, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_typing(cx);
        self.stop_reply_typing(cx);
        self.new_post_open = false;
        self.feed_stash = None;
        self.feed_at_top = true;
        self.restore_subject(None, window, cx);
        self.toolbar_hovered = None;
        self.toolbar_pinned = None;
        self.toolbar_suppressed = None;
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
        self.reset_channel_tabs(cx);
        self.reload_pending(cx);
        self.notice = None;
        self.scheduled_failure = None;
        self.app.update(cx, |state, cx| state.refresh_scheduled(cx));
        self.rebuild(true, cx);
        self.start_fetch(cx);
        self.mark_read(ReadTrigger::Open, cx);
        self.refresh_pin_previews(cx);
        if let Some(chat_id) = self.chat_id() {
            self.app
                .update(cx, |state, cx| state.refresh_pins(&chat_id, cx));
        }
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
        self.sync_reply_composer(window, cx);
        if !self.draft_active && !self.in_feed() {
            self.composer
                .update(cx, |composer, cx| composer.focus(window, cx));
        }
        self.hovered_message = None;
        self.apply_pending_jump(cx);
    }

    fn enter_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.draft_active {
            self.stop_typing(cx);
            self.draft_active = true;
            self.current = None;
            self.clear_pending();
            self.notice = None;
            self.draft_created = None;
            self.draft_error = None;
            self.toolbar_hovered = None;
            self.toolbar_pinned = None;
            self.toolbar_suppressed = None;
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
            .filter(|(_, row)| {
                row.messages().any(|message| {
                    message.images.iter().any(|image| keys.contains(&image.url))
                        || message
                            .link_preview
                            .as_ref()
                            .and_then(LinkPreview::image)
                            .is_some_and(|image| keys.contains(&image.url))
                })
            })
            .map(|(index, _)| index)
            .collect();
        self.scroller.update(cx, |scroller, cx| {
            for index in indices {
                scroller.remeasure_items(index..index + 1, cx);
            }
        });
        cx.notify();
    }

    fn refresh_cards(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        let is_current = self
            .current
            .as_ref()
            .is_some_and(|current| current.selection.conversation_id() == conversation_id);
        if !is_current {
            return;
        }
        self.rebuild(false, cx);
        let indices: Vec<usize> = self
            .rows
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.messages()
                    .any(|message| !message.adaptive_cards.is_empty())
            })
            .map(|(index, _)| index)
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
        let in_channel = matches!(current.selection, Selection::Channel(_));
        if in_channel {
            self.pane = ChannelPane::Posts;
            self.close_web();
            match record.reply_to_id.clone() {
                Some(root_id) => {
                    self.enter_thread_mode(root_id.clone(), cx);
                    self.refresh_thread(root_id, cx);
                }
                None => {
                    self.enter_feed_mode();
                }
            }
        }
        let in_thread = self
            .current
            .as_ref()
            .is_some_and(|current| matches!(current.mode, ViewMode::Thread(_)));
        let target_key = if in_thread {
            message_id.to_owned()
        } else {
            row_key
        };
        self.reload_pending(cx);
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
        let key = self
            .rows
            .borrow()
            .iter()
            .rev()
            .flat_map(Row::messages)
            .find(|message| !message.own)
            .map(|message| message.key.clone());
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
        if self.in_feed() {
            let root_id = record
                .reply_to_id
                .clone()
                .unwrap_or_else(|| record.message_id.clone());
            let quote = record.reply_to_id.is_some().then_some(preview);
            self.open_reply_editor(root_id, quote, window, cx);
            return;
        }
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
        let names = local_names(app);
        let card_overrides = self
            .current
            .as_ref()
            .map(|current| app.cards.overrides_for(current.selection.conversation_id()))
            .unwrap_or_default();
        let translation = self
            .current
            .as_ref()
            .map(|current| {
                app.translation
                    .context_for(current.selection.conversation_id())
            })
            .unwrap_or_default();
        RowContext {
            offset: now.offset().fix(),
            today: now.date_naive(),
            my_user_id: app.store.meta(META_USER_ID).ok().flatten(),
            names,
            pending_reactions: self.pending_reactions.clone(),
            card_overrides,
            translation,
        }
    }

    fn unknown_reactor_ids(&self) -> Vec<String> {
        self.rows
            .borrow()
            .iter()
            .flat_map(Row::messages)
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
        let names = resolve_names(self.app.read(cx), &unknown);
        if !names.is_empty() {
            self.rebuild(false, cx);
        }
    }

    fn drop_stale_reaction_details(&mut self) {
        let Some(details) = self.reaction_details.as_ref() else {
            return;
        };
        let present = self
            .rows
            .borrow()
            .iter()
            .flat_map(Row::messages)
            .any(|message| {
                message.key == details.message_key
                    && message
                        .reactions
                        .iter()
                        .any(|chip| chip.glyph() == details.anchor_glyph)
            });
        if !present {
            self.reaction_details = None;
        }
    }

    fn open_reaction_details(&mut self, message_key: &str, glyph: &str, cx: &mut Context<Self>) {
        let multiple_kinds = self
            .rows
            .borrow()
            .iter()
            .flat_map(Row::messages)
            .any(|message| message.key == message_key && message.reactions.len() > 1);
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
        let thread_root = match &current.mode {
            ViewMode::Thread(root_id) => Some(root_id.clone()),
            ViewMode::Flat | ViewMode::ThreadList => None,
        };
        let records: Vec<MessageRecord> = match &thread_root {
            Some(root_id) => store.thread_messages(&conversation_id, root_id),
            None => store.messages(&conversation_id, None, limit),
        }
        .unwrap_or_default();
        let sync_has_more = store
            .sync_state(&conversation_id)
            .ok()
            .flatten()
            .is_none_or(|state| state.has_more);
        let mut context = self.row_context(cx);
        let reactor_ids: Vec<String> = records
            .iter()
            .flat_map(teams_core::reactions)
            .filter(|reaction| reaction.user_name.is_none())
            .filter_map(|reaction| reaction.user_id)
            .filter(|user_id| !context.names.contains_key(user_id))
            .collect();
        for (user_id, name) in resolve_names(self.app.read(cx), &reactor_ids) {
            context.names.entry(user_id).or_insert(name);
        }
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
        if thread_root.is_none() {
            current.loaded_count = records.len();
        }
        let has_older = sync_has_more && !current.older_blocked && can_load_older;
        let placeholders = current.sync.messages
            && records.is_empty()
            && !matches!(current.mode, ViewMode::Thread(_));
        let in_feed = current.mode == ViewMode::ThreadList;
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
        let local: Vec<MessageRow> = self
            .pending
            .iter()
            .cloned()
            .chain(self.scheduled_rows(context.my_user_id.as_deref(), cx))
            .collect();
        if in_feed {
            place_local_rows(&mut rows, local);
        } else {
            rows.extend(local.into_iter().map(|row| Row::Message(Box::new(row))));
        }
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
        let in_feed = self.in_feed();
        if reset {
            let marker_index = self
                .rows
                .borrow()
                .iter()
                .position(|row| matches!(row, Row::Message(message) if message.new_marker));
            self.feed_at_top = true;
            self.scroller.update(cx, |scroller, cx| {
                scroller.reset(new_len, cx);
                if in_feed {
                    scroller.scroll_to_item(0, cx);
                    return;
                }
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
        let top_changed = splice
            .as_ref()
            .is_some_and(|splice| splice.range.start == 0);
        let keep_top = in_feed && (old_rows.is_empty() || (self.feed_at_top && top_changed));
        self.scroller.update(cx, |scroller, cx| {
            if let Some(splice) = splice
                && !scroller.splice(splice.range, splice.count, cx)
            {
                scroller.reset(new_len, cx);
            }
            if keep_top {
                scroller.scroll_to_item(0, cx);
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
        let newest_cached = self
            .rows
            .borrow()
            .iter()
            .flat_map(Row::messages)
            .filter(|message| !message.key.starts_with(PENDING_KEY_PREFIX))
            .map(|message| message.created_at)
            .max();
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
                (SLOW_AFTER, PROGRESS_DELAY),
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
        if state.viewing_call() {
            return;
        }
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
            .any(|chat| chat.id == chat_id && chat.unread)
            && !state.keeps_unread(&chat_id);
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

    fn outbox_route(&self, reply_root: Option<String>) -> (OutboxTarget, Option<String>) {
        if let Some(root_id) = reply_root {
            return (OutboxTarget::Thread, Some(root_id));
        }
        match self.current.as_ref().map(|current| &current.mode) {
            Some(ViewMode::Thread(root_id)) => (OutboxTarget::Thread, Some(root_id.clone())),
            Some(ViewMode::ThreadList) => (OutboxTarget::Post, None),
            _ => (OutboxTarget::Flat, None),
        }
    }

    fn reload_pending(&mut self, cx: &App) {
        self.clear_pending();
        let state = self.app.read(cx);
        if state.mode.demo || state.mode.read_only {
            return;
        }
        let Some(current) = self.current.as_ref() else {
            return;
        };
        let conversation_id = current.selection.conversation_id().to_owned();
        let Ok(records) = state.store.outbox_for_conversation(&conversation_id) else {
            return;
        };
        let my_user_id = state.store.meta(META_USER_ID).ok().flatten();
        let mut pending = Vec::new();
        for record in records {
            if !shown_in_view(&current.mode, &record) {
                continue;
            }
            let Some(outgoing) = outgoing_of(&record) else {
                continue;
            };
            let delivery = match record.state {
                OutboxState::Sending => Delivery::Sending,
                OutboxState::Failed => Delivery::Failed(record.last_error.unwrap_or_default()),
            };
            let mut row = pending_row(
                record.id.clone(),
                conversation_id.clone(),
                my_user_id.clone(),
                record.created_at,
                &outgoing,
                delivery,
            );
            row.reply_root = record.thread_root_id.clone();
            pending.push((row, record.id, outgoing));
        }
        for (row, id, outgoing) in pending {
            self.pending.push(row);
            self.pending_outgoing.insert(id, outgoing);
        }
    }

    fn discard_outbox_row(&self, id: &str, cx: &mut Context<Self>) {
        self.app.read(cx).store.delete_outbox(id).ok();
        self.app
            .update(cx, |state, cx| state.refresh_local_previews(cx));
    }

    fn delete_pending(&mut self, key: &str, cx: &mut Context<Self>) {
        self.pending.retain(|row| row.key != key);
        self.pending_outgoing.remove(key);
        self.discard_outbox_row(key, cx);
        self.rebuild(false, cx);
    }

    fn send(&mut self, outgoing: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        self.send_as(new_outbox_id(), outgoing, None, window, cx);
    }

    fn send_in_thread(
        &mut self,
        root_id: String,
        outgoing: Outgoing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.send_as(new_outbox_id(), outgoing, Some(root_id), window, cx);
    }

    fn send_as(
        &mut self,
        id: String,
        outgoing: Outgoing,
        reply_root: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.app.read(cx);
        let (mode, engine, store) = (state.mode, state.engine.clone(), state.store.clone());
        let Some(current) = self.current.as_ref() else {
            return;
        };
        if mode.read_only || mode.demo {
            self.restore_unsent(
                "Read-only mode: nothing was sent",
                &outgoing,
                reply_root.as_deref(),
                window,
                cx,
            );
            return;
        }
        let Some(engine) = engine else {
            self.discard_outbox_row(&id, cx);
            self.restore_unsent(
                "Not connected yet: nothing was sent",
                &outgoing,
                reply_root.as_deref(),
                window,
                cx,
            );
            return;
        };
        let conversation_id = current.selection.conversation_id().to_owned();
        let (target, thread_root_id) = self.outbox_route(reply_root);
        let created_at = Utc::now();
        self.push_pending(
            id.clone(),
            created_at,
            &outgoing,
            thread_root_id.clone(),
            cx,
        );
        if let Some(record) = outbox_record(
            &id,
            &conversation_id,
            target,
            thread_root_id.as_deref(),
            &outgoing,
            created_at,
        ) {
            store.put_outbox(&record).ok();
        }
        self.notice = None;
        self.rebuild(false, cx);
        if self.in_feed() {
            self.scroller
                .update(cx, |scroller, cx| scroller.scroll_to_item(0, cx));
        } else {
            self.scroller
                .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
        }

        let receiver = runtime::spawn(async move {
            deliver(
                &engine,
                &conversation_id,
                target,
                thread_root_id.as_deref(),
                &outgoing,
            )
            .await
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            this.update(cx, |this, cx| this.finish_send(&id, result, cx))
                .ok();
        })
        .detach();
    }

    fn restore_unsent(
        &mut self,
        message: &str,
        outgoing: &Outgoing,
        reply_root: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notice = Some(message.to_owned());
        if let (Some(root_id), true) = (reply_root, self.in_feed()) {
            self.open_reply_editor(root_id.to_owned(), outgoing.reply.clone(), window, cx);
            if let Some(composer) = self.reply_composer.clone() {
                composer.update(cx, |composer, cx| composer.restore(outgoing, window, cx));
            }
        } else {
            self.show_main_composer(cx);
            self.restore_subject(outgoing.subject.as_deref(), window, cx);
            self.composer
                .update(cx, |composer, cx| composer.restore(outgoing, window, cx));
        }
        cx.notify();
    }

    fn next_pending_key(&mut self) -> String {
        self.pending_counter += 1;
        format!("{PENDING_KEY_PREFIX}{}", self.pending_counter)
    }

    fn push_pending(
        &mut self,
        key: String,
        created_at: DateTime<Utc>,
        outgoing: &Outgoing,
        reply_root: Option<String>,
        cx: &App,
    ) {
        let my_user_id = self.row_context(cx).my_user_id;
        let conversation_id = self
            .current
            .as_ref()
            .map(|current| current.selection.conversation_id().to_owned())
            .unwrap_or_default();
        let mut row = pending_row(
            key.clone(),
            conversation_id,
            my_user_id,
            created_at,
            outgoing,
            Delivery::Sending,
        );
        row.reply_root = reply_root;
        self.pending.push(row);
        self.pending_outgoing.insert(key, outgoing.clone());
    }

    fn send_new_chat(&mut self, outgoing: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        let (recipients, topic) = {
            let draft = self.draft.read(cx);
            let topic = Some(draft.group_name(cx)).filter(|name| !name.is_empty());
            (draft.chips().to_vec(), topic)
        };
        if recipients.is_empty() {
            self.restore_unsent("Add at least one person first", &outgoing, None, window, cx);
            self.draft
                .update(cx, |draft, cx| draft.focus_query(window, cx));
            return;
        }
        if self
            .pending
            .iter()
            .any(|row| matches!(row.delivery, Delivery::Sending))
        {
            self.restore_unsent("Still creating the chat", &outgoing, None, window, cx);
            return;
        }
        let state = self.app.read(cx);
        let (mode, engine) = (state.mode, state.engine.clone());
        if mode.read_only {
            self.restore_unsent(
                "Read-only mode: nothing was sent",
                &outgoing,
                None,
                window,
                cx,
            );
            return;
        }
        if mode.demo && outgoing.has_attachments() {
            self.restore_unsent(
                "Read-only mode: nothing was sent",
                &outgoing,
                None,
                window,
                cx,
            );
            return;
        }
        if mode.demo {
            self.send_demo_new_chat(&outgoing, &recipients, topic.as_deref(), cx);
            return;
        }
        let Some(engine) = engine else {
            self.restore_unsent(
                "Not connected yet: nothing was sent",
                &outgoing,
                None,
                window,
                cx,
            );
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
        let key = self.next_pending_key();
        self.push_pending(key.clone(), Utc::now(), &outgoing, None, cx);
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
            if let (Ok(record), Some(preview)) = (&sent, &outgoing.link_preview) {
                let _ = engine
                    .attach_link_preview(&chat_id, &record.message_id, preview)
                    .await;
            }
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
        if let Some(suppressed) = self.toolbar_suppressed.as_deref()
            && self.hovered_message.as_deref() != Some(suppressed)
            && self.toolbar_hovered.as_deref() != Some(suppressed)
        {
            self.toolbar_suppressed = None;
        }
        cx.notify();
    }

    fn suppress_toolbar(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.toolbar_hovered.as_deref() == Some(key) {
            self.toolbar_hovered = None;
        }
        self.toolbar_suppressed = Some(key.to_owned());
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
        self.toolbar_suppressed.as_deref() != Some(key)
            && self
                .toolbar_pinned
                .as_deref()
                .or(self.toolbar_hovered.as_deref())
                .or(self.hovered_message.as_deref())
                == Some(key)
    }

    fn toggle_reaction(&mut self, message_id: &str, glyph: &str, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        let state = self.app.read(cx);
        let (mode, engine, store) = (state.mode, state.engine.clone(), state.store.clone());
        let engine = engine.filter(|_| !mode.read_only);
        if !mode.demo && engine.is_none() {
            self.show_notice("Read-only mode: reaction not sent", cx);
            return;
        }
        let my_user_id = self.row_context(cx).my_user_id;
        let Some(mut record) = store
            .messages_by_id(&conversation_id, &[message_id.to_owned()])
            .ok()
            .and_then(|mut found| found.remove(message_id))
        else {
            return;
        };
        let wanted = reaction_glyph(glyph);
        let mut visible = teams_core::reactions(&record);
        apply_pending_reactions(
            &mut visible,
            self.pending_reactions.get(message_id),
            my_user_id.as_deref(),
            Utc::now(),
        );
        let added = !has_own_reaction(&visible, glyph, my_user_id.as_deref());
        let Some(engine) = engine.filter(|_| !mode.demo) else {
            let mut reactions = teams_core::reactions(&record);
            set_own_reaction(
                &mut reactions,
                glyph,
                my_user_id.as_deref(),
                added,
                Utc::now(),
            );
            record.reactions_json = serde_json::to_string(&reactions).unwrap_or_default();
            let _ = store.upsert_messages(std::slice::from_ref(&record));
            if added {
                self.record_reaction_use(glyph, cx);
            }
            self.rebuild(false, cx);
            return;
        };
        let entries = self
            .pending_reactions
            .entry(message_id.to_owned())
            .or_default();
        entries.retain(|(pending, _)| *pending != wanted);
        entries.push((wanted.clone(), added));
        self.rebuild(false, cx);
        let (message_id, glyph) = (message_id.to_owned(), glyph.to_owned());
        let reaction_type = api_reaction(&wanted);
        let receiver = runtime::spawn({
            let (conversation_id, message_id) = (conversation_id.clone(), message_id.clone());
            async move {
                if added {
                    engine
                        .set_reaction(&conversation_id, &message_id, &reaction_type)
                        .await
                } else {
                    engine
                        .unset_reaction(&conversation_id, &message_id, &reaction_type)
                        .await
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(_) => Err("cancelled".to_owned()),
            };
            this.update(cx, |this, cx| {
                this.finish_reaction(&conversation_id, &message_id, &glyph, added, outcome, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_reaction(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        glyph: &str,
        added: bool,
        outcome: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let wanted = reaction_glyph(glyph);
        let store = self.app.read(cx).store.clone();
        if outcome.is_ok() {
            let my_user_id = self.row_context(cx).my_user_id;
            if let Some(mut record) = store
                .messages_by_id(conversation_id, &[message_id.to_owned()])
                .ok()
                .and_then(|mut found| found.remove(message_id))
            {
                let mut reactions = teams_core::reactions(&record);
                set_own_reaction(
                    &mut reactions,
                    glyph,
                    my_user_id.as_deref(),
                    added,
                    Utc::now(),
                );
                record.reactions_json = serde_json::to_string(&reactions).unwrap_or_default();
                let _ = store.upsert_messages(std::slice::from_ref(&record));
            }
            if added {
                self.record_reaction_use(glyph, cx);
            }
        }
        if let Some(entries) = self.pending_reactions.get_mut(message_id) {
            entries.retain(|entry| *entry != (wanted.clone(), added));
            if entries.is_empty() {
                self.pending_reactions.remove(message_id);
            }
        }
        self.rebuild(false, cx);
        if let Err(error) = outcome {
            self.show_notice(&format!("Reaction failed: {error}"), cx);
        }
    }

    fn record_reaction_use(&mut self, glyph: &str, cx: &mut Context<Self>) {
        let store = self.app.read(cx).store.clone();
        self.recent.update(cx, |recent, cx| {
            recent.reload(&store);
            recent.record_use(glyph);
            recent.save(&store);
            cx.notify();
        });
    }

    fn begin_edit(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(record) = self.own_record(message_id, cx) else {
            return;
        };
        let edit = EditPreview {
            message_id: record.message_id.clone(),
            excerpt: reply_excerpt(&record),
            scheduled: None,
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
            subject: None,
            mentions,
            reply: None,
            edit: Some(edit),
            images: Vec::new(),
            files: Vec::new(),
            link_preview: None,
        };
        self.show_main_composer(cx);
        self.composer
            .update(cx, |composer, cx| composer.begin_edit(draft, window, cx));
    }

    fn edit_last_own(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft_active {
            return;
        }
        let key = self
            .rows
            .borrow()
            .iter()
            .flat_map(Row::messages)
            .filter(|message| {
                message.own && !message.deleted && !message.key.starts_with(PENDING_KEY_PREFIX)
            })
            .max_by_key(|message| message.created_at)
            .map(|message| message.key.clone());
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
            self.show_main_composer(cx);
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
                    this.show_main_composer(cx);
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

    fn refresh_card(
        &mut self,
        key: &str,
        card_index: usize,
        action: ExecuteAction,
        cx: &mut Context<Self>,
    ) {
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        let scope = CardScope::message(&conversation_id, key, card_index);
        self.app.update(cx, |state, cx| {
            state.refresh_card_manually(scope, action, cx)
        });
    }

    pub(super) fn translate_message(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        self.app.update(cx, |state, cx| {
            state.translate_message(&conversation_id, key, cx)
        });
    }

    fn show_notice(&mut self, message: &str, cx: &mut Context<Self>) {
        self.app.update(cx, |state, cx| {
            state.raise_notice(message.to_owned(), None, cx)
        });
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

    fn message_menu(
        &self,
        message: &MessageRow,
        mine: HashSet<String>,
        refresh: Option<(usize, ExecuteAction)>,
        view: WeakEntity<Self>,
        cx: &App,
    ) -> MessageMenu {
        let (key, own) = (message.key.as_str(), message.own);
        let translate_label = match message.translation {
            Some(TranslationLine::Translated {
                showing_original: false,
                ..
            }) => "See original",
            Some(TranslationLine::Translated { .. }) => "See translation",
            _ => "Translate",
        };
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
        let refresh_card = refresh.map(|(card_index, refresh_action)| {
            let (view, key) = (view.clone(), key.to_owned());
            Rc::new(move |_: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| {
                    this.refresh_card(&key, card_index, refresh_action.clone(), cx)
                })
                .ok();
            }) as Action
        });
        let chat_id = self.chat_id();
        let state = self.app.read(cx);
        let saved = self
            .conversation_id()
            .is_some_and(|conversation_id| state.is_saved(&conversation_id, key));
        let pinned = chat_id
            .as_deref()
            .is_some_and(|chat_id| state.is_pinned(chat_id, key));
        let in_chat = chat_id.is_some();
        MessageMenu {
            key: key.to_owned(),
            react,
            reply: Some(action(|this, key, window, cx| {
                this.begin_reply(key, window, cx)
            })),
            forward: Some(action(|this, key, _, cx| this.forward_message(key, cx))),
            copy_link: Some(action(|this, key, _, cx| this.copy_link(key, cx))),
            copy: action(|this, key, _, cx| this.copy_text(key, cx)),
            translate: Some(action(|this, key, _, cx| this.translate_message(key, cx))),
            translate_label,
            save: Some(action(|this, key, _, cx| this.toggle_saved(key, cx))),
            saved,
            toggle_pinned: in_chat.then(|| action(|this, key, _, cx| this.toggle_pinned(key, cx))),
            pinned,
            edit: own.then(|| action(|this, key, window, cx| this.begin_edit(key, window, cx))),
            delete: own.then(|| action(|this, key, _, cx| this.delete(key, cx))),
            mark_unread: (in_chat && !own)
                .then(|| action(|this, key, _, cx| this.mark_unread_from(key, cx))),
            refresh_card,
            pin,
            hover,
            picker: self.picker.clone(),
            quick: self.recent.read(cx).most_used(QUICK_REACTION_COUNT),
            mine,
            done: action(|this, key, _, cx| this.suppress_toolbar(key, cx)),
        }
    }

    fn retry(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(outgoing) = self.pending_outgoing.remove(key) else {
            return;
        };
        let reply_root = self
            .pending
            .iter()
            .find(|row| row.key == key)
            .and_then(|row| row.reply_root.clone());
        self.pending.retain(|row| row.key != key);
        if self.draft_active {
            self.send_new_chat(outgoing, window, cx);
        } else {
            self.send_as(key.to_owned(), outgoing, reply_root, window, cx);
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
        let store = self.app.read(cx).store.clone();
        match failure {
            None => {
                store.delete_outbox(key).ok();
                self.pending.retain(|row| row.key != key);
                self.pending_outgoing.remove(key);
            }
            Some(error) => {
                store.mark_outbox_failed(key, &error).ok();
                if let Some(row) = self.pending.iter_mut().find(|row| row.key == key) {
                    row.delivery = Delivery::Failed(error.clone());
                    self.notice = Some(format!("Not sent: {error}"));
                }
            }
        }
        self.app
            .update(cx, |state, cx| state.refresh_local_previews(cx));
        self.rebuild(false, cx);
    }

    fn render_drop_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let target = match self.current.as_ref().map(|current| &current.selection) {
            Some(Selection::Chat(_)) => DropTarget::Chat,
            Some(Selection::Channel(_)) if self.shared_active() => DropTarget::Library,
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
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                let paths = dropped.paths().to_vec();
                if this.shared_active() {
                    this.upload_paths(paths, cx);
                    return;
                }
                this.composer
                    .update(cx, |composer, cx| composer.add_paths(paths, window, cx));
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
                        let avatar = match presence {
                            Some(kind) => with_presence(avatar, kind, 36., theme::background())
                                .into_any_element(),
                            None => avatar,
                        };
                        lead = Some(
                            opens_profile(div().id("header-avatar").flex_none(), user_id)
                                .child(avatar)
                                .into_any_element(),
                        );
                        subline = match (user_id, presence) {
                            (Some(id), Some(presence)) => {
                                directory.status_label(id, presence).to_owned()
                            }
                            _ => String::new(),
                        };
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
                h_flex()
                    .id("back-to-channel")
                    .flex_none()
                    .gap(px(2.))
                    .pl(px(4.))
                    .pr(px(8.))
                    .py(px(4.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::accent_text())
                    .hover(|button| button.bg(theme::row_hover()))
                    .on_click(cx.listener(|this, _, _, cx| this.back_to_threads(cx)))
                    .child(icon(IconName::ChevronLeft, 16., theme::accent_text()))
                    .child("Back to channel"),
            );
        }
        let channel_note = match current.mode {
            ViewMode::Flat => None,
            ViewMode::ThreadList => Some("Posts"),
            ViewMode::Thread(_) => Some("Conversation"),
        };
        if let Some(note) = channel_note {
            subline = note.to_owned();
        }
        let call_controls = header_call_controls(&self.app, app, &current.selection);
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
            .children(call_controls)
    }
}

impl Render for ConversationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.web.is_some() && !self.web_active() {
            self.close_web();
        }
        let background = theme::background();
        let mut root = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(background)
            .track_focus(&self.focus_handle);
        let drafting = self.draft_active;
        if self.current.is_none() && !drafting {
            return root
                .items_center()
                .justify_center()
                .text_color(theme::text_muted())
                .child("Select a chat or channel")
                .into_any_element();
        }
        let in_feed = self.in_feed();
        let rows_seen = self.feed_rows_seen.replace(usize::MAX);
        if in_feed && rows_seen != usize::MAX {
            self.feed_at_top = rows_seen == 0;
        }
        let rows = self.rows.clone();
        let rows_seen = self.feed_rows_seen.clone();
        let env = RenderEnv {
            view: cx.weak_entity(),
            app: self.app.clone(),
            highlighted: self.highlighted_message.clone(),
            drafting,
            reply_editor: self.reply_root.clone().zip(self.reply_composer.clone()),
        };
        let scroller = MessageScroller::new(
            "messages",
            self.scroller.clone(),
            move |index, window, cx| {
                rows_seen.set(rows_seen.get().min(index));
                let rows = rows.borrow();
                match rows.get(index) {
                    Some(Row::Message(message)) => {
                        let actions = env.message_actions(message, window, cx);
                        let state = env.app.read(cx);
                        render_message_row(message, index, actions, &state.directory, cx)
                    }
                    Some(Row::Post(post)) => {
                        let actions = env.post_actions(post, window, cx);
                        let state = env.app.read(cx);
                        render_post(post, index, actions, &state.directory, cx)
                    }
                    Some(Row::Skeleton(skeleton)) => render_skeleton_row(skeleton, index),
                    Some(Row::LoadOlder) => {
                        let view = env.view.clone();
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
                        let state = env.app.read(cx);
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
        .jump_button(!in_feed)
        .with_bottom_fade(background);

        root = match self.current.as_ref().filter(|_| !drafting) {
            Some(current) => root.child(self.render_header(current, cx)),
            None => root.child(self.draft.clone()),
        };
        root = root.children(self.render_tab_bar(window, cx).filter(|_| !drafting));
        root = root.children(self.render_pin_banner(cx).filter(|_| !drafting));
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
        let empty_channel = in_feed && self.rows.borrow().is_empty();
        let new_post = self.render_new_post(window, cx);
        let body = if drafting && self.current.is_none() && self.rows.borrow().is_empty() {
            let draft = self.draft.read(cx);
            let group_name = draft.group_name(cx);
            draft.render_empty(&self.app.read(cx).directory, &group_name)
        } else if self.shared_active() {
            self.render_shared(cx)
        } else if let Some(web) = self.web.clone().filter(|_| self.web_active()) {
            web.into_any_element()
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
                .children(new_post)
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
                .when(!in_feed, |column| {
                    column
                        .child(self.render_typing_line(cx))
                        .child(self.composer.clone())
                })
                .child(self.render_drop_overlay(cx)),
        )
        .capture_action(cx.listener(|this, _: &Escape, _, cx| {
            if this.reaction_details.is_some() {
                this.close_reaction_details(cx);
                cx.stop_propagation();
            }
        }))
        .on_action(cx.listener(|this, _: &Escape, window, cx| {
            if this.draft_active {
                if this.draft.read(cx).has_focus(window, cx) {
                    this.draft.update(cx, |draft, cx| draft.escape(cx));
                }
            } else if !this.on_escape(window, cx) {
                cx.propagate();
            }
        }))
        .into_any_element()
    }
}

fn typing_dots() -> impl IntoElement {
    h_flex()
        .gap(px(3.))
        .flex_none()
        .children((0..TYPING_DOT_COUNT).map(|index| {
            let phase_offset = index as f32 / TYPING_DOT_COUNT as f32;
            div()
                .size(px(TYPING_DOT_SIZE))
                .rounded_full()
                .bg(theme::text_muted())
                .with_animation(
                    ElementId::NamedInteger("typing-dot".into(), index as u64),
                    Animation::new(TYPING_PULSE_PERIOD).repeat(),
                    move |dot, delta| {
                        let wave = 1. - ((delta - phase_offset).rem_euclid(1.) * 2. - 1.).abs();
                        dot.opacity(0.3 + 0.7 * wave)
                    },
                )
        }))
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
