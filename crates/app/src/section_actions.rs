use gpui_kit::*;
use teams_core::FolderChange;

use crate::app_state::{AppEvent, AppState};

impl AppState {
    pub fn create_section(&mut self, name: &str, cx: &mut Context<Self>) {
        let change = FolderChange::Create {
            name: name.to_owned(),
        };
        self.change_sections(change, "New section", cx);
    }

    pub fn rename_section(&mut self, folder_id: &str, name: &str, cx: &mut Context<Self>) {
        let change = FolderChange::Rename {
            folder_id: folder_id.to_owned(),
            name: name.to_owned(),
        };
        self.change_sections(change, "Rename section", cx);
    }

    pub fn delete_section(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        let change = FolderChange::Delete {
            folder_id: folder_id.to_owned(),
        };
        self.change_sections(change, "Delete section", cx);
    }

    pub fn move_section(
        &mut self,
        folder_id: &str,
        target_folder_id: &str,
        cx: &mut Context<Self>,
    ) {
        let change = FolderChange::Move {
            folder_id: folder_id.to_owned(),
            target_folder_id: target_folder_id.to_owned(),
        };
        self.change_sections(change, "Move section", cx);
    }

    fn change_sections(
        &mut self,
        change: FolderChange,
        label: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        self.directory.apply_folder_change(&change);
        cx.emit(AppEvent::Sidebar);
        cx.notify();
        let Some(engine) = engine_or_demo else {
            return;
        };
        self.run_chat_action(
            label,
            async move { engine.change_folders(change).await },
            Self::reload_sections,
            Self::reload_sections,
            cx,
        );
    }

    fn reload_sections(&mut self, cx: &mut Context<Self>) {
        self.reload_directory();
        cx.emit(AppEvent::Sidebar);
        cx.notify();
    }
}
