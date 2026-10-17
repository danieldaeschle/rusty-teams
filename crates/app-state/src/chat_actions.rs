use std::future::Future;

use chatsvc::HistoryShare;
use chrono::{DateTime, Duration, Local, Offset, Utc};
use gpui_kit::*;
use store::{ChannelNotifications, ChatRecord, MemberRecord};
use teams_core::roster_event::{RosterChange, RosterEvent, SharedHistory, merge_members};

use crate::app_state::{AppState, Selection, chat_title};
use crate::notice::{NoticeAction, short_error, truncated};
use crate::sidebar_model::{
    Section, SectionInput, Step, build_sections, next_chat_id, step_id, visible_chat_ids,
};

pub const TITLE_LIMIT: usize = 40;

impl AppState {
    pub fn keeps_unread(&self, chat_id: &str) -> bool {
        self.keep_unread.as_deref() == Some(chat_id)
    }

    pub fn mark_chat_unread(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        self.mark_chat_unread_from(chat_id, None, cx);
    }

    pub fn mark_chat_unread_from(
        &mut self,
        chat_id: &str,
        message_created_at: Option<DateTime<Utc>>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(last_message_at) = chat.last_message_at else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let last_read_at =
            message_created_at.unwrap_or(last_message_at) - Duration::milliseconds(1);
        let _ = self.store.mark_chat_unread(chat_id, last_read_at);
        if self.selection == Some(Selection::Chat(chat_id.to_owned())) {
            self.keep_unread = Some(chat_id.to_owned());
        }
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        let revert_id = owned_id.clone();
        let restored_read_at = chat.last_read_at.unwrap_or(last_message_at);
        self.run_chat_action(
            "Mark as unread",
            async move {
                match message_created_at {
                    Some(created_at) => engine.mark_unread_from(&owned_id, created_at).await,
                    None => engine.mark_unread(&owned_id).await,
                }
            },
            move |state, cx| {
                let _ = state.store.mark_chat_read(&revert_id, restored_read_at);
                if state.keep_unread.as_deref() == Some(revert_id.as_str()) {
                    state.keep_unread = None;
                }
                state.reload_sidebar(cx);
            },
            |_, _| {},
            cx,
        );
    }

    pub fn set_chat_muted(&mut self, chat_id: &str, muted: bool, cx: &mut Context<Self>) {
        if self.chat_record(chat_id).is_none() {
            return;
        }
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let _ = self.store.set_chat_muted(chat_id, muted);
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        let revert_id = owned_id.clone();
        self.run_chat_action(
            if muted { "Mute" } else { "Unmute" },
            async move { engine.set_chat_muted(&owned_id, muted).await },
            move |state, cx| {
                let _ = state.store.set_chat_muted(&revert_id, !muted);
                state.reload_sidebar(cx);
            },
            |_, _| {},
            cx,
        );
    }

    pub fn set_channel_notifications(
        &mut self,
        channel_id: &str,
        notifications: ChannelNotifications,
        cx: &mut Context<Self>,
    ) {
        let Some(previous) = self.channel_notifications(channel_id) else {
            return;
        };
        if previous == notifications {
            return;
        }
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let _ = self
            .store
            .set_channel_notifications(channel_id, notifications);
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = channel_id.to_owned();
        let revert_id = owned_id.clone();
        self.run_chat_action(
            "Change notifications",
            async move {
                engine
                    .set_channel_notifications(&owned_id, notifications)
                    .await
            },
            move |state, cx| {
                let _ = state.store.set_channel_notifications(&revert_id, previous);
                state.reload_sidebar(cx);
            },
            |_, _| {},
            cx,
        );
    }

    pub fn channel_notifications(&self, channel_id: &str) -> Option<ChannelNotifications> {
        self.sidebar.teams.iter().find_map(|team| {
            team.channels
                .iter()
                .any(|channel| channel.id == channel_id)
                .then(|| team.channel_notifications(channel_id))
        })
    }

    pub fn hide_chat(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        self.remove_chat_locally(chat_id, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        let undo_id = owned_id.clone();
        let text = format!("\"{}\" hidden", truncated(&chat_title(&chat), TITLE_LIMIT));
        self.run_chat_action(
            "Hide",
            async move { engine.hide_chat(&owned_id).await },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            move |state, cx| {
                let undo =
                    NoticeAction::new("Undo", move |state, cx| state.unhide_chat(&undo_id, cx));
                state.raise_notice(text, Some(undo), cx);
            },
            cx,
        );
    }

    pub fn unhide_chat(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(engine) = self.chat_action_engine(cx).flatten() else {
            return;
        };
        let owned_id = chat_id.to_owned();
        self.run_chat_action(
            "Undo",
            async move { engine.unhide_chat(&owned_id).await },
            |_, _| {},
            |_, _| {},
            cx,
        );
    }

    pub fn leave_chat(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        self.remove_chat_locally(chat_id, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        self.run_chat_action(
            "Leave chat",
            async move { engine.leave_chat(&owned_id).await },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            |_, _| {},
            cx,
        );
    }

    pub fn remove_chat_member(&mut self, chat_id: &str, user_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let mut without_member = chat.clone();
        without_member
            .members
            .retain(|member| member.user_id.as_deref() != Some(user_id));
        let _ = self
            .store
            .upsert_chats(std::slice::from_ref(&without_member));
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let (owned_chat_id, owned_user_id) = (chat_id.to_owned(), user_id.to_owned());
        self.run_chat_action(
            "Remove from chat",
            async move {
                engine
                    .remove_chat_member(&owned_chat_id, &owned_user_id)
                    .await
            },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            |_, _| {},
            cx,
        );
    }

    pub fn add_chat_members(
        &mut self,
        chat_id: &str,
        people: Vec<MemberRecord>,
        share: HistoryShare,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let mut with_members = chat.clone();
        merge_members(&mut with_members.members, &people);
        let _ = self.store.upsert_chats(std::slice::from_ref(&with_members));
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            self.add_demo_roster_line(chat_id, &people, share, cx);
            return;
        };
        let owned_chat_id = chat_id.to_owned();
        self.run_chat_action(
            "Add people",
            async move {
                engine
                    .add_chat_members(&owned_chat_id, &people, share)
                    .await
            },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            |_, _| {},
            cx,
        );
    }

    fn add_demo_roster_line(
        &mut self,
        chat_id: &str,
        people: &[MemberRecord],
        share: HistoryShare,
        cx: &mut Context<Self>,
    ) {
        let me = self.directory.me.as_ref();
        let event = RosterEvent {
            change: RosterChange::Added,
            initiator_id: me.map(|person| person.user_id.clone()),
            initiator_name: me.map(|person| person.display_name.clone()),
            member_ids: people
                .iter()
                .filter_map(|person| person.user_id.clone())
                .collect(),
            member_names: people
                .iter()
                .map(|person| Some(person.display_name.clone()))
                .collect(),
            history: match share {
                HistoryShare::None => SharedHistory::None,
                HistoryShare::Days(days) => SharedHistory::Days(days),
                HistoryShare::All => SharedHistory::All,
            },
            topic: None,
        };
        self.store_demo_roster_line(chat_id, &event, cx);
    }

    fn store_demo_roster_line(
        &mut self,
        chat_id: &str,
        event: &RosterEvent,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        let record = crate::demo::roster_record(
            chat_id,
            &format!("demo-roster-{}", now.timestamp_millis()),
            now,
            event,
        );
        let _ = self.store.upsert_messages(std::slice::from_ref(&record));
        cx.emit(crate::app_state::AppEvent::Messages(chat_id.to_owned()));
        cx.notify();
    }

    /// Teams' "Remove chat history": hidden for me only until a newer message arrives.
    pub fn remove_chat_history(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        self.remove_chat_locally(chat_id, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = chat_id.to_owned();
        self.run_chat_action(
            "Remove chat history",
            async move { engine.remove_chat_history(&owned_id).await },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            |_, _| {},
            cx,
        );
    }

    pub fn rename_chat(&mut self, chat_id: &str, topic: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_record(chat_id) else {
            return;
        };
        let topic = topic.trim().to_owned();
        if topic == chat.title.trim() || chatsvc::topic_problem(&topic).is_some() {
            return;
        }
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let mut renamed = chat.clone();
        renamed.title = topic.clone();
        let _ = self.store.upsert_chats(std::slice::from_ref(&renamed));
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            let me = self.directory.me.as_ref();
            let event = RosterEvent {
                change: RosterChange::Renamed,
                initiator_id: me.map(|person| person.user_id.clone()),
                initiator_name: me.map(|person| person.display_name.clone()),
                member_ids: Vec::new(),
                member_names: Vec::new(),
                history: SharedHistory::None,
                topic: (!topic.is_empty()).then(|| topic.clone()),
            };
            self.store_demo_roster_line(chat_id, &event, cx);
            return;
        };
        let owned_id = chat_id.to_owned();
        self.run_chat_action(
            "Rename group chat",
            async move { engine.rename_chat(&owned_id, &topic).await },
            move |state, cx| state.restore_chat_locally(&chat, cx),
            |_, _| {},
            cx,
        );
    }

    /// Teams turns adding people to a 1:1 into a new group chat draft with everyone.
    pub fn start_group_chat_with(&mut self, people: Vec<MemberRecord>, cx: &mut Context<Self>) {
        self.new_chat_people = people;
        self.start_new_chat(cx);
    }

    fn chat_record(&self, chat_id: &str) -> Option<ChatRecord> {
        self.sidebar
            .chats
            .iter()
            .find(|chat| chat.id == chat_id)
            .cloned()
    }

    /// `None` stops the action. `Some(None)` is demo mode, which only changes the local store.
    pub fn chat_action_engine(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Option<std::sync::Arc<crate::backend::Engine>>> {
        if self.mode.read_only {
            self.raise_notice("Read-only mode: chat not changed".to_owned(), None, cx);
            return None;
        }
        match self.engine.clone() {
            Some(engine) => Some(Some(engine)),
            None if self.mode.demo => Some(None),
            None => None,
        }
    }

    fn remove_chat_locally(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let was_selected = self.selection == Some(Selection::Chat(chat_id.to_owned()));
        let next = was_selected
            .then(|| self.next_chat_after(chat_id))
            .flatten();
        let _ = self.store.remove_chats(&[chat_id.to_owned()]);
        self.reload_sidebar(cx);
        if was_selected {
            self.leave_removed_selection(next, cx);
        }
    }

    fn leave_removed_selection(&mut self, next: Option<String>, cx: &mut Context<Self>) {
        match next {
            Some(next_id) => self.select(Selection::Chat(next_id), cx),
            None => {
                self.selection = None;
                self.keep_unread = None;
                cx.emit(crate::app_state::AppEvent::Selection);
                cx.notify();
            }
        }
    }

    pub fn reload_sidebar_after_sync(&mut self, cx: &mut Context<Self>) {
        let selected = match &self.selection {
            Some(Selection::Chat(chat_id)) => Some(chat_id.clone()),
            _ => None,
        };
        let is_listed =
            |state: &Self, chat_id: &str| state.sidebar.chats.iter().any(|chat| chat.id == chat_id);
        let was_listed = selected
            .as_deref()
            .is_some_and(|chat_id| is_listed(self, chat_id));
        let next = selected
            .as_deref()
            .filter(|_| was_listed)
            .and_then(|chat_id| self.next_chat_after(chat_id));
        self.reload_sidebar(cx);
        let removed = was_listed
            && selected
                .as_deref()
                .is_some_and(|chat_id| !is_listed(self, chat_id));
        if removed {
            self.leave_removed_selection(next, cx);
        }
    }

    fn restore_chat_locally(&mut self, chat: &ChatRecord, cx: &mut Context<Self>) {
        let _ = self.store.upsert_chats(std::slice::from_ref(chat));
        let _ = self.store.set_chat_muted(&chat.id, chat.muted);
        self.reload_sidebar(cx);
    }

    fn next_chat_after(&self, chat_id: &str) -> Option<String> {
        next_chat_id(&self.chat_sections(), chat_id)
    }

    pub fn adjacent_chat_id(&self, step: Step) -> Option<String> {
        let current = match &self.selection {
            Some(Selection::Chat(chat_id)) => Some(chat_id.as_str()),
            _ => None,
        };
        step_id(&visible_chat_ids(&self.chat_sections()), current, step)
    }

    fn chat_sections(&self) -> Vec<Section> {
        let input = SectionInput {
            chats: &self.sidebar.chats,
            directory: &self.directory,
            collapsed: &self.collapsed,
            expanded_lists: &self.expanded_lists,
            typing: &self.typing,
            now: Utc::now(),
            offset: Local::now().offset().fix(),
        };
        build_sections(&input)
    }

    pub fn run_chat_action(
        &mut self,
        label: &'static str,
        call: impl Future<Output = teams_core::Result<()>> + Send + 'static,
        revert: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        on_success: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let receiver = crate::runtime::spawn(call);
        cx.spawn(async move |this, cx| {
            let failure = match receiver.await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(short_error(&error)),
                Err(_) => Some("cancelled".to_owned()),
            };
            this.update(cx, |state, cx| match failure {
                None => on_success(state, cx),
                Some(reason) => {
                    revert(state, cx);
                    state.raise_notice(format!("{label} failed: {reason}"), None, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}
