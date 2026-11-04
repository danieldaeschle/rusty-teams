use chrono::Utc;
use gpui_kit::*;
use teams_core::{ActivitySnapshot, stored_activity_feed};

use super::feed::{Entry, Feed};
use super::server::feed_of;
use crate::agent_api;
use crate::app_state::{AppEvent, AppState};
use crate::demo;

const SEEN_KEY: &str = "activity_seen_ms";

pub struct ActivityCenter {
    app: Entity<AppState>,
    main_window: AnyWindowHandle,
    snapshot: ActivitySnapshot,
    seen_locally_ms: i64,
    feed: Feed,
    _subscription: Subscription,
    _proposal_observation: Option<Subscription>,
}

impl ActivityCenter {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = app.read(cx);
        let snapshot = if state.mode.demo {
            demo::activity_snapshot()
        } else {
            stored_activity_feed(&state.store).unwrap_or_default()
        };
        let seen_locally_ms = state
            .store
            .meta(SEEN_KEY)
            .ok()
            .flatten()
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        let subscription = cx.subscribe(&app, |this, _, event: &AppEvent, cx| {
            this.on_app_event(event, cx);
        });
        let proposal_observation =
            agent_api::handle(cx).map(|api| cx.observe(&api, |this, _, cx| this.on_proposals(cx)));
        let feed = feed_of(&snapshot, seen_locally_ms);
        let center = ActivityCenter {
            app,
            main_window: window.window_handle(),
            snapshot,
            seen_locally_ms,
            feed,
            _subscription: subscription,
            _proposal_observation: proposal_observation,
        };
        center.request_avatars(cx);
        center
    }

    pub fn feed(&self) -> &Feed {
        &self.feed
    }

    pub fn mark_read(&mut self, id: &str, cx: &mut Context<Self>) {
        self.mark_read_where(|entry| entry.id == id, cx);
    }

    pub fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        agent_api::mark_all_proposals_seen(cx);
        if self.feed.unread_count() == 0 {
            return;
        }
        self.snapshot.mark_all_read(Utc::now().timestamp_millis());
        self.rebuild(cx);
        self.app
            .update(cx, |state, cx| state.mark_all_activity_read(cx));
    }

    /// Teams resets the bell when the feed is opened.
    pub fn mark_seen(&mut self, cx: &mut Context<Self>) {
        if self.feed.bell_count() == 0 {
            return;
        }
        self.seen_locally_ms = Utc::now().timestamp_millis();
        let _ = self
            .app
            .read(cx)
            .store
            .set_meta(SEEN_KEY, &self.seen_locally_ms.to_string());
        self.rebuild(cx);
    }

    fn mark_read_where(&mut self, matches: impl Fn(&Entry) -> bool, cx: &mut Context<Self>) {
        let message_ids: Vec<String> = self
            .feed
            .unread_entries()
            .filter(|entry| matches(entry))
            .map(|entry| entry.id.clone())
            .collect();
        if message_ids.is_empty() {
            return;
        }
        for message_id in &message_ids {
            self.snapshot.mark_read(message_id);
        }
        self.rebuild(cx);
        self.app
            .update(cx, |state, cx| state.mark_activity_read(message_ids, cx));
    }

    fn on_app_event(&mut self, event: &AppEvent, cx: &mut Context<Self>) {
        match event {
            AppEvent::Activity => self.reload(cx),
            AppEvent::Selection => self.on_selection(cx),
            AppEvent::Messages(conversation_id) => {
                if self.in_foreground(conversation_id, cx) {
                    let conversation_id = conversation_id.clone();
                    self.mark_read_where(|entry| entry.conversation_id == conversation_id, cx);
                }
            }
            _ => {}
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        if state.mode.demo {
            return;
        }
        let Some(snapshot) = stored_activity_feed(&state.store) else {
            return;
        };
        self.snapshot = snapshot;
        self.rebuild(cx);
        self.request_avatars(cx);
    }

    fn on_selection(&mut self, cx: &mut Context<Self>) {
        let Some(conversation_id) = self.selected_conversation(cx) else {
            return;
        };
        agent_api::mark_conversation_proposals_seen(&conversation_id, cx);
        self.mark_read_where(|entry| entry.conversation_id == conversation_id, cx);
    }

    fn on_proposals(&mut self, cx: &mut Context<Self>) {
        if let Some(conversation_id) = self.selected_conversation(cx)
            && self.in_foreground(&conversation_id, cx)
        {
            agent_api::mark_conversation_proposals_seen(&conversation_id, cx);
        }
        cx.notify();
    }

    fn selected_conversation(&self, cx: &App) -> Option<String> {
        self.app
            .read(cx)
            .selection
            .as_ref()
            .map(|selection| selection.conversation_id().to_owned())
    }

    fn in_foreground(&self, conversation_id: &str, cx: &mut Context<Self>) -> bool {
        self.selected_conversation(cx).as_deref() == Some(conversation_id)
            && self
                .main_window
                .update(cx, |_, window, _| window.is_window_active())
                .unwrap_or(false)
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.feed = feed_of(&self.snapshot, self.seen_locally_ms);
        cx.notify();
    }

    fn request_avatars(&self, cx: &mut Context<Self>) {
        let mut user_ids: Vec<String> = self
            .feed
            .entries()
            .iter()
            .filter_map(|entry| entry.latest_actor()?.user_id.clone())
            .collect();
        user_ids.sort();
        user_ids.dedup();
        if user_ids.is_empty() {
            return;
        }
        self.app
            .update(cx, |state, cx| state.request_avatars(user_ids, cx));
    }
}
