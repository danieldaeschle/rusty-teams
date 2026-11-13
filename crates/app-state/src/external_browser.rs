//! Preferred browser for external opens (Google OAuth, links).
//!
//! The managed Chrome session for Microsoft Teams login is unchanged (ADR 0003).

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui_kit::App;
use store::Store;

const META_KEY: &str = "external_browser";
const SYSTEM_VALUE: &str = "system";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalBrowser {
    System,
    Program(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserChoice {
    pub id: String,
    pub label: String,
}

impl ExternalBrowser {
    pub fn id(&self) -> &str {
        match self {
            ExternalBrowser::System => SYSTEM_VALUE,
            ExternalBrowser::Program(program) => program.as_str(),
        }
    }
}

pub fn load(store: &Store) -> ExternalBrowser {
    match store.meta(META_KEY).ok().flatten().as_deref() {
        None | Some(SYSTEM_VALUE) | Some("") => ExternalBrowser::System,
        Some(program) => ExternalBrowser::Program(program.to_owned()),
    }
}

pub fn save(store: &Store, browser: &ExternalBrowser) {
    let value = match browser {
        ExternalBrowser::System => SYSTEM_VALUE,
        ExternalBrowser::Program(program) => program.as_str(),
    };
    let _ = store.set_meta(META_KEY, value);
}

pub fn open_url(cx: &mut App, store: &Store, url: &str) {
    match load(store) {
        ExternalBrowser::System => cx.open_url(url),
        ExternalBrowser::Program(program) => {
            if launch_program(&program, url).is_err() {
                cx.open_url(url);
            }
        }
    }
}

fn launch_program(program: &str, url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        if looks_like_app_name(program) {
            Command::new("open").args(["-a", program, url]).spawn()?;
            return Ok(());
        }
    }
    Command::new(program).arg(url).spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn looks_like_app_name(program: &str) -> bool {
    !program.contains('/') && program.chars().any(|c| c.is_uppercase() || c == ' ')
}

pub fn available_choices() -> Vec<BrowserChoice> {
    let mut choices = vec![BrowserChoice {
        id: SYSTEM_VALUE.to_owned(),
        label: SYSTEM_VALUE.to_owned(),
    }];
    for (id, label) in discover_browsers() {
        if choices.iter().any(|choice| choice.id == id) {
            continue;
        }
        choices.push(BrowserChoice { id, label });
    }
    choices
}

pub fn is_system_id(id: &str) -> bool {
    id == SYSTEM_VALUE
}

fn discover_browsers() -> Vec<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        const CANDIDATES: [(&str, &str); 6] = [
            ("google-chrome", "Google Chrome"),
            ("google-chrome-stable", "Google Chrome"),
            ("chromium", "Chromium"),
            ("chromium-browser", "Chromium"),
            ("firefox", "Firefox"),
            ("brave-browser", "Brave"),
        ];
        return CANDIDATES
            .into_iter()
            .filter(|(name, _)| which(name).is_some())
            .map(|(name, label)| (name.to_owned(), label.to_owned()))
            .collect();
    }
    #[cfg(target_os = "macos")]
    {
        let mut found = Vec::new();
        const APPS: [(&str, &str); 4] = [
            ("Google Chrome", "Google Chrome"),
            ("Chromium", "Chromium"),
            ("Firefox", "Firefox"),
            ("Safari", "Safari"),
        ];
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = dirs_home() {
            roots.push(home.join("Applications"));
        }
        for (app, label) in APPS {
            if roots
                .iter()
                .any(|root| root.join(format!("{app}.app")).is_dir())
            {
                found.push((app.to_owned(), label.to_owned()));
            }
        }
        return found;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Vec::new()
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(target_os = "macos")]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::{ExternalBrowser, SYSTEM_VALUE, load, save};
    use store::Store;

    #[test]
    fn defaults_to_system_and_round_trips() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load(&store), ExternalBrowser::System);
        save(&store, &ExternalBrowser::Program("firefox".into()));
        assert_eq!(load(&store), ExternalBrowser::Program("firefox".into()));
        save(&store, &ExternalBrowser::System);
        assert_eq!(load(&store), ExternalBrowser::System);
        assert_eq!(ExternalBrowser::System.id(), SYSTEM_VALUE);
    }
}
