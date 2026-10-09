use std::collections::HashMap;

use chrono::{Duration, Utc};
use teams_core::DriveEntry;

const DEMO_SITE: &str = "https://demo.sharepoint.example/sites/demo";
const AUTHORS: [&str; 4] = [
    "Mara Lindqvist",
    "Jonas Ortega",
    "Priya Nair",
    "Tobias Klein",
];
const ROOT_FILES: [(&str, u64); 5] = [
    ("Roadmap-Q4.pptx", 3_412_000),
    ("Release-Checklist-Q4.xlsx", 48_213),
    ("Rollout-Plan.pdf", 2_306_867),
    ("Postmortem-Draft.docx", 91_400),
    ("Screenshots.zip", 12_880_000),
];
const SPEC_FILES: [(&str, u64); 2] = [("API-Design.pdf", 812_000), ("Data-model.xlsx", 64_300)];
const NOTE_FILES: [(&str, u64); 2] = [
    ("Weekly-sync-notes.docx", 38_900),
    ("Retro-2026-10.pdf", 205_000),
];
const YEAR_FILES: [(&str, u64); 1] = [("Settings-spec-v3.docx", 120_500)];

#[derive(Default)]
pub struct DemoLibrary {
    children: HashMap<String, Vec<DriveEntry>>,
    next_id: u64,
}

impl DemoLibrary {
    pub fn root(&mut self, channel_id: &str, channel_name: &str) -> DriveEntry {
        let root_id = format!("{channel_id}-root");
        if !self.children.contains_key(&root_id) {
            self.fill(&root_id);
        }
        self.entry(&root_id, channel_name, Some(0), 0)
    }

    pub fn children(&self, folder_id: &str) -> Vec<DriveEntry> {
        self.children
            .get(folder_id)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| DriveEntry {
                        child_count: entry.child_count.map(|_| {
                            self.children
                                .get(&entry.id)
                                .map_or(0, |inner| inner.len() as u64)
                        }),
                        ..entry.clone()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn create_folder(&mut self, parent_id: &str, name: &str) -> DriveEntry {
        let mut name = name.to_owned();
        let taken = |library: &Self, name: &str| {
            library.children.get(parent_id).is_some_and(|entries| {
                entries
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(name))
            })
        };
        let base = name.clone();
        let mut number = 1;
        while taken(self, &name) {
            number += 1;
            name = format!("{base} {number}");
        }
        let id = self.new_id(parent_id);
        let entry = self.entry(&id, &name, Some(0), 0);
        self.children
            .entry(parent_id.to_owned())
            .or_default()
            .push(entry.clone());
        self.children.insert(id, Vec::new());
        entry
    }

    pub fn add_file(&mut self, parent_id: &str, name: &str, size: u64) -> DriveEntry {
        let id = self.new_id(parent_id);
        let entry = self.entry(&id, name, None, size);
        self.children
            .entry(parent_id.to_owned())
            .or_default()
            .push(entry.clone());
        entry
    }

    fn new_id(&mut self, parent_id: &str) -> String {
        self.next_id += 1;
        format!("{parent_id}-{}", self.next_id)
    }

    fn entry(&self, id: &str, name: &str, child_count: Option<u64>, size: u64) -> DriveEntry {
        let age = Duration::hours(3 + (id.len() as i64 * 37) % 400);
        DriveEntry {
            drive_id: "demo-drive".to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
            web_url: format!("{DEMO_SITE}/{id}/{name}"),
            size,
            modified_at: Some(Utc::now() - age),
            modified_by: Some(AUTHORS[id.len() % AUTHORS.len()].to_owned()),
            child_count,
            download_url: None,
        }
    }

    fn fill(&mut self, root_id: &str) {
        let specs = format!("{root_id}-specs");
        let notes = format!("{root_id}-notes");
        let year = format!("{specs}-2026");
        let spec_folder = self.entry(&specs, "Specs", Some(0), 0);
        let note_folder = self.entry(&notes, "Meeting notes", Some(0), 0);
        let year_folder = self.entry(&year, "2026", Some(0), 0);
        let files = |library: &Self, parent: &str, list: &[(&str, u64)]| -> Vec<DriveEntry> {
            list.iter()
                .map(|(name, size)| library.entry(&format!("{parent}-{name}"), name, None, *size))
                .collect()
        };
        let mut root = vec![spec_folder, note_folder];
        root.extend(files(self, root_id, &ROOT_FILES));
        let mut spec_children = vec![year_folder];
        spec_children.extend(files(self, &specs, &SPEC_FILES));
        let note_children = files(self, &notes, &NOTE_FILES);
        let year_children = files(self, &year, &YEAR_FILES);
        self.children.insert(root_id.to_owned(), root);
        self.children.insert(specs, spec_children);
        self.children.insert(notes, note_children);
        self.children.insert(year, year_children);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_library_has_folders_files_and_a_nested_folder() {
        let mut library = DemoLibrary::default();
        let root = library.root("demo-channel-1-1", "General");
        let entries = library.children(&root.id);
        assert_eq!(entries.iter().filter(|entry| entry.is_folder()).count(), 2);
        assert_eq!(entries.iter().filter(|entry| !entry.is_folder()).count(), 5);
        let specs = entries.iter().find(|entry| entry.name == "Specs").unwrap();
        assert_eq!(specs.child_count, Some(3));
        let inner = library.children(&specs.id);
        assert!(
            inner
                .iter()
                .any(|entry| entry.is_folder() && entry.child_count == Some(1))
        );
    }

    #[test]
    fn new_folders_get_a_free_name_and_uploads_show_up() {
        let mut library = DemoLibrary::default();
        let root = library.root("demo-channel-1-1", "General");
        let first = library.create_folder(&root.id, "Specs");
        assert_eq!(first.name, "Specs 2");
        library.add_file(&first.id, "a.pdf", 10);
        let listed = library.children(&root.id);
        let created = listed.iter().find(|entry| entry.id == first.id).unwrap();
        assert_eq!(created.child_count, Some(1));
    }
}
