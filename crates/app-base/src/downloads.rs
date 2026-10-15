use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const FALLBACK_FILE_NAME: &str = "file";
const PART_SUFFIX: &str = ".part";
const MAX_NAME_BYTES: usize = 255;
const MAX_EXTENSION_BYTES: usize = 32;
const DEMO_DIRECTORY: &str = "rusty-teams-demo";
const DEMO_CONTENT: &[u8] = b"Rusty Teams demo file\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Saving(u8),
    Saved(PathBuf),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickAction {
    Start,
    Reveal(PathBuf),
    Ignore,
}

pub fn click_action(state: Option<&DownloadState>) -> ClickAction {
    match state {
        None | Some(DownloadState::Failed(_)) => ClickAction::Start,
        Some(DownloadState::Saved(path)) => ClickAction::Reveal(path.clone()),
        Some(DownloadState::Saving(_)) => ClickAction::Ignore,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonLook {
    pub symbol: &'static str,
    pub percent: Option<u8>,
    pub tooltip: String,
    pub clickable: bool,
}

pub fn button_look(state: Option<&DownloadState>) -> ButtonLook {
    match state {
        None => ButtonLook {
            symbol: "download",
            percent: None,
            tooltip: "Save to Downloads".to_owned(),
            clickable: true,
        },
        Some(DownloadState::Saving(percent)) => ButtonLook {
            symbol: "download",
            percent: Some(*percent),
            tooltip: "Saving".to_owned(),
            clickable: false,
        },
        Some(DownloadState::Saved(_)) => ButtonLook {
            symbol: "folder_open",
            percent: None,
            tooltip: "Show in folder".to_owned(),
            clickable: true,
        },
        Some(DownloadState::Failed(reason)) => ButtonLook {
            symbol: "refresh",
            percent: None,
            tooltip: format!("Save failed: {reason}. Click to retry"),
            clickable: true,
        },
    }
}

pub fn percent_label(percent: u8) -> String {
    format!("{percent} %")
}

pub fn progress_fraction(state: Option<&DownloadState>) -> Option<f32> {
    match state {
        Some(DownloadState::Saving(percent)) => Some(f32::from((*percent).min(100)) / 100.),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DownloadKey {
    pub conversation_id: String,
    pub message_key: String,
    pub open_url: String,
}

impl DownloadKey {
    pub fn new(conversation_id: &str, message_key: &str, open_url: &str) -> Self {
        DownloadKey {
            conversation_id: conversation_id.to_owned(),
            message_key: message_key.to_owned(),
            open_url: open_url.to_owned(),
        }
    }
}

#[derive(Default)]
pub struct Downloads {
    states: HashMap<DownloadKey, DownloadState>,
}

impl Downloads {
    pub fn state(&self, key: &DownloadKey) -> Option<&DownloadState> {
        self.states.get(key)
    }

    pub fn states_for(
        &self,
        conversation_id: &str,
        message_key: &str,
        open_urls: &[&str],
    ) -> Vec<Option<DownloadState>> {
        open_urls
            .iter()
            .map(|open_url| {
                self.state(&DownloadKey::new(conversation_id, message_key, open_url))
                    .cloned()
            })
            .collect()
    }

    pub fn begin(&mut self, key: &DownloadKey) -> bool {
        if matches!(self.state(key), Some(DownloadState::Saving(_))) {
            return false;
        }
        self.states.insert(key.clone(), DownloadState::Saving(0));
        true
    }

    pub fn set_progress(&mut self, key: &DownloadKey, percent: u8) {
        if let Some(state @ DownloadState::Saving(_)) = self.states.get_mut(key) {
            *state = DownloadState::Saving(percent.min(100));
        }
    }

    pub fn finish(&mut self, key: &DownloadKey, outcome: Result<PathBuf, String>) {
        if !matches!(self.state(key), Some(DownloadState::Saving(_))) {
            return;
        }
        let state = match outcome {
            Ok(path) => DownloadState::Saved(path),
            Err(reason) => DownloadState::Failed(reason),
        };
        self.states.insert(key.clone(), state);
    }
}

fn is_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{ad}'
            | '\u{61c}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper
                .strip_prefix(prefix)
                .is_some_and(|number| matches!(number.as_bytes(), [b'1'..=b'9']))
        })
}

pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|character| !is_format_character(*character))
        .map(|character| {
            if character.is_control() || "/\\:*?\"<>|".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let trimmed = cleaned.trim_start().trim_end_matches([' ', '.']);
    if trimmed.is_empty() {
        FALLBACK_FILE_NAME.to_owned()
    } else if is_reserved_windows_name(trimmed) {
        format!("_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

fn split_extension(file_name: &str) -> (&str, &str) {
    match file_name.rfind('.') {
        Some(dot) if dot > 0 && file_name.len() - dot <= MAX_EXTENSION_BYTES => {
            file_name.split_at(dot)
        }
        _ => (file_name, ""),
    }
}

fn truncate_on_boundary(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn numbered_name(file_name: &str, number: usize) -> String {
    let (stem, extension) = split_extension(file_name);
    let suffix = if number == 0 {
        String::new()
    } else {
        format!(" ({number})")
    };
    let budget = MAX_NAME_BYTES.saturating_sub(PART_SUFFIX.len() + suffix.len() + extension.len());
    format!("{}{suffix}{extension}", truncate_on_boundary(stem, budget))
}

pub fn pick_downloads_directory(download: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    download.or(home).unwrap_or_else(std::env::temp_dir)
}

pub fn downloads_directory() -> PathBuf {
    let directories = directories::UserDirs::new();
    pick_downloads_directory(
        directories
            .as_ref()
            .and_then(|directories| directories.download_dir().map(Path::to_path_buf)),
        directories
            .as_ref()
            .map(|directories| directories.home_dir().to_path_buf()),
    )
}

pub fn demo_directory() -> PathBuf {
    std::env::temp_dir().join(DEMO_DIRECTORY)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevealTarget {
    File(PathBuf),
    Folder(PathBuf),
}

pub fn reveal_target(path: &Path, exists: bool, fallback_folder: PathBuf) -> RevealTarget {
    if exists {
        RevealTarget::File(path.to_path_buf())
    } else {
        RevealTarget::Folder(fallback_folder)
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn explorer_select_argument(path: &Path) -> String {
    format!("/select,\"{}\"", path.display())
}

pub fn reveal_in_file_manager(path: &Path) {
    let mut command = reveal_command(path);
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(windows)]
fn reveal_command(path: &Path) -> std::process::Command {
    use std::os::windows::process::CommandExt as _;
    let mut command = std::process::Command::new("explorer.exe");
    command.raw_arg(explorer_select_argument(path));
    command
}

#[cfg(target_os = "macos")]
fn reveal_command(path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("open");
    command.arg("-R").arg(path);
    command
}

#[cfg(not(any(windows, target_os = "macos")))]
fn reveal_command(path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(path.parent().unwrap_or(path));
    command
}

pub struct PartFile {
    file: Option<File>,
    part: PathBuf,
    target: PathBuf,
    finished: bool,
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(PART_SUFFIX);
    PathBuf::from(name)
}

fn reserve_name(directory: &Path, file_name: &str) -> std::io::Result<(PathBuf, File)> {
    let create_new = |path: &Path| OpenOptions::new().write(true).create_new(true).open(path);
    for number in 0usize.. {
        let candidate = directory.join(numbered_name(file_name, number));
        match create_new(&candidate) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
        match create_new(&part_path(&candidate)) {
            Ok(part) => return Ok((candidate, part)),
            Err(error) => {
                let _ = std::fs::remove_file(&candidate);
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
        }
    }
    unreachable!("the numbering loop only returns")
}

impl PartFile {
    pub fn create(directory: &Path, file_name: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let (target, file) = reserve_name(directory, &safe_file_name(file_name))?;
        Ok(PartFile {
            file: Some(file),
            part: part_path(&target),
            target,
            finished: false,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.write_all(bytes),
            None => Err(std::io::Error::other("the file is already closed")),
        }
    }

    pub fn finish(mut self) -> std::io::Result<PathBuf> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        std::fs::rename(&self.part, &self.target)?;
        self.finished = true;
        Ok(self.target.clone())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.finished {
            self.file.take();
            let _ = std::fs::remove_file(&self.part);
            let _ = std::fs::remove_file(&self.target);
        }
    }
}

pub fn write_demo_file(file_name: &str) -> std::io::Result<PathBuf> {
    let mut part = PartFile::create(&demo_directory(), file_name)?;
    part.write(DEMO_CONTENT)?;
    part.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_goes_before_the_extension_in_brackets() {
        assert_eq!(numbered_name("plan.pdf", 0), "plan.pdf");
        assert_eq!(numbered_name("plan.pdf", 1), "plan (1).pdf");
        assert_eq!(numbered_name("plan.pdf", 12), "plan (12).pdf");
        assert_eq!(numbered_name("a.tar.gz", 2), "a.tar (2).gz");
        assert_eq!(numbered_name("README", 3), "README (3)");
        assert_eq!(numbered_name(".env", 1), ".env (1)");
    }

    #[test]
    fn numbering_skips_names_that_exist() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("plan.pdf"), b"").unwrap();
        std::fs::write(directory.path().join("plan (1).pdf"), b"").unwrap();
        let (path, _) = reserve_name(directory.path(), "plan.pdf").unwrap();
        assert_eq!(path, directory.path().join("plan (2).pdf"));
        let (next, _) = reserve_name(directory.path(), "plan.pdf").unwrap();
        assert_eq!(next, directory.path().join("plan (3).pdf"));
        assert_eq!(
            reserve_name(directory.path(), "new.pdf").unwrap().0,
            directory.path().join("new.pdf")
        );
    }

    #[test]
    fn a_stale_part_file_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("plan.pdf.part"), b"old").unwrap();
        let (path, _) = reserve_name(directory.path(), "plan.pdf").unwrap();
        assert_eq!(path, directory.path().join("plan (1).pdf"));
        assert!(!directory.path().join("plan.pdf").exists());
        assert_eq!(
            std::fs::read(directory.path().join("plan.pdf.part")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn concurrent_saves_of_one_name_get_separate_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = PartFile::create(directory.path(), "a.txt").unwrap();
        let mut second = PartFile::create(directory.path(), "a.txt").unwrap();
        first.write(b"1").unwrap();
        second.write(b"2").unwrap();
        let first_path = first.finish().unwrap();
        let second_path = second.finish().unwrap();
        assert_eq!(std::fs::read(first_path).unwrap(), b"1");
        assert_eq!(std::fs::read(second_path).unwrap(), b"2");
    }

    #[test]
    fn names_are_cut_so_the_part_file_fits_255_bytes() {
        let long = format!("{}.pdf", "x".repeat(400));
        for number in [0, 1, 123] {
            let name = numbered_name(&long, number);
            assert!(name.len() + PART_SUFFIX.len() <= 255, "{}", name.len());
            assert!(name.ends_with(".pdf"));
        }
        assert!(numbered_name(&long, 12).ends_with(" (12).pdf"));
        let wide = format!("{}.txt", "\u{e4}".repeat(200));
        let name = numbered_name(&wide, 1);
        assert!(name.len() + PART_SUFFIX.len() <= 255);
        assert!(name.ends_with(" (1).txt"));
        let huge_extension = format!("a.{}", "e".repeat(400));
        assert!(numbered_name(&huge_extension, 0).len() + PART_SUFFIX.len() <= 255);
    }

    #[test]
    fn trailing_spaces_and_dots_go_together() {
        assert_eq!(safe_file_name("report ."), "report");
        assert_eq!(safe_file_name("report. . "), "report");
        assert_eq!(safe_file_name("  a.txt"), "a.txt");
    }

    #[test]
    fn windows_reserved_names_get_a_prefix() {
        assert_eq!(safe_file_name("CON"), "_CON");
        assert_eq!(safe_file_name("nul.txt"), "_nul.txt");
        assert_eq!(safe_file_name("Com1.tar.gz"), "_Com1.tar.gz");
        assert_eq!(safe_file_name("lpt9"), "_lpt9");
        assert_eq!(safe_file_name("COM0"), "COM0");
        assert_eq!(safe_file_name("console.txt"), "console.txt");
    }

    #[test]
    fn direction_overrides_and_format_characters_are_removed() {
        assert_eq!(safe_file_name("a\u{202e}fdp.exe"), "afdp.exe");
        assert_eq!(safe_file_name("\u{2066}b\u{2069}.txt\u{200f}"), "b.txt");
        assert_eq!(safe_file_name("\u{feff}c.txt"), "c.txt");
    }

    #[test]
    fn file_names_lose_path_parts_and_reserved_characters() {
        assert_eq!(safe_file_name("../a/b.pdf"), ".._a_b.pdf");
        assert_eq!(safe_file_name("q: 1?.txt"), "q_ 1_.txt");
        assert_eq!(safe_file_name("  "), "file");
        assert_eq!(safe_file_name("..."), "file");
    }

    #[test]
    fn downloads_directory_falls_back_to_home_then_temp() {
        let download = Some(PathBuf::from("/dl"));
        let home = Some(PathBuf::from("/home/me"));
        assert_eq!(
            pick_downloads_directory(download.clone(), home.clone()),
            PathBuf::from("/dl")
        );
        assert_eq!(
            pick_downloads_directory(None, home),
            PathBuf::from("/home/me")
        );
        assert_eq!(pick_downloads_directory(None, None), std::env::temp_dir());
    }

    #[test]
    fn click_starts_reveals_or_waits_by_state() {
        let path = PathBuf::from("/d/a.pdf");
        assert_eq!(click_action(None), ClickAction::Start);
        assert_eq!(
            click_action(Some(&DownloadState::Failed("x".into()))),
            ClickAction::Start
        );
        assert_eq!(
            click_action(Some(&DownloadState::Saving(5))),
            ClickAction::Ignore
        );
        assert_eq!(
            click_action(Some(&DownloadState::Saved(path.clone()))),
            ClickAction::Reveal(path)
        );
    }

    fn key(message: &str, url: &str) -> DownloadKey {
        DownloadKey::new("chat", message, url)
    }

    #[test]
    fn states_move_from_saving_to_saved_or_failed() {
        let mut downloads = Downloads::default();
        let key = key("m1", "u");
        assert!(downloads.begin(&key));
        assert!(!downloads.begin(&key));
        downloads.set_progress(&key, 42);
        assert_eq!(downloads.state(&key), Some(&DownloadState::Saving(42)));
        downloads.finish(&key, Err("offline".into()));
        assert_eq!(
            downloads.state(&key),
            Some(&DownloadState::Failed("offline".into()))
        );
        assert!(downloads.begin(&key));
        assert_eq!(downloads.state(&key), Some(&DownloadState::Saving(0)));
        downloads.finish(&key, Ok(PathBuf::from("/d/a.pdf")));
        assert_eq!(
            downloads.state(&key),
            Some(&DownloadState::Saved(PathBuf::from("/d/a.pdf")))
        );
    }

    #[test]
    fn late_progress_and_results_do_not_change_settled_states() {
        let mut downloads = Downloads::default();
        let key = key("m1", "u");
        downloads.set_progress(&key, 10);
        downloads.finish(&key, Ok(PathBuf::from("/x")));
        assert_eq!(downloads.state(&key), None);
        downloads.begin(&key);
        downloads.finish(&key, Ok(PathBuf::from("/d/a.pdf")));
        downloads.set_progress(&key, 50);
        assert!(matches!(
            downloads.state(&key),
            Some(DownloadState::Saved(_))
        ));
    }

    #[test]
    fn states_are_kept_per_conversation_message_and_url() {
        let mut downloads = Downloads::default();
        downloads.begin(&DownloadKey::new("chat", "m1", "u2"));
        downloads.begin(&DownloadKey::new("chat", "m2", "u1"));
        downloads.begin(&DownloadKey::new("other", "m1", "u1"));
        let states = downloads.states_for("chat", "m1", &["u1", "u2"]);
        assert_eq!(states, vec![None, Some(DownloadState::Saving(0))]);
    }

    #[test]
    fn button_follows_the_state() {
        let idle = button_look(None);
        assert_eq!(
            (idle.symbol, idle.tooltip.as_str()),
            ("download", "Save to Downloads")
        );
        assert!(idle.clickable);
        let saving = button_look(Some(&DownloadState::Saving(42)));
        assert_eq!(saving.percent, Some(42));
        assert!(!saving.clickable);
        let saved = button_look(Some(&DownloadState::Saved(PathBuf::from("/a"))));
        assert_eq!(
            (saved.symbol, saved.tooltip.as_str()),
            ("folder_open", "Show in folder")
        );
        let failed = button_look(Some(&DownloadState::Failed("HTTP 404".into())));
        assert_eq!(failed.symbol, "refresh");
        assert_eq!(failed.tooltip, "Save failed: HTTP 404. Click to retry");
        assert_eq!(percent_label(42), "42 %");
    }

    #[test]
    fn progress_bar_only_while_saving() {
        assert_eq!(
            progress_fraction(Some(&DownloadState::Saving(50))),
            Some(0.5)
        );
        assert_eq!(progress_fraction(None), None);
        assert_eq!(
            progress_fraction(Some(&DownloadState::Saved(PathBuf::new()))),
            None
        );
    }

    #[test]
    fn missing_file_falls_back_to_the_folder() {
        let path = Path::new("/d/a.pdf");
        assert_eq!(
            reveal_target(path, true, PathBuf::from("/d")),
            RevealTarget::File(path.to_path_buf())
        );
        assert_eq!(
            reveal_target(path, false, PathBuf::from("/d")),
            RevealTarget::Folder(PathBuf::from("/d"))
        );
    }

    #[test]
    fn explorer_gets_the_path_quoted_after_select() {
        assert_eq!(
            explorer_select_argument(Path::new("C:\\Users\\me\\Downloads\\a b.pdf")),
            "/select,\"C:\\Users\\me\\Downloads\\a b.pdf\""
        );
    }

    #[test]
    fn part_file_is_renamed_on_finish() {
        let directory = tempfile::tempdir().unwrap();
        let mut part = PartFile::create(directory.path(), "a.txt").unwrap();
        part.write(b"hi").unwrap();
        assert!(directory.path().join("a.txt.part").exists());
        let path = part.finish().unwrap();
        assert_eq!(path, directory.path().join("a.txt"));
        assert_eq!(std::fs::read(&path).unwrap(), b"hi");
        assert!(!directory.path().join("a.txt.part").exists());
    }

    #[test]
    fn part_file_is_removed_when_dropped_unfinished() {
        let directory = tempfile::tempdir().unwrap();
        let mut part = PartFile::create(directory.path(), "a.txt").unwrap();
        part.write(b"hi").unwrap();
        drop(part);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_file_that_appears_meanwhile_is_not_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let part = PartFile::create(directory.path(), "a.txt").unwrap();
        let other = PartFile::create(directory.path(), "a.txt").unwrap();
        drop(part);
        std::fs::write(directory.path().join("a.txt"), b"browser").unwrap();
        let mut other = other;
        other.write(b"ours").unwrap();
        assert_eq!(other.finish().unwrap(), directory.path().join("a (1).txt"));
        assert_eq!(
            std::fs::read(directory.path().join("a.txt")).unwrap(),
            b"browser"
        );
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.txt"), b"old").unwrap();
        let mut part = PartFile::create(directory.path(), "a.txt").unwrap();
        part.write(b"new").unwrap();
        let path = part.finish().unwrap();
        assert_eq!(path, directory.path().join("a (1).txt"));
        assert_eq!(
            std::fs::read(directory.path().join("a.txt")).unwrap(),
            b"old"
        );
    }
}
