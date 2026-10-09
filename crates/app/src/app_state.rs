use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use chatsvc::TypingEvent;
use chrono::{DateTime, Utc};
use gpui_kit::*;
use store::{ChatRecord, Sidebar, Store};
use teams_core::{ChatApp, CoreEvent, ImageRef, ScheduledDraft};

use crate::backend::{BackendEvent, ConnectionState, Engine, LiveState};
use crate::card_state::{CardState, TaskDialogState};
use crate::data::{self, Directory};
use crate::local_previews::{LocalPreview, load_local_previews};
use crate::notice::Notice;
use crate::notify;
use crate::typing::TypingState;

const COLLAPSED_META_KEY: &str = "ui.collapsed_sections";
const RECENT_MESSAGES_FOR_TYPING: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Chat(String),
    Channel(String),
}

impl Selection {
    pub fn conversation_id(&self) -> &str {
        match self {
            Selection::Chat(id) | Selection::Channel(id) => id,
        }
    }
}

pub enum AppEvent {
    Sidebar,
    Selection,
    Messages(String),
    Images(Vec<String>),
    Jump,
    Status,
    Directory,
    Cards(String),
    TaskDialog,
    Typing,
    LocalPreviews,
    Outbox(String),
    Scheduled,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mode {
    pub demo: bool,
    pub read_only: bool,
    pub demo_sync: Option<std::time::Duration>,
}

pub struct AppState {
    pub store: Arc<Store>,
    pub engine: Option<Arc<Engine>>,
    pub sidebar: Sidebar,
    pub favorite_ids: Vec<String>,
    pub selection: Option<Selection>,
    pub new_chat: bool,
    pub connection: ConnectionState,
    pub live: LiveState,
    pub last_sync: Option<DateTime<Utc>>,
    pub mode: Mode,
    pub directory: Directory,
    pub collapsed: HashSet<String>,
    pub followed_channels: HashSet<String>,
    pub start_on_channels: bool,
    pub pending_jump: Option<(String, String)>,
    pub cards: CardState,
    pub task_dialog: Option<TaskDialogState>,
    pub bot_apps: HashMap<String, Vec<ChatApp>>,
    pub bot_apps_pending: HashSet<String>,
    pub notice: Option<Notice>,
    pub notice_count: u64,
    pub keep_unread: Option<String>,
    pub typing: TypingState,
    typing_timer_running: bool,
    pub local_previews: HashMap<String, LocalPreview>,
    pub scheduled: Vec<ScheduledDraft>,
    pub scheduled_polling: bool,
}

pub struct AppHandle(pub Entity<AppState>);

impl Global for AppHandle {}

impl EventEmitter<AppEvent> for AppState {}

pub fn chat_title(chat: &ChatRecord) -> String {
    if !chat.title.trim().is_empty() {
        chat.title.clone()
    } else if !chat.member_summary.trim().is_empty() {
        chat.member_summary.clone()
    } else {
        "Chat".to_owned()
    }
}

pub fn selection_title(sidebar: &Sidebar, selection: &Selection) -> String {
    match selection {
        Selection::Chat(id) => sidebar
            .chats
            .iter()
            .find(|chat| &chat.id == id)
            .map(chat_title)
            .unwrap_or_else(|| "Chat".to_owned()),
        Selection::Channel(id) => sidebar
            .teams
            .iter()
            .find_map(|team| {
                team.channels
                    .iter()
                    .find(|channel| &channel.id == id)
                    .map(|channel| format!("{} / {}", team.team.name, channel.name))
            })
            .unwrap_or_else(|| "Channel".to_owned()),
    }
}

impl AppState {
    pub fn new(store: Arc<Store>, mode: Mode) -> Self {
        let sidebar = store.sidebar().unwrap_or_default();
        let mut state = AppState {
            store,
            engine: None,
            sidebar,
            favorite_ids: Vec::new(),
            selection: None,
            new_chat: false,
            connection: if mode.demo {
                ConnectionState::Online
            } else {
                ConnectionState::Connecting
            },
            live: LiveState::Off,
            last_sync: None,
            mode,
            directory: Directory::default(),
            collapsed: HashSet::new(),
            followed_channels: HashSet::new(),
            start_on_channels: false,
            pending_jump: None,
            cards: CardState::default(),
            task_dialog: None,
            bot_apps: HashMap::new(),
            bot_apps_pending: HashSet::new(),
            notice: None,
            notice_count: 0,
            keep_unread: None,
            typing: TypingState::default(),
            typing_timer_running: false,
            local_previews: HashMap::new(),
            scheduled: Vec::new(),
            scheduled_polling: false,
        };
        state.local_previews = load_local_previews(&state.store);
        state.collapsed = state.load_collapsed();
        state.followed_channels = notify::load_followed_channels(&state.store);
        if !mode.demo {
            state.directory.load_cached_presence(&state.store);
            state.reload_directory();
        }
        state
    }

    fn load_collapsed(&self) -> HashSet<String> {
        self.store
            .meta(COLLAPSED_META_KEY)
            .ok()
            .flatten()
            .map(|value| {
                value
                    .lines()
                    .map(str::to_owned)
                    .filter(|id| !id.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn toggle_followed_channel(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        if !self.followed_channels.remove(channel_id) {
            self.followed_channels.insert(channel_id.to_owned());
        }
        notify::save_followed_channels(&self.store, &self.followed_channels);
        cx.notify();
    }

    pub fn toggle_collapsed(&mut self, section_id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(section_id) {
            self.collapsed.insert(section_id.to_owned());
        }
        let mut ids: Vec<&str> = self.collapsed.iter().map(String::as_str).collect();
        ids.sort_unstable();
        let _ = self.store.set_meta(COLLAPSED_META_KEY, &ids.join("\n"));
        cx.emit(AppEvent::Sidebar);
        cx.notify();
    }

    pub fn reload_directory(&mut self) {
        self.directory.me = data::me(&self.store);
        self.directory.folders = data::folders(&self.store, &self.favorite_ids);
        self.directory.pinned_channels = data::pinned_channels(&self.store);
        if self.directory.pinned_channels.is_empty() {
            let known: HashSet<&str> = self
                .sidebar
                .teams
                .iter()
                .flat_map(|team| team.channels.iter().map(|channel| channel.id.as_str()))
                .collect();
            self.directory.pinned_channels = self
                .favorite_ids
                .iter()
                .filter(|id| known.contains(id.as_str()))
                .cloned()
                .collect();
        }
        self.resolve_pending_avatars();
        self.sync_presence();
        self.directory.unread_counts = match &self.engine {
            Some(engine) => data::unread_counts(engine, &self.sidebar.chats),
            None => Default::default(),
        };
    }

    pub fn request_avatars(&mut self, user_ids: Vec<String>, cx: &mut Context<Self>) {
        let wanted = self.directory.missing_avatars(user_ids);
        if wanted.is_empty() {
            return;
        }
        let mut changed = false;
        let mut uncached = Vec::new();
        for user_id in wanted {
            match data::cached_avatar(&self.store, &user_id) {
                Some(cached) => {
                    self.directory.set_avatar(&user_id, cached);
                    changed = true;
                }
                None => uncached.push(user_id),
            }
        }
        if let Some(engine) = self.engine.clone().filter(|_| !uncached.is_empty()) {
            self.directory.mark_avatars_pending(&uncached);
            let done = data::fetch_avatars(&engine, uncached);
            self.on_done(done, cx, |state, cx| {
                state.resolve_pending_avatars();
                cx.emit(AppEvent::Directory);
                cx.notify();
            });
            changed = true;
        }
        if changed {
            cx.emit(AppEvent::Directory);
            cx.notify();
        }
    }

    pub fn request_images(&mut self, images: Vec<ImageRef>, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let wanted = self
            .directory
            .images_to_request(images.iter().map(|image| image.url.clone()));
        let mut changed = false;
        for image in images
            .into_iter()
            .filter(|image| wanted.contains(&image.url))
        {
            match engine.image_path(&image) {
                Some(path) => {
                    self.directory.set_image(&image.url, &path);
                    changed = true;
                }
                None => {
                    let done = data::fetch_image(&engine, image);
                    self.on_done(done, cx, |_, _| {});
                }
            }
        }
        if changed {
            cx.emit(AppEvent::Images(wanted));
            cx.notify();
        }
    }

    pub fn jump_to_message(
        &mut self,
        selection: Selection,
        message_id: String,
        cx: &mut Context<Self>,
    ) {
        self.pending_jump = Some((selection.conversation_id().to_owned(), message_id));
        if self.selection.as_ref() == Some(&selection) {
            cx.emit(AppEvent::Jump);
        } else {
            self.select(selection, cx);
        }
    }

    fn on_done(
        &self,
        done: data::Done,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        cx.spawn(async move |this, cx| {
            let _ = done.await;
            this.update(cx, |state, cx| then(state, cx)).ok();
        })
        .detach();
    }

    fn resolve_pending_avatars(&mut self) {
        for user_id in self.directory.pending_avatars() {
            if let Some(cached) = data::cached_avatar(&self.store, &user_id) {
                self.directory.set_avatar(&user_id, cached);
            }
        }
    }

    fn sync_presence(&mut self) -> Vec<String> {
        let Some(engine) = self.engine.clone() else {
            return Vec::new();
        };
        self.directory
            .requested_presence_ids()
            .into_iter()
            .filter(|user_id| {
                data::presence_kind(&engine, user_id)
                    .is_some_and(|kind| self.directory.set_presence(user_id, kind))
            })
            .collect()
    }

    pub fn request_presence(&mut self, user_ids: Vec<String>, cx: &mut Context<Self>) {
        let now = Instant::now();
        let pushed = self.live == LiveState::Live;
        let wanted = self.directory.stale_presence(user_ids, now, pushed);
        if wanted.is_empty() {
            return;
        }
        self.directory.mark_presence_pending(&wanted);
        if let Some(engine) = self.engine.clone() {
            self.directory.mark_presence_requested(&wanted, now);
            drop(data::watch_presence(&engine, wanted.clone()));
            let done = data::refresh_presence(&engine, wanted.clone());
            self.on_done(done, cx, move |state, cx| {
                state.sync_presence();
                state.directory.save_presence(&state.store, &wanted);
                state.directory.settle_presence(&wanted);
                cx.emit(AppEvent::Directory);
                cx.notify();
            });
        }
    }

    pub fn move_chat(
        &mut self,
        conversation_id: &str,
        folder_id: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if self.mode.read_only {
            return;
        }
        let previous_folder = self
            .directory
            .folder_of(conversation_id)
            .map(|folder| folder.id.clone());
        self.directory.assign(conversation_id, folder_id);
        if let Some(engine) = self.engine.clone() {
            let previous = previous_folder.clone();
            let done = match (folder_id, previous.as_deref()) {
                (Some(folder_id), _) => {
                    Some(data::move_to_folder(&engine, conversation_id, folder_id))
                }
                (None, Some(previous)) => {
                    Some(data::remove_from_folder(&engine, conversation_id, previous))
                }
                (None, None) => None,
            };
            if let Some(done) = done {
                self.on_done(done, cx, |state, cx| {
                    state.reload_directory();
                    cx.emit(AppEvent::Sidebar);
                    cx.notify();
                });
            }
        }
        cx.emit(AppEvent::Sidebar);
        cx.notify();
    }

    pub fn pin_chat(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        let favorites = self.directory.favorites().map(|folder| folder.id.clone());
        self.move_chat(conversation_id, favorites.as_deref(), cx);
    }

    pub fn mark_chat_read(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.keeps_unread(conversation_id) {
            self.keep_unread = None;
        }
        if self.mode.demo {
            if let Some(chat) = self
                .sidebar
                .chats
                .iter()
                .find(|chat| chat.id == conversation_id)
            {
                let mut read = chat.clone();
                read.unread = false;
                let _ = self.store.upsert_chats(&[read]);
                self.reload_sidebar(cx);
            }
            return;
        }
        if self.mode.read_only {
            return;
        }
        if let Some(engine) = self.engine.clone() {
            let _ = self
                .store
                .mark_chat_read(conversation_id, chrono::Utc::now());
            self.reload_sidebar(cx);
            let conversation_id = conversation_id.to_owned();
            drop(crate::runtime::spawn(async move {
                engine.mark_read(&conversation_id).await
            }));
        }
    }

    pub fn start_new_chat(&mut self, cx: &mut Context<Self>) {
        self.new_chat = true;
        cx.emit(AppEvent::Selection);
        cx.notify();
    }

    pub fn close_new_chat(&mut self, cx: &mut Context<Self>) {
        if !self.new_chat {
            return;
        }
        self.new_chat = false;
        cx.emit(AppEvent::Selection);
        cx.notify();
    }

    pub fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        let was_drafting = std::mem::take(&mut self.new_chat);
        if !was_drafting && self.selection.as_ref() == Some(&selection) {
            return;
        }
        self.selection = Some(selection);
        self.keep_unread = None;
        cx.emit(AppEvent::Selection);
        cx.notify();
    }

    fn apply_typing(&mut self, event: TypingEvent, cx: &mut Context<Self>) {
        let is_me = self
            .directory
            .me
            .as_ref()
            .is_some_and(|me| me.user_id == event.user_id);
        if is_me {
            return;
        }
        if event.active {
            let display_name = match event.display_name.trim() {
                "" => crate::people::resolve_names(self, std::slice::from_ref(&event.user_id))
                    .remove(&event.user_id)
                    .unwrap_or_default(),
                _ => event.display_name.clone(),
            };
            self.typing.start(
                &event.conversation_id,
                &event.user_id,
                &display_name,
                Instant::now(),
                event.received_at,
            );
            self.request_avatars(vec![event.user_id.clone()], cx);
            self.schedule_typing_expiry(cx);
        } else {
            self.typing.clear(&event.conversation_id, &event.user_id);
        }
        cx.emit(AppEvent::Typing);
        cx.notify();
    }

    fn clear_typists_who_sent(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if !self.typing.is_active(conversation_id) {
            return;
        }
        let Ok(recent) = self
            .store
            .messages(conversation_id, None, RECENT_MESSAGES_FOR_TYPING)
        else {
            return;
        };
        let mut changed = false;
        for message in recent {
            if let Some(sender_id) = message.sender_id.as_deref() {
                changed |= self
                    .typing
                    .message_from(conversation_id, sender_id, message.created_at);
            }
        }
        if changed {
            cx.emit(AppEvent::Typing);
            cx.notify();
        }
    }

    fn schedule_typing_expiry(&mut self, cx: &mut Context<Self>) {
        if self.typing_timer_running {
            return;
        }
        self.typing_timer_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                let next = this.update(cx, |state, _| {
                    let next = state.typing.next_expiry();
                    state.typing_timer_running = next.is_some();
                    next
                });
                let Ok(Some(next)) = next else { return };
                cx.background_executor()
                    .timer(next.saturating_duration_since(Instant::now()))
                    .await;
                let alive = this.update(cx, |state, cx| {
                    if state.typing.expire(Instant::now()) {
                        cx.emit(AppEvent::Typing);
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    pub fn reload_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Ok(sidebar) = self.store.sidebar() {
            self.sidebar = sidebar;
            self.apply_bot_titles();
        }
        if !self.mode.demo {
            self.reload_directory();
        }
        cx.emit(AppEvent::Sidebar);
        cx.notify();
    }

    pub fn apply(&mut self, event: BackendEvent, cx: &mut Context<Self>) {
        match event {
            BackendEvent::Connection(connection) => {
                self.connection = connection;
                cx.emit(AppEvent::Status);
            }
            BackendEvent::Engine(engine) => {
                self.engine = Some(engine);
                self.resend_outbox(cx);
                self.refresh_scheduled(cx);
                let waiting = self.directory.waiting_presence_ids();
                self.request_presence(waiting, cx);
                cx.emit(AppEvent::Status);
            }
            BackendEvent::Core(CoreEvent::SidebarChanged) => self.reload_sidebar(cx),
            BackendEvent::Core(
                CoreEvent::MessagesChanged { conversation_id }
                | CoreEvent::ReceiptsChanged { conversation_id },
            ) => {
                self.clear_typists_who_sent(&conversation_id, cx);
                cx.emit(AppEvent::Messages(conversation_id));
            }
            BackendEvent::Core(CoreEvent::ImagesChanged { keys }) => {
                if let Some(engine) = &self.engine {
                    for key in &keys {
                        let image = ImageRef {
                            id: String::new(),
                            url: key.clone(),
                            width: None,
                            height: None,
                        };
                        if let Some(path) = engine.image_path(&image) {
                            self.directory.set_image(key, &path);
                        }
                    }
                }
                cx.emit(AppEvent::Images(keys));
            }
            BackendEvent::Core(CoreEvent::PresenceChanged) => {
                let changed = self.sync_presence();
                self.directory.save_presence(&self.store, &changed);
                cx.emit(AppEvent::Directory);
                cx.notify();
            }
            BackendEvent::Core(_) => {
                if !self.mode.demo {
                    self.reload_directory();
                }
                cx.emit(AppEvent::Directory);
            }
            BackendEvent::Typing(event) => self.apply_typing(event, cx),
            BackendEvent::Live(live) => {
                self.live = live;
                cx.emit(AppEvent::Status);
            }
            BackendEvent::Favorites(ids) => {
                self.favorite_ids = ids;
                if !self.mode.demo {
                    self.reload_directory();
                }
                cx.emit(AppEvent::Sidebar);
            }
            BackendEvent::Synced(time) => {
                self.last_sync = Some(time);
                cx.emit(AppEvent::Status);
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chatsvc::TypingEvent;
    use chrono::{Duration, Utc};
    use gpui_kit::{AppContext as _, TestAppContext};
    use store::{
        ChannelRecord, ChatRecord, MemberRecord, MessageRecord, SidebarTeam, Store, TeamRecord,
    };
    use teams_core::CoreEvent;

    use super::{AppState, Mode, Selection, chat_title, selection_title};
    use crate::backend::BackendEvent;
    use crate::data::Person;
    use store::Sidebar;

    fn chat(id: &str, title: &str, unread: bool) -> ChatRecord {
        ChatRecord {
            id: id.into(),
            kind: "group".into(),
            title: title.into(),
            member_summary: "Member".into(),
            last_message_at: None,
            last_read_at: None,
            unread,
            ..Default::default()
        }
    }

    fn sidebar() -> Sidebar {
        Sidebar {
            chats: vec![chat("c1", "First", true), chat("c2", "", false)],
            teams: vec![SidebarTeam {
                team: TeamRecord {
                    id: "t".into(),
                    name: "Team".into(),
                },
                channels: vec![ChannelRecord {
                    id: "ch".into(),
                    team_id: "t".into(),
                    name: "General".into(),
                    membership_type: None,
                    last_message_at: None,
                    unread: false,
                }],
                hidden: false,
                hidden_channel_ids: Vec::new(),
            }],
        }
    }

    #[test]
    fn empty_chat_title_falls_back_to_members() {
        assert_eq!(chat_title(&sidebar().chats[1]), "Member");
    }

    #[test]
    fn selection_titles() {
        assert_eq!(
            selection_title(&sidebar(), &Selection::Channel("ch".into())),
            "Team / General"
        );
        assert_eq!(
            selection_title(&sidebar(), &Selection::Chat("c1".into())),
            "First"
        );
    }

    fn typing_event(user_id: &str, active: bool) -> BackendEvent {
        BackendEvent::Typing(TypingEvent {
            conversation_id: "c1".into(),
            user_id: user_id.into(),
            display_name: format!("Name {user_id}"),
            active,
            received_at: Utc::now(),
        })
    }

    #[gpui_kit::test]
    fn typing_events_track_other_users_and_ignore_me(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| {
            cx.new(|_| {
                let mut state = AppState::new(store.clone(), Mode::default());
                state.directory.me = Some(Person {
                    user_id: "me".into(),
                    display_name: "Me".into(),
                });
                state
            })
        });
        let names = |cx: &mut TestAppContext| cx.update(|cx| app.read(cx).typing.names("c1"));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply(typing_event("me", true), cx);
                state.apply(typing_event("ada", true), cx);
            })
        });
        assert_eq!(names(cx), vec!["Name ada".to_owned()]);

        let message = MessageRecord {
            conversation_id: "c1".into(),
            message_id: "m1".into(),
            sender_id: Some("ada".into()),
            created_at: Utc::now() + Duration::seconds(1),
            ..Default::default()
        };
        store.upsert_messages(&[message]).unwrap();
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply(
                    BackendEvent::Core(CoreEvent::MessagesChanged {
                        conversation_id: "c1".into(),
                    }),
                    cx,
                )
            })
        });
        assert!(names(cx).is_empty());

        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply(typing_event("ada", true), cx);
                state.apply(typing_event("ada", false), cx);
            })
        });
        assert!(names(cx).is_empty());
    }

    #[gpui_kit::test]
    fn typing_without_a_display_name_uses_the_chat_member_name(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| {
            cx.new(|_| {
                let mut state = AppState::new(store.clone(), Mode::default());
                let mut member_chat = chat("c1", "First", false);
                member_chat.members = vec![MemberRecord {
                    user_id: Some("ada".into()),
                    display_name: "Ada Lovelace".into(),
                }];
                state.sidebar.chats = vec![member_chat];
                state
            })
        });
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply(
                    BackendEvent::Typing(TypingEvent {
                        conversation_id: "c1".into(),
                        user_id: "ada".into(),
                        display_name: String::new(),
                        active: true,
                        received_at: Utc::now(),
                    }),
                    cx,
                )
            })
        });
        assert_eq!(
            cx.update(|cx| app.read(cx).typing.names("c1")),
            vec!["Ada Lovelace".to_owned()]
        );
    }
}
