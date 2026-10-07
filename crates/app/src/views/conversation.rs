use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    message_scroller::{MessageScroller, MessageScrollerState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::MessageRecord;
use teams_core::ImageRef;

use super::avatar::{member_stack, person_avatar, spec_avatar, square_avatar, with_presence};
use super::composer::{Composer, ComposerEvent, Outgoing, ReplyPreview};
use super::message_row::{RowActions, render_message_row};
use super::widgets::icon;
use crate::app_state::{AppEvent, AppState, Selection, selection_title};
use crate::data::{is_one_on_one, others};
use crate::render::Block;
use crate::render::blocks::Inline;
use crate::rows::{
    Delivery, MessageRow, Row, RowContext, Series, StartInfo, assign_series, changed_indices,
    diff_keys, flat_rows, reply_excerpt, thread_list_rows, thread_rows,
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
const JUMP_SCAN_LIMIT: usize = 5000;
const JUMP_CONTEXT_MESSAGES: usize = 12;
const HIGHLIGHT_DURATION: Duration = Duration::from_secs(2);

actions!(teams, [ReplyToHovered]);

#[derive(Debug, Clone, PartialEq, Eq)]
enum ViewMode {
    Flat,
    ThreadList,
    Thread(String),
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
}

pub struct ConversationView {
    app: Entity<AppState>,
    scroller: Entity<MessageScrollerState>,
    composer: Entity<Composer>,
    rows: Rc<RefCell<Vec<Row>>>,
    current: Option<Current>,
    pending: Vec<MessageRow>,
    pending_counter: usize,
    pending_outgoing: HashMap<String, Outgoing>,
    hovered_message: Option<String>,
    highlighted_message: Option<String>,
    notice: Option<String>,
    _subscriptions: Vec<Subscription>,
}

fn open_limit(selection: &Selection) -> usize {
    match selection {
        Selection::Chat(_) => CHAT_OPEN_LIMIT,
        Selection::Channel(_) => CHANNEL_OPEN_LIMIT,
    }
}

fn short_error(error: &dyn std::fmt::Display) -> String {
    error.to_string().chars().take(MAX_NOTICE_CHARS).collect()
}

impl ConversationView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let composer = cx.new(|cx| Composer::new(app.clone(), window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&app, window, Self::on_app_event),
            cx.subscribe_in(&composer, window, Self::on_composer_event),
        ];
        let mut view = ConversationView {
            app,
            scroller,
            composer,
            rows: Rc::new(RefCell::new(Vec::new())),
            current: None,
            pending: Vec::new(),
            pending_counter: 0,
            pending_outgoing: HashMap::new(),
            hovered_message: None,
            highlighted_message: None,
            notice: None,
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

    fn on_app_event(
        &mut self,
        _: &Entity<AppState>,
        event: &AppEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            AppEvent::Selection => {
                if let Some(selection) = self.app.read(cx).selection.clone() {
                    self.open(selection, window, cx);
                }
            }
            AppEvent::Messages(conversation_id) => {
                let is_current = self
                    .current
                    .as_ref()
                    .is_some_and(|current| current.selection.conversation_id() == conversation_id);
                if is_current {
                    self.rebuild(false, cx);
                    self.mark_read(cx);
                }
            }
            AppEvent::Images(keys) => self.remeasure_images(keys, cx),
            AppEvent::Jump => self.apply_pending_jump(cx),
            AppEvent::Status => self.start_fetch(cx),
            AppEvent::Directory => cx.notify(),
            AppEvent::Sidebar => {
                if let Some(current) = self.current.as_mut() {
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
        let ComposerEvent::Submit(outgoing) = event;
        self.send(outgoing.clone(), window, cx);
    }

    fn open(&mut self, selection: Selection, window: &mut Window, cx: &mut Context<Self>) {
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
        });
        self.pending.clear();
        self.notice = None;
        self.rebuild(true, cx);
        self.start_fetch(cx);
        self.mark_read(cx);
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
            composer.focus(window, cx);
        });
        self.hovered_message = None;
        self.apply_pending_jump(cx);
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
        self.pending.clear();
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
                .unwrap_or_else(|| "Unbekannt".to_owned()),
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
        RowContext {
            offset: now.offset().fix(),
            today: now.date_naive(),
            my_user_id: self.app.read(cx).store.meta(META_USER_ID).ok().flatten(),
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
            self.apply_rows(Vec::new(), true, cx);
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
        let context = self.row_context(cx);
        let can_load_older = self.app.read(cx).engine.is_some();
        let start = self.start_info(cx);
        let current = self.current.as_mut().expect("checked above");
        let first_unread = current.first_unread.clone();
        current.loaded_limit = limit;
        current.loaded_count = records.len();
        let has_older = sync_has_more && !current.older_blocked && can_load_older;
        let mut rows = match &current.mode {
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
        rows.extend(
            self.pending
                .iter()
                .cloned()
                .map(|row| Row::Message(Box::new(row))),
        );
        let read_ids = self.read_message_ids(&records, cx);
        for row in rows.iter_mut() {
            if let Row::Message(message) = row {
                message.read = read_ids.contains(&message.key);
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
        let Some(engine) = self.app.read(cx).engine.clone() else {
            return;
        };
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.fetched {
            return;
        }
        current.fetched = true;
        let conversation_id = current.selection.conversation_id().to_owned();
        let receiver = runtime::spawn(async move { engine.fetch_newer(&conversation_id).await });
        cx.spawn(async move |this, cx| {
            if let Ok(Err(error)) = receiver.await {
                this.update(cx, |this, cx| {
                    this.notice = Some(format!("Could not refresh: {}", short_error(&error)));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        self.mark_read(cx);
        self.refresh_receipts(cx);
    }

    fn read_message_ids(
        &self,
        records: &[MessageRecord],
        cx: &App,
    ) -> std::collections::HashSet<String> {
        let state = self.app.read(cx);
        if state.mode.demo {
            return crate::demo::read_message_ids(records);
        }
        let Some(engine) = state.engine.as_ref() else {
            return Default::default();
        };
        records
            .iter()
            .filter(|record| {
                matches!(
                    engine.receipt_state_for(record),
                    teams_core::ReceiptState::Read { .. }
                )
            })
            .map(|record| record.message_id.clone())
            .collect()
    }

    fn refresh_receipts(&mut self, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let (Some(engine), Some(Selection::Chat(chat_id))) = (
            state.engine.clone(),
            self.current.as_ref().map(|c| c.selection.clone()),
        ) else {
            return;
        };
        drop(runtime::spawn(async move {
            engine.refresh_receipts(&chat_id).await
        }));
    }

    fn mark_read(&mut self, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let (Some(engine), Some(Selection::Chat(chat_id))) = (
            state.engine.clone(),
            self.current.as_ref().map(|c| c.selection.clone()),
        ) else {
            return;
        };
        if state.mode.read_only || state.mode.demo {
            return;
        }
        let unread = state
            .sidebar
            .chats
            .iter()
            .any(|chat| chat.id == chat_id && chat.unread);
        if !unread {
            return;
        }
        drop(runtime::spawn(
            async move { engine.mark_read(&chat_id).await },
        ));
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

    fn open_thread(&mut self, root_id: String, cx: &mut Context<Self>) {
        if let Some(current) = self.current.as_mut() {
            current.mode = ViewMode::Thread(root_id);
        }
        self.pending.clear();
        self.rebuild(true, cx);
    }

    fn back_to_threads(&mut self, cx: &mut Context<Self>) {
        if let Some(current) = self.current.as_mut() {
            current.mode = ViewMode::ThreadList;
        }
        self.pending.clear();
        self.rebuild(true, cx);
    }

    fn send(&mut self, outgoing: Outgoing, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let (mode, engine) = (state.mode, state.engine.clone());
        let Some(current) = self.current.as_ref() else {
            return;
        };
        let restore =
            |this: &mut Self, message: &str, window: &mut Window, cx: &mut Context<Self>| {
                this.notice = Some(message.to_owned());
                this.composer
                    .update(cx, |composer, cx| composer.restore(&outgoing, window, cx));
                cx.notify();
            };
        if mode.read_only || mode.demo {
            restore(self, "Read-only mode: nothing was sent", window, cx);
            return;
        }
        let Some(engine) = engine else {
            restore(self, "Not connected yet: nothing was sent", window, cx);
            return;
        };
        let conversation_id = current.selection.conversation_id().to_owned();
        let view_mode = current.mode.clone();
        self.pending_counter += 1;
        let key = format!("{PENDING_KEY_PREFIX}{}", self.pending_counter);
        let my_user_id = self.row_context(cx).my_user_id;
        self.pending_outgoing.insert(key.clone(), outgoing.clone());
        self.pending.push(MessageRow {
            key: key.clone(),
            author: "Du".to_owned(),
            sender_id: my_user_id,
            created_at: Utc::now(),
            series: Series::default(),
            card: false,
            time: chrono::Local::now().format("%H:%M").to_string(),
            day_header: None,
            blocks: vec![Block::Paragraph(Inline::plain(&outgoing.text))],
            edited: false,
            deleted: false,
            reactions: Vec::new(),
            images: Vec::new(),
            files: Vec::new(),
            reply_count: None,
            new_marker: false,
            reply_faces: Vec::new(),
            last_reply_time: None,
            open_thread: None,
            is_reply: false,
            delivery: Delivery::Sending,
            read: false,
            own: true,
        });
        self.notice = None;
        self.rebuild(false, cx);
        self.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_end(cx));

        let sent = outgoing.clone();
        let receiver = runtime::spawn(async move {
            let Outgoing {
                text,
                mentions,
                reply,
            } = &sent;
            match (&view_mode, reply) {
                (_, Some(reply)) => {
                    engine
                        .reply_to_with_mentions(&conversation_id, &reply.message_id, text, mentions)
                        .await
                }
                (ViewMode::Flat, None) => {
                    engine
                        .send_message_with_mentions(&conversation_id, text, None, mentions)
                        .await
                }
                (ViewMode::ThreadList, None) => {
                    engine
                        .post_to_channel_with_mentions(&conversation_id, text, None, mentions)
                        .await
                }
                (ViewMode::Thread(root_id), None) => {
                    engine
                        .send_message_with_mentions(&conversation_id, text, Some(root_id), mentions)
                        .await
                }
            }
            .map(|_| ())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            this.update_in(cx, |this, window, cx| {
                this.finish_send(&key, result, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn retry(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(outgoing) = self.pending_outgoing.remove(key) else {
            return;
        };
        self.pending.retain(|row| row.key != key);
        self.send(outgoing, window, cx);
    }

    fn finish_send(
        &mut self,
        key: &str,
        result: Result<teams_core::Result<()>, tokio::sync::oneshot::error::RecvError>,
        window: &mut Window,
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
                }
                self.notice = Some(format!("Not sent: {error}"));
                if self.composer.read(cx).is_empty(cx)
                    && let Some(outgoing) = self.pending_outgoing.get(key).cloned()
                {
                    self.composer
                        .update(cx, |composer, cx| composer.restore(&outgoing, window, cx));
                }
            }
        }
        self.rebuild(false, cx);
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
                            .map(|kind| kind.label().to_owned())
                            .unwrap_or_default();
                    } else {
                        let pair = AvatarSpec::Pair(face(faces[0].clone()), face(faces[1].clone()));
                        lead = Some(spec_avatar(directory, &pair, 36., theme::background()));
                        subline = format!("{} Teilnehmer", chat.members.len());
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
        header
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
        let Some(current) = self.current.as_ref() else {
            return root
                .items_center()
                .justify_center()
                .text_color(theme::text_muted())
                .child("Chat oder Channel auswählen")
                .into_any_element();
        };
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
                        let sender = (!message.own && !message.series.has_prev)
                            .then(|| message.sender_id.clone())
                            .flatten()
                            .filter(|sender| app.read(cx).directory.avatar(sender).is_none());
                        if let Some(sender) = sender {
                            let app = app.clone();
                            cx.defer(move |cx| {
                                app.update(cx, |state, cx| state.request_avatars(vec![sender], cx));
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
                        let is_real = !message.key.starts_with(PENDING_KEY_PREFIX);
                        let hovered = is_real.then(|| {
                            let (view, key) = (view.clone(), message.key.clone());
                            Box::new(move |cx: &mut App| {
                                view.update(cx, |this, _| {
                                    this.hovered_message = Some(key.clone());
                                })
                                .ok();
                            }) as Box<dyn Fn(&mut App)>
                        });
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
                        let state = app.read(cx);
                        render_message_row(
                            message,
                            index,
                            RowActions {
                                open_thread,
                                retry,
                                reply,
                                hovered,
                                highlighted: highlighted.as_deref() == Some(message.key.as_str()),
                            },
                            &state.directory,
                            cx,
                        )
                    }
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
                            .child("Ältere Nachrichten werden geladen")
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
                                    .child("Das ist der Anfang der Unterhaltung"),
                            )
                            .into_any_element()
                    }
                    None => div().into_any_element(),
                }
            },
        )
        .with_row_style(StyleRefinement::default().px(px(0.)).pb(px(0.)))
        .with_bottom_fade(background);

        root = root.child(self.render_header(current, cx));
        if let Some(notice) = &self.notice {
            root = root.child(
                div()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .bg(theme::amber().opacity(0.15))
                    .text_color(theme::amber())
                    .child(notice.clone()),
            );
        }
        let empty_channel = current.mode == ViewMode::ThreadList && self.rows.borrow().is_empty();
        let body = if empty_channel {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(theme::text_muted())
                .child("Noch keine Beiträge in diesem Channel")
                .into_any_element()
        } else {
            scroller.into_any_element()
        };
        root.child(div().flex_1().min_h_0().child(body))
            .child(self.composer.clone())
            .into_any_element()
    }
}
