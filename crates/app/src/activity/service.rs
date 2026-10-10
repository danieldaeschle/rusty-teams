use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use gpui_kit::*;

use super::feed::{Actor, Entry, Feed, RETENTION_DAYS, ReactedMessage, start_time};
use super::preview_label;
use super::reactions::reacted_messages;
use crate::app_state::{AppEvent, AppState};
use crate::call::MissedCall;
use crate::demo;
use crate::notify::{IncomingTracker, channel_alerts};
use crate::people::resolve_names;

const WATERMARK_KEY: &str = "activity_watermark";
const RECENT_WINDOW: usize = 20;

pub struct ActivityCenter {
    app: Entity<AppState>,
    main_window: AnyWindowHandle,
    feed: Feed,
    tracker: IncomingTracker,
    cutoff: DateTime<Utc>,
    _subscription: Subscription,
}

impl ActivityCenter {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = app.read(cx);
        let store = state.store.clone();
        let now = Utc::now();
        let _ = store.prune_activity(now - Duration::days(RETENTION_DAYS));
        let cutoff = start_time(store.meta_time(WATERMARK_KEY).ok().flatten(), now);
        let mut feed = Feed::load(store.activity().unwrap_or_default());
        if state.mode.demo {
            for entry in demo::activity_entries() {
                feed.add(entry);
            }
        }
        let subscription = cx.subscribe(&app, |this, _, event: &AppEvent, cx| {
            this.on_app_event(event, cx);
        });
        let mut center = ActivityCenter {
            app,
            main_window: window.window_handle(),
            feed,
            tracker: IncomingTracker::new(cutoff),
            cutoff,
            _subscription: subscription,
        };
        center.flush(cx);
        center
    }

    pub fn feed(&self) -> &Feed {
        &self.feed
    }

    pub fn mark_read(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.feed.mark_read(id) {
            self.flush(cx);
        }
    }

    pub fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        if self.feed.mark_all_read() {
            self.flush(cx);
        }
    }

    fn on_app_event(&mut self, event: &AppEvent, cx: &mut Context<Self>) {
        match event {
            AppEvent::Messages(conversation_id) => self.on_messages(conversation_id, cx),
            AppEvent::Selection => self.on_selection(cx),
            AppEvent::Sidebar => self.on_sidebar(cx),
            AppEvent::MissedCall(missed) => self.on_missed_call(missed, cx),
            _ => {}
        }
    }

    fn on_missed_call(&mut self, missed: &MissedCall, cx: &mut Context<Self>) {
        let conversation_id = self.app.read(cx).conversation_of_missed_call(missed).unwrap_or_default();
        let actor = Actor {
            user_id: missed.caller_mri.strip_prefix("8:orgid:").map(str::to_owned),
            name: missed.caller_name.clone(),
        };
        self.feed.record_missed_call(&conversation_id, actor, missed.at);
        self.flush(cx);
    }

    fn on_selection(&mut self, cx: &mut Context<Self>) {
        let selected = self
            .app
            .read(cx)
            .selection
            .as_ref()
            .map(|selection| selection.conversation_id().to_owned());
        if let Some(conversation_id) = selected
            && self.feed.mark_conversation_read(&conversation_id)
        {
            self.flush(cx);
        }
    }

    fn on_sidebar(&mut self, cx: &mut Context<Self>) {
        self.record_offline_chats(cx);
        let sidebar = &self.app.read(cx).sidebar;
        let read_at: HashMap<&str, DateTime<Utc>> = sidebar
            .chats
            .iter()
            .filter(|chat| !chat.unread)
            .filter_map(|chat| Some((chat.id.as_str(), chat.last_read_at?)))
            .collect();
        if self
            .feed
            .sync_read(|conversation_id| read_at.get(conversation_id).copied())
        {
            self.flush(cx);
        }
    }

    fn record_offline_chats(&mut self, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let my_user_id = state.directory.me.as_ref().map(|me| me.user_id.as_str());
        let selected = state
            .selection
            .as_ref()
            .map(|selection| selection.conversation_id());
        for chat in state.sidebar.chats.iter().filter(|chat| chat.unread) {
            let (Some(at), Some(preview)) = (chat.last_message_at, &chat.last_message_preview)
            else {
                continue;
            };
            let from_me =
                my_user_id.is_some() && chat.last_message_sender_id.as_deref() == my_user_id;
            if at <= self.cutoff
                || from_me
                || chat.last_message_deleted
                || selected == Some(chat.id.as_str())
            {
                continue;
            }
            let actor = Actor {
                user_id: chat.last_message_sender_id.clone(),
                name: chat.last_message_sender_name.clone().unwrap_or_default(),
            };
            self.feed
                .record_chat_preview(&chat.id, actor, preview.clone(), at);
        }
    }

    fn on_messages(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        let foreground = self.in_foreground(conversation_id, cx);
        let state = self.app.read(cx);
        let my_user_id = state.directory.me.as_ref().map(|me| me.user_id.clone());
        let found = self.tracker.collect(
            &state.store,
            &state.sidebar,
            my_user_id.as_deref(),
            conversation_id,
        );
        let reacted = my_user_id
            .as_deref()
            .map(|me| reacted_in(state, conversation_id, me))
            .unwrap_or_default();
        let _ = state.store.set_meta_time(WATERMARK_KEY, Utc::now());
        if !foreground {
            for incoming in found.iter().filter(|incoming| channel_alerts(incoming)) {
                self.feed
                    .record_message(incoming, preview_label(&incoming.preview));
            }
        }
        if let Some(me) = my_user_id {
            for message in &reacted {
                self.feed
                    .record_reactions(conversation_id, message, &me, self.cutoff, Utc::now());
            }
        }
        if foreground {
            self.feed.mark_conversation_read(conversation_id);
        }
        self.flush(cx);
    }

    fn in_foreground(&self, conversation_id: &str, cx: &mut Context<Self>) -> bool {
        let selected = self
            .app
            .read(cx)
            .selection
            .as_ref()
            .is_some_and(|selection| selection.conversation_id() == conversation_id);
        selected
            && self
                .main_window
                .update(cx, |_, window, _| window.is_window_active())
                .unwrap_or(false)
    }

    fn flush(&mut self, cx: &mut Context<Self>) {
        let dirty = self.feed.take_dirty();
        if !dirty.is_empty() {
            let records: Vec<_> = dirty.iter().map(Entry::to_record).collect();
            let _ = self.app.read(cx).store.upsert_activity(&records);
            let user_ids: Vec<String> = dirty
                .iter()
                .filter_map(|entry| entry.latest_actor()?.user_id.clone())
                .collect();
            self.app
                .update(cx, |state, cx| state.request_avatars(user_ids, cx));
        }
        cx.notify();
    }
}

fn reacted_in(state: &AppState, conversation_id: &str, my_user_id: &str) -> Vec<ReactedMessage> {
    let Ok(records) = state.store.messages(conversation_id, None, RECENT_WINDOW) else {
        return Vec::new();
    };
    let reactor_ids: Vec<String> = records
        .iter()
        .flat_map(teams_core::reactions)
        .filter(|reaction| reaction.user_name.is_none())
        .filter_map(|reaction| reaction.user_id)
        .collect();
    let names = resolve_names(state, &reactor_ids);
    reacted_messages(&records, my_user_id, |user_id| names.get(user_id).cloned())
}
