use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::Document;

const REPORT_DIRECTORY: &str = "call-reports";
const KEEP_REPORTS: usize = 30;
const JSON_EXTENSION: &str = "json";
const MARKDOWN_EXTENSION: &str = "md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedReport {
    pub json: PathBuf,
    pub markdown: PathBuf,
}

#[cfg(test)]
pub fn report_directory() -> PathBuf {
    std::env::temp_dir().join("rusty-teams-test-call-reports")
}

#[cfg(not(test))]
pub fn report_directory() -> PathBuf {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(REPORT_DIRECTORY))
        .unwrap_or_else(|| PathBuf::from(REPORT_DIRECTORY))
}

pub fn save_report(directory: &Path, document: &Document) -> io::Result<SavedReport> {
    fs::create_dir_all(directory)?;
    let stem = free_stem(directory, &document.stem);
    let saved = SavedReport {
        json: directory.join(format!("{stem}.{JSON_EXTENSION}")),
        markdown: directory.join(format!("{stem}.{MARKDOWN_EXTENSION}")),
    };
    fs::write(&saved.json, &document.json)?;
    fs::write(&saved.markdown, &document.markdown)?;
    prune(directory, KEEP_REPORTS);
    Ok(saved)
}

fn free_stem(directory: &Path, stem: &str) -> String {
    let taken = |candidate: &str| directory.join(format!("{candidate}.{JSON_EXTENSION}")).exists();
    if !taken(stem) {
        return stem.to_owned();
    }
    (2..)
        .map(|counter| format!("{stem}-{counter}"))
        .find(|candidate| !taken(candidate))
        .expect("an unbounded counter finds a free name")
}

fn prune(directory: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut stems: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == JSON_EXTENSION))
        .filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .collect();
    stems.sort();
    let surplus = stems.len().saturating_sub(keep);
    for stem in &stems[..surplus] {
        for extension in [JSON_EXTENSION, MARKDOWN_EXTENSION] {
            let _ = fs::remove_file(directory.join(format!("{stem}.{extension}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(stem: &str) -> Document {
        Document { stem: stem.to_owned(), json: "{}".to_owned(), markdown: "# x".to_owned() }
    }

    #[test]
    fn the_newest_thirty_reports_are_kept_with_both_files() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..33 {
            save_report(directory.path(), &document(&format!("20260101T0000{index:02}Z-1to1"))).unwrap();
        }
        let mut names: Vec<String> =
            fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), 60);
        assert_eq!(names[0], "20260101T000003Z-1to1.json");
        assert_eq!(names[59], "20260101T000032Z-1to1.md");
    }

    #[test]
    fn a_report_in_the_same_second_gets_a_counter() {
        let directory = tempfile::tempdir().unwrap();
        let first = save_report(directory.path(), &document("20260101T000000Z-echo")).unwrap();
        let second = save_report(directory.path(), &document("20260101T000000Z-echo")).unwrap();
        assert_ne!(first.json, second.json);
        assert!(second.json.to_string_lossy().ends_with("20260101T000000Z-echo-2.json"));
    }
}
