use std::collections::VecDeque;

use gpui_kit::*;
use teams_core::FolderChange;

use crate::app_state::{AppEvent, AppState};
use crate::data::{Directory, PENDING_FOLDER_PREFIX};

pub const READ_ONLY_SECTIONS: &str = "Read-only mode: sections not changed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedFolderChange {
    pub change: FolderChange,
    pub label: &'static str,
    pub created_id: String,
}

#[derive(Debug, Default)]
pub struct SectionChanges {
    queue: VecDeque<QueuedFolderChange>,
    created: u64,
}

impl SectionChanges {
    pub fn push(&mut self, change: FolderChange, label: &'static str) -> QueuedFolderChange {
        self.created += 1;
        let queued = QueuedFolderChange {
            change,
            label,
            created_id: format!("{PENDING_FOLDER_PREFIX}{}", self.created),
        };
        self.queue.push_back(queued.clone());
        queued
    }

    pub fn in_flight(&self) -> Option<&QueuedFolderChange> {
        self.queue.front()
    }

    pub fn finish(&mut self) -> Option<&QueuedFolderChange> {
        self.queue.pop_front();
        self.queue.front()
    }

    pub fn clear(&mut self) {
        self.queue.clear();
    }

    pub fn reapply(&self, directory: &mut Directory) {
        for queued in &self.queue {
            directory.apply_folder_change(&queued.change, &queued.created_id);
        }
    }
}

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
        if self.mode.read_only {
            self.raise_notice(READ_ONLY_SECTIONS.to_owned(), None, cx);
            return;
        }
        if self.engine.is_none() {
            if self.mode.demo {
                let demo_id = uuid::Uuid::new_v4().to_string();
                self.directory.apply_folder_change(&change, &demo_id);
                self.notify_sections(cx);
            }
            return;
        }
        let starts_now = self.section_changes.in_flight().is_none();
        let queued = self.section_changes.push(change, label);
        self.directory
            .apply_folder_change(&queued.change, &queued.created_id);
        self.notify_sections(cx);
        if starts_now {
            self.run_next_section_change(cx);
        }
    }

    fn run_next_section_change(&mut self, cx: &mut Context<Self>) {
        let Some(queued) = self.section_changes.in_flight().cloned() else {
            return;
        };
        let Some(engine) = self.engine.clone() else {
            self.section_changes.clear();
            return;
        };
        self.run_chat_action(
            queued.label,
            async move { engine.change_folders(queued.change).await },
            Self::finish_section_change,
            Self::finish_section_change,
            cx,
        );
    }

    fn finish_section_change(&mut self, cx: &mut Context<Self>) {
        let has_next = self.section_changes.finish().is_some();
        self.reload_directory();
        self.notify_sections(cx);
        if has_next {
            self.run_next_section_change(cx);
        }
    }

    pub fn reapply_section_changes(&mut self) {
        self.section_changes.reapply(&mut self.directory);
    }

    fn notify_sections(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppEvent::Sidebar);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{Directory, FolderChange, SectionChanges};
    use crate::data::{FolderInfo, FolderKind};

    fn create(name: &str) -> FolderChange {
        FolderChange::Create { name: name.into() }
    }

    #[test]
    fn folder_changes_run_one_at_a_time_in_order() {
        let mut changes = SectionChanges::default();
        assert!(changes.in_flight().is_none());
        changes.push(create("a"), "New section");
        changes.push(create("b"), "New section");
        assert_eq!(changes.in_flight().unwrap().change, create("a"));
        assert_eq!(changes.finish().unwrap().change, create("b"));
        assert!(changes.finish().is_none());
        assert!(changes.in_flight().is_none());
    }

    #[test]
    fn same_name_creates_get_distinct_pending_ids() {
        let mut changes = SectionChanges::default();
        changes.push(create("Todo"), "New section");
        changes.push(create("Todo"), "New section");
        let mut directory = Directory::default();
        changes.reapply(&mut directory);
        assert_eq!(directory.folders.len(), 2);
        assert_ne!(directory.folders[0].id, directory.folders[1].id);
    }

    #[test]
    fn pending_create_survives_a_directory_reload() {
        let mut changes = SectionChanges::default();
        changes.push(create("Todo"), "New section");
        let reloaded = || Directory {
            folders: vec![FolderInfo {
                id: "recent".into(),
                name: String::new(),
                kind: FolderKind::Recent,
                expanded: None,
                conversation_ids: Vec::new(),
            }],
            ..Default::default()
        };
        let mut directory = reloaded();
        changes.reapply(&mut directory);
        assert_eq!(directory.folders[0].name, "Todo");
        changes.finish();
        let mut directory = reloaded();
        changes.reapply(&mut directory);
        assert_eq!(directory.folders.len(), 1);
    }
}
