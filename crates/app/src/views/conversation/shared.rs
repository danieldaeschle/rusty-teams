use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use store::ChannelTabRecord;
use teams_core::{DriveEntry, DriveFolder};

use super::ConversationView;
use crate::app_state::Selection;
use crate::channel_files::{
    Crumb, DownloadSource, LibraryPath, MessageFilter, SharedContent, SharedItem, file_card,
    library_file, shared_items, sorted_entries,
};
use crate::downloads::DownloadKey;
use crate::embedded_web::{self, EmbeddedWeb, EmbeddedWebEvent};
use crate::notice::short_error;
use crate::runtime;
use crate::views::attachment_tray::{display_name, read_attachment};
use crate::views::channel_tabs::{
    ChannelPane, TabAction, TabBarActions, fallback_tab_link, render_tab_bar, tab_action,
};
use crate::views::shared_tab::{
    LibraryView, Listing, MessagesView, SharedCommand, SharedRun, SharedScope, render_library,
    render_messages,
};
use crate::views::sidebar::SIDEBAR_WIDTH;

const LIBRARY_KEY: &str = "library";
const NEW_FOLDER_PLACEHOLDER: &str = "Folder name";
const ITEM_SCAN_LIMIT: usize = 5000;

pub(super) struct SharedState {
    scope: SharedScope,
    filter: MessageFilter,
    path: Option<LibraryPath>,
    listing: Listing,
    generation: u64,
    new_folder_open: bool,
    uploads_running: usize,
    items: Vec<SharedItem>,
    listed_at: Option<Instant>,
}

impl Default for SharedState {
    fn default() -> Self {
        SharedState {
            scope: SharedScope::Library,
            filter: MessageFilter::All,
            path: None,
            listing: Listing::Loading,
            generation: 0,
            new_folder_open: false,
            uploads_running: 0,
            items: Vec::new(),
            listed_at: None,
        }
    }
}

impl ConversationView {
    fn channel_id(&self) -> Option<String> {
        match &self.current.as_ref()?.selection {
            Selection::Channel(channel_id) => Some(channel_id.clone()),
            Selection::Chat(_) => None,
        }
    }

    pub(super) fn shared_active(&self) -> bool {
        self.pane == ChannelPane::Shared && self.in_feed()
    }

    pub(super) fn web_active(&self) -> bool {
        self.pane == ChannelPane::Web && self.web.is_some() && self.in_feed()
    }

    pub(super) fn close_web(&mut self) {
        self.web = None;
        self.web_subscription = None;
    }

    pub(super) fn reset_channel_tabs(&mut self, cx: &mut Context<Self>) {
        self.pane = ChannelPane::Posts;
        self.close_web();
        self.tab_links.clear();
        self.shared = SharedState {
            generation: self.shared.generation + 1,
            ..SharedState::default()
        };
        self.reload_channel_tabs(cx);
    }

    pub(super) fn reload_channel_tabs(&mut self, cx: &mut Context<Self>) {
        let store = self.app.read(cx).store.clone();
        let tabs = self
            .channel_id()
            .and_then(|channel_id| store.channel_tabs(&channel_id).ok())
            .unwrap_or_default();
        if tabs != self.channel_tabs {
            self.channel_tabs = tabs;
            cx.notify();
        }
    }

    pub(super) fn render_tab_bar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.channel_id().filter(|_| self.in_feed())?;
        let available = f32::from(window.viewport_size().width) - SIDEBAR_WIDTH;
        let view = cx.weak_entity();
        let select = Rc::new(move |pane, cx: &mut App| {
            view.update(cx, |this, cx| this.select_pane(pane, cx)).ok();
        });
        let open = {
            let view = cx.weak_entity();
            Rc::new(move |index: usize, window: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| this.open_channel_tab(index, window, cx))
                    .ok();
            })
        };
        let active_tab_id = self
            .web
            .as_ref()
            .filter(|_| self.web_active())
            .map(|web| web.read(cx).tab_id().to_owned());
        Some(render_tab_bar(
            &self.channel_tabs,
            self.pane,
            active_tab_id.as_deref(),
            embedded_web::available(),
            available,
            &TabBarActions {
                select,
                open,
            },
        ))
    }

    fn open_channel_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(tab), Some(channel_id)) =
            (self.channel_tabs.get(index).cloned(), self.channel_id())
        else {
            return;
        };
        let resolved = self.tab_links.get(&tab.tab_id).map(String::as_str);
        match tab_action(&tab, embedded_web::available(), resolved) {
            TabAction::Embed(url) => self.show_web_tab(tab.tab_id, url, window, cx),
            TabAction::Browser(url) => cx.open_url(&url),
            TabAction::ResolveLink => self.resolve_tab_link(tab, channel_id, cx),
        }
    }

    fn show_web_tab(
        &mut self,
        tab_id: String,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.web_active()
            && self
                .web
                .as_ref()
                .is_some_and(|web| web.read(cx).tab_id() == tab_id)
        {
            return;
        }
        self.close_web();
        let Some((native, events)) = embedded_web::Native::open(&url, window) else {
            cx.open_url(&url);
            return;
        };
        let web = cx.new(|cx| {
            EmbeddedWeb::new(tab_id, url, native, events, self.focus_handle.clone(), cx)
        });
        self.web_subscription =
            Some(cx.subscribe(&web, |this, _, event: &EmbeddedWebEvent, cx| {
                let EmbeddedWebEvent::Closed { start_url } = event;
                this.close_web();
                this.pane = ChannelPane::Posts;
                cx.open_url(start_url);
                cx.notify();
            }));
        self.web = Some(web);
        self.pane = ChannelPane::Web;
        cx.notify();
    }

    fn resolve_tab_link(
        &mut self,
        tab: ChannelTabRecord,
        channel_id: String,
        cx: &mut Context<Self>,
    ) {
        let fallback = fallback_tab_link(&tab, &channel_id);
        let state = self.app.read(cx);
        let engine = state.engine.clone().filter(|_| !state.mode.demo);
        let Some(engine) = engine else {
            cx.open_url(&fallback);
            return;
        };
        let tab_id = tab.tab_id;
        let lookup = {
            let tab_id = tab_id.clone();
            runtime::spawn(async move { engine.channel_tab_web_url(&channel_id, &tab_id).await })
        };
        cx.spawn(async move |this, cx| {
            let link = lookup.await.ok().and_then(Result::ok).flatten();
            this.update(cx, |this, cx| {
                let target = match link {
                    Some(link) => {
                        this.tab_links.insert(tab_id, link.clone());
                        link
                    }
                    None => fallback,
                };
                cx.open_url(&target);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn select_pane(&mut self, pane: ChannelPane, cx: &mut Context<Self>) {
        if self.pane == pane {
            return;
        }
        self.pane = pane;
        self.close_web();
        if pane == ChannelPane::Shared {
            self.refresh_shared_items(cx);
            if self.shared.path.is_none() {
                self.load_library(cx);
            }
        }
        cx.notify();
    }

    pub(super) fn refresh_shared_items(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.channel_id() else {
            return;
        };
        let store = self.app.read(cx).store.clone();
        let records = store
            .messages(&channel_id, None, ITEM_SCAN_LIMIT)
            .unwrap_or_default();
        self.shared.items = shared_items(&records);
    }

    fn download_library_entry(
        &mut self,
        conversation_id: &str,
        entry: &DriveEntry,
        cx: &mut Context<Self>,
    ) {
        let source =
            DownloadSource::Library(library_file(entry, self.shared.listed_at, Instant::now()));
        self.activate_file(conversation_id, LIBRARY_KEY, file_card(entry), source, cx);
    }

    fn library_entry(&self, index: usize) -> Option<DriveEntry> {
        match &self.shared.listing {
            Listing::Ready(entries) => entries.get(index).cloned(),
            _ => None,
        }
    }

    fn library_download_states(&self) -> Vec<Option<crate::downloads::DownloadState>> {
        let conversation_id = self.conversation_id().unwrap_or_default();
        let Listing::Ready(entries) = &self.shared.listing else {
            return Vec::new();
        };
        entries
            .iter()
            .map(|entry| {
                let key = DownloadKey::new(&conversation_id, LIBRARY_KEY, &entry.web_url);
                self.downloads.state(&key).cloned()
            })
            .collect()
    }

    fn item_download_states(&self) -> Vec<Option<crate::downloads::DownloadState>> {
        let conversation_id = self.conversation_id().unwrap_or_default();
        self.shared
            .items
            .iter()
            .map(|item| match &item.content {
                SharedContent::File(card) => {
                    let key = DownloadKey::new(&conversation_id, &item.message_id, &card.open_url);
                    self.downloads.state(&key).cloned()
                }
                SharedContent::Link(_) => None,
            })
            .collect()
    }

    pub(super) fn render_shared(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.weak_entity();
        let run: SharedRun = Rc::new(move |command, window, cx| {
            view.update(cx, |this, cx| this.run_shared(command, window, cx))
                .ok();
        });
        match self.shared.scope {
            SharedScope::Library => {
                let status = (self.shared.uploads_running > 0)
                    .then(|| format!("Uploading {} ...", self.shared.uploads_running));
                render_library(
                    LibraryView {
                        path: self.shared.path.as_ref(),
                        listing: &self.shared.listing,
                        new_folder: self
                            .new_folder_input
                            .as_ref()
                            .filter(|_| self.shared.new_folder_open),
                        status: status.as_deref(),
                        download_states: self.library_download_states(),
                    },
                    &run,
                )
            }
            SharedScope::Messages => render_messages(
                MessagesView {
                    items: &self.shared.items,
                    filter: self.shared.filter,
                    download_states: self.item_download_states(),
                },
                &run,
            ),
        }
    }

    fn run_shared(&mut self, command: SharedCommand, window: &mut Window, cx: &mut Context<Self>) {
        let conversation_id = self.conversation_id().unwrap_or_default();
        match command {
            SharedCommand::Scope(scope) => {
                self.shared.scope = scope;
                if scope == SharedScope::Messages {
                    self.refresh_shared_items(cx);
                }
                cx.notify();
            }
            SharedCommand::Filter(filter) => {
                self.shared.filter = filter;
                cx.notify();
            }
            SharedCommand::Crumb(index) => {
                if let Some(path) = self.shared.path.as_mut() {
                    path.go_to(index);
                }
                self.shared.new_folder_open = false;
                self.load_library(cx);
            }
            SharedCommand::Open(index) => {
                let Some(entry) = self.library_entry(index) else {
                    return;
                };
                if entry.is_folder() {
                    if let Some(path) = self.shared.path.as_mut() {
                        path.enter(&entry);
                    }
                    self.shared.new_folder_open = false;
                    self.load_library(cx);
                } else {
                    self.download_library_entry(&conversation_id, &entry, cx);
                }
            }
            SharedCommand::OpenInBrowser(index) => {
                if let Some(entry) = self.library_entry(index) {
                    cx.open_url(&entry.web_url);
                }
            }
            SharedCommand::Download(index) => {
                if let Some(entry) = self.library_entry(index) {
                    self.download_library_entry(&conversation_id, &entry, cx);
                }
            }
            SharedCommand::CopyLink(index) => {
                if let Some(entry) = self.library_entry(index) {
                    cx.write_to_clipboard(ClipboardItem::new_string(entry.web_url));
                    self.app.update(cx, |state, cx| {
                        state.raise_notice("Link copied".to_owned(), None, cx)
                    });
                }
            }
            SharedCommand::Retry => self.load_library(cx),
            SharedCommand::ToggleNewFolder => self.toggle_new_folder(window, cx),
            SharedCommand::Upload => self.pick_upload(cx),
            SharedCommand::OpenSharePoint => {
                if let Some(path) = &self.shared.path {
                    cx.open_url(path.current_web_url());
                }
            }
            SharedCommand::OpenItem(index) => {
                let Some(item) = self.shared.items.get(index).cloned() else {
                    return;
                };
                match item.content {
                    SharedContent::File(card) => self.activate_file(
                        &conversation_id,
                        &item.message_id,
                        card,
                        DownloadSource::Share,
                        cx,
                    ),
                    SharedContent::Link(url) => cx.open_url(&url),
                }
            }
            SharedCommand::JumpToPost(index) => {
                let Some(message_id) = self
                    .shared
                    .items
                    .get(index)
                    .map(|item| item.message_id.clone())
                else {
                    return;
                };
                self.jump_to_message(&conversation_id, &message_id, cx);
            }
        }
    }

    fn load_library(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.channel_id() else {
            return;
        };
        self.shared.generation += 1;
        let generation = self.shared.generation;
        self.shared.listing = Listing::Loading;
        cx.notify();
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        let folder = self.shared.path.as_ref().map(|path| path.current().clone());
        if demo {
            self.load_demo_library(&channel_id, folder.is_none());
            return;
        }
        let Some(engine) = engine else {
            self.shared.listing = Listing::Failed("not connected".to_owned());
            return;
        };
        let receiver = runtime::spawn(async move {
            let (root, target) = match folder {
                Some(folder) => (None, folder),
                None => {
                    let root = engine.channel_library_root(&channel_id).await?;
                    let target = root.folder();
                    (Some(root), target)
                }
            };
            let entries = engine.library_children(&target).await?;
            Ok::<_, teams_core::Error>((root, entries))
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(loaded)) => Ok(loaded),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(error) => Err(short_error(&error)),
            };
            this.update(cx, |this, cx| {
                this.finish_library_load(generation, outcome, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_library_load(
        &mut self,
        generation: u64,
        outcome: Result<(Option<DriveEntry>, Vec<DriveEntry>), String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.shared.generation {
            return;
        }
        match outcome {
            Ok((root, entries)) => {
                if let Some(root) = root {
                    self.shared.path = Some(LibraryPath::new(Crumb {
                        name: root.name.clone(),
                        folder: root.folder(),
                        web_url: root.web_url,
                    }));
                }
                self.shared.listing = Listing::Ready(sorted_entries(entries));
                self.shared.listed_at = Some(Instant::now());
            }
            Err(reason) => self.shared.listing = Listing::Failed(reason),
        }
        cx.notify();
    }

    fn load_demo_library(&mut self, channel_id: &str, at_root: bool) {
        if at_root {
            let name = self
                .current
                .as_ref()
                .map(|current| current.title.clone())
                .unwrap_or_default();
            let root = self.demo_library.root(channel_id, &name);
            self.shared.path = Some(LibraryPath::new(Crumb {
                name: root.name.clone(),
                folder: root.folder(),
                web_url: root.web_url,
            }));
        }
        let Some(path) = &self.shared.path else {
            return;
        };
        let entries = self.demo_library.children(&path.current().item_id);
        self.shared.listing = Listing::Ready(sorted_entries(entries));
    }

    fn toggle_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shared.new_folder_open {
            self.shared.new_folder_open = false;
            cx.notify();
            return;
        }
        let input = match &self.new_folder_input {
            Some(input) => input.clone(),
            None => {
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder(NEW_FOLDER_PLACEHOLDER));
                let subscription = cx.subscribe_in(&input, window, Self::on_new_folder_event);
                self._subscriptions.push(subscription);
                self.new_folder_input = Some(input.clone());
                input
            }
        };
        input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.shared.new_folder_open = true;
        cx.notify();
    }

    fn on_new_folder_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.create_folder(cx);
        }
    }

    pub(super) fn close_new_folder(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.shared.new_folder_open {
            return false;
        }
        self.shared.new_folder_open = false;
        cx.notify();
        true
    }

    fn create_folder(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.new_folder_input.clone() else {
            return;
        };
        let name = input.read(cx).value().trim().to_owned();
        let Some(parent) = self.shared.path.as_ref().map(|path| path.current().clone()) else {
            return;
        };
        if name.is_empty() {
            return;
        }
        self.shared.new_folder_open = false;
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        if demo {
            self.demo_library.create_folder(&parent.item_id, &name);
            self.load_library(cx);
            return;
        }
        let Some(engine) = engine else {
            return;
        };
        let target = parent.clone();
        let receiver =
            runtime::spawn(async move { engine.create_library_folder(&target, &name).await });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(error) => Err(short_error(&error)),
            };
            this.update(cx, |this, cx| {
                this.finish_change(&parent, outcome, "Folder not created", cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_change(
        &mut self,
        folder: &DriveFolder,
        outcome: Result<(), String>,
        failure: &str,
        cx: &mut Context<Self>,
    ) {
        if let Err(reason) = outcome {
            self.notice = Some(format!("{failure}: {reason}"));
        }
        let still_open = self
            .shared
            .path
            .as_ref()
            .is_some_and(|path| path.current() == folder);
        if still_open {
            self.load_library(cx);
        } else {
            cx.notify();
        }
    }

    fn pick_upload(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                this.update(cx, |this, cx| this.upload_paths(paths, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(super) fn upload_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(folder) = self.shared.path.as_ref().map(|path| path.current().clone()) else {
            return;
        };
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        for path in paths {
            self.shared.uploads_running += 1;
            let (folder, engine) = (folder.clone(), engine.clone());
            cx.spawn(async move |this, cx| {
                let name = display_name(&path);
                let loaded = cx
                    .background_executor()
                    .spawn(async move { read_attachment(&path) })
                    .await;
                let outcome = match loaded {
                    Err(reason) => Err(reason),
                    Ok(file) if demo => {
                        let size = file.bytes.len() as u64;
                        let (folder_id, file_name) = (folder.item_id.clone(), name.clone());
                        this.update(cx, |this, _| {
                            this.demo_library.add_file(&folder_id, &file_name, size);
                        })
                        .ok();
                        Ok(())
                    }
                    Ok(file) => match engine {
                        None => Err("not connected".to_owned()),
                        Some(engine) => {
                            let (target, file_name) = (folder.clone(), name.clone());
                            let receiver = runtime::spawn(async move {
                                engine
                                    .upload_to_library(&target, &file_name, &file.bytes, |_| {})
                                    .await
                            });
                            match receiver.await {
                                Ok(Ok(_)) => Ok(()),
                                Ok(Err(error)) => Err(short_error(&error)),
                                Err(error) => Err(short_error(&error)),
                            }
                        }
                    },
                };
                this.update(cx, |this, cx| {
                    this.shared.uploads_running = this.shared.uploads_running.saturating_sub(1);
                    let failure = format!("{name} not uploaded");
                    if this.shared.uploads_running == 0 || outcome.is_err() {
                        this.finish_change(&folder, outcome, &failure, cx);
                    } else {
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }
}
