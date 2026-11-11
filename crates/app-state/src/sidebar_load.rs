use std::collections::HashMap;

use gpui_kit::*;
use store::{ChatRecord, MessageImportance, Sidebar, Store};

use crate::app_state::{AppState, Selection};
use crate::backend::Engine;
use crate::data;

#[derive(Default)]
pub struct SidebarLoad {
    running: bool,
    requested_again: bool,
    local_changes: u64,
}

impl SidebarLoad {
    pub fn local_change(&mut self) {
        self.local_changes += 1;
    }
}

#[derive(Default)]
pub struct Unread {
    pub counts: HashMap<String, u32>,
    pub importance: HashMap<String, MessageImportance>,
}

pub struct LoadedSidebar {
    pub sidebar: Sidebar,
    pub unread: Unread,
}

pub fn unread_of(engine: Option<&Engine>, chats: &[ChatRecord]) -> Unread {
    match engine {
        Some(engine) => Unread {
            counts: data::unread_counts(engine, chats),
            importance: data::unread_importance(engine, chats),
        },
        None => Unread::default(),
    }
}

fn load(store: &Store, engine: Option<&Engine>) -> Option<LoadedSidebar> {
    let sidebar = store.sidebar().ok()?;
    let unread = unread_of(engine, &sidebar.chats);
    Some(LoadedSidebar { sidebar, unread })
}

impl AppState {
    pub fn reload_sidebar_after_sync(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_load.running {
            self.sidebar_load.requested_again = true;
            return;
        }
        self.sidebar_load.running = true;
        cx.spawn(async move |this, cx| {
            loop {
                let Ok((store, engine, local_changes)) = this.update(cx, |state, _| {
                    state.sidebar_load.requested_again = false;
                    (
                        state.store.clone(),
                        state.engine.clone(),
                        state.sidebar_load.local_changes,
                    )
                }) else {
                    return;
                };
                let loaded = cx
                    .background_spawn(async move { load(&store, engine.as_deref()) })
                    .await;
                let again = this.update(cx, |state, cx| {
                    let stale = state.sidebar_load.local_changes != local_changes;
                    if let Some(loaded) = loaded.filter(|_| !stale) {
                        state.adopt_synced_sidebar(loaded, cx);
                    }
                    let again = state.sidebar_load.requested_again || stale;
                    state.sidebar_load.running = again;
                    again
                });
                if !matches!(again, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    fn adopt_synced_sidebar(&mut self, loaded: LoadedSidebar, cx: &mut Context<Self>) {
        let selected = match &self.selection {
            Some(Selection::Chat(chat_id)) => Some(chat_id.clone()),
            _ => None,
        };
        let is_listed =
            |chats: &[ChatRecord], chat_id: &str| chats.iter().any(|chat| chat.id == chat_id);
        let was_listed = selected
            .as_deref()
            .is_some_and(|chat_id| is_listed(&self.sidebar.chats, chat_id));
        let removed = was_listed
            && selected
                .as_deref()
                .is_some_and(|chat_id| !is_listed(&loaded.sidebar.chats, chat_id));
        let next = selected
            .as_deref()
            .filter(|_| removed)
            .and_then(|chat_id| self.next_chat_after(chat_id));
        self.adopt_loaded_sidebar(loaded, cx);
        self.warm_recent_chats();
        if removed {
            self.leave_removed_selection(next, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::{AppContext, TestAppContext};
    use store::{ChatRecord, Store};
    use teams_core::CoreEvent;

    use crate::app_state::{AppState, Mode, Selection};
    use crate::backend::BackendEvent;

    fn chat(id: &str) -> ChatRecord {
        ChatRecord {
            id: id.into(),
            kind: "group".into(),
            title: id.into(),
            ..Default::default()
        }
    }

    fn listed(app: &gpui_kit::Entity<AppState>, cx: &mut TestAppContext) -> Vec<String> {
        cx.update(|cx| {
            app.read(cx)
                .sidebar
                .chats
                .iter()
                .map(|chat| chat.id.clone())
                .collect()
        })
    }

    fn sidebar_changed(app: &gpui_kit::Entity<AppState>, cx: &mut TestAppContext) {
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply(BackendEvent::Core(CoreEvent::SidebarChanged), cx)
            })
        });
    }

    #[gpui_kit::test]
    fn a_synced_sidebar_loads_in_the_background_and_bursts_load_once_more(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        store.upsert_chats(&[chat("first")]).unwrap();
        sidebar_changed(&app, cx);
        assert!(listed(&app, cx).is_empty());
        store.upsert_chats(&[chat("second")]).unwrap();
        sidebar_changed(&app, cx);
        cx.run_until_parked();
        let mut chats = listed(&app, cx);
        chats.sort();
        assert_eq!(chats, vec!["first".to_owned(), "second".to_owned()]);
    }

    #[gpui_kit::test]
    fn a_selected_chat_gone_after_sync_moves_the_selection_on(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        store.upsert_chats(&[chat("kept"), chat("gone")]).unwrap();
        let app = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.reload_sidebar(cx);
                state.selection = Some(Selection::Chat("gone".into()));
            })
        });
        store.remove_chats(&["gone".to_owned()]).unwrap();
        sidebar_changed(&app, cx);
        cx.run_until_parked();
        let selection = cx.update(|cx| app.read(cx).selection.clone());
        assert_eq!(selection, Some(Selection::Chat("kept".into())));
    }
}
