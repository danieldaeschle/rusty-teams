//! Which browser the app signs in with. The user picks one in Settings; Automatic takes the
//! system's default browser when it is Chromium- or Firefox-based, else the first one installed.
//! The choice lives in a file beside the browser profiles, since every org shares them.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::config::{
    Engine, LINUX_CHROME_NAMES, LINUX_FIREFOX_NAMES, MACOS_CHROME_APP_PATHS,
    MACOS_FIREFOX_APP_PATHS, Platform, application_roots,
};

const CHOICE_FILE: &str = "rusty-teams-sign-in-browser";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInBrowser {
    Automatic,
    Program(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledBrowser {
    pub name: String,
    pub path: PathBuf,
    pub engine: Engine,
}

fn choice_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.data_dir().join(CHOICE_FILE))
}

impl SignInBrowser {
    pub fn load() -> SignInBrowser {
        choice_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| SignInBrowser::parse(&text))
            .unwrap_or(SignInBrowser::Automatic)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = choice_path().ok_or_else(|| std::io::Error::other("no data directory"))?;
        match self {
            SignInBrowser::Automatic => match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
            SignInBrowser::Program(program) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, program.to_string_lossy().as_bytes())
            }
        }
    }

    fn parse(text: &str) -> SignInBrowser {
        match text.trim() {
            "" => SignInBrowser::Automatic,
            path => SignInBrowser::Program(PathBuf::from(path)),
        }
    }
}

/// Chromium and Firefox browsers on this machine, Chromium first.
pub fn installed_browsers() -> Vec<InstalledBrowser> {
    let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    installed_in(
        Platform::detect(),
        home.as_deref(),
        &crate::host::path_variable(),
        &crate::host::is_file,
    )
}

/// The browser Automatic stands for.
pub fn automatic_browser(installed: &[InstalledBrowser]) -> Option<&InstalledBrowser> {
    automatic_in(Platform::detect(), installed)
}

fn automatic_in(platform: Platform, installed: &[InstalledBrowser]) -> Option<&InstalledBrowser> {
    system_default(platform, installed).or(installed.first())
}

/// The executable to sign in with: the saved choice while it is installed, else Automatic.
pub(crate) fn chosen_executable(platform: Platform, home: Option<&Path>) -> Option<PathBuf> {
    if !platform.uses_unix_paths() {
        return None;
    }
    if let SignInBrowser::Program(path) = SignInBrowser::load()
        && crate::host::is_file(&path)
    {
        return Some(path);
    }
    let installed = installed_in(
        platform,
        home,
        &crate::host::path_variable(),
        &crate::host::is_file,
    );
    automatic_in(platform, &installed).map(|browser| browser.path.clone())
}

pub(crate) fn installed_in(
    platform: Platform,
    home: Option<&Path>,
    path_variable: &std::ffi::OsStr,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<InstalledBrowser> {
    let found: Vec<(String, PathBuf)> = match platform {
        Platform::Macos => MACOS_CHROME_APP_PATHS
            .iter()
            .chain(&MACOS_FIREFOX_APP_PATHS)
            .filter_map(|relative| {
                let path = application_roots(home)
                    .map(|root| root.join(relative))
                    .find(|path| exists(path))?;
                let name = relative.split(".app/").next().unwrap_or(relative);
                Some((name.to_owned(), path))
            })
            .collect(),
        Platform::Linux => {
            let mut found = Vec::new();
            let mut seen = Vec::new();
            for name in LINUX_CHROME_NAMES.iter().chain(LINUX_FIREFOX_NAMES.iter()) {
                let Some(path) = std::env::split_paths(path_variable)
                    .map(|directory| directory.join(name))
                    .find(|path| exists(path))
                else {
                    continue;
                };
                let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                found.push(((*name).to_owned(), path));
            }
            found
        }
        Platform::Windows | Platform::Wsl => Vec::new(),
    };
    found
        .into_iter()
        .map(|(name, path)| InstalledBrowser {
            engine: Engine::of_executable(&path),
            name,
            path,
        })
        .collect()
}

fn system_default(platform: Platform, installed: &[InstalledBrowser]) -> Option<&InstalledBrowser> {
    match platform {
        Platform::Macos => {
            let wanted = macos_default_bundle_id()?;
            installed.iter().find(|browser| {
                bundle_id(&browser.path).is_some_and(|id| id.eq_ignore_ascii_case(&wanted))
            })
        }
        Platform::Linux => {
            let output = crate::host::command("xdg-settings")
                .args(["get", "default-web-browser"])
                .output()
                .ok()?;
            desktop_entry_match(&String::from_utf8_lossy(&output.stdout), installed)
        }
        Platform::Windows | Platform::Wsl => None,
    }
}

fn plist_json(path: &Path) -> Option<Value> {
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .output()
        .ok()?;
    serde_json::from_slice(&output.stdout).ok()
}

fn macos_default_bundle_id() -> Option<String> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    let handlers = home
        .join("Library/Preferences/com.apple.LaunchServices/com.apple.launchservices.secure.plist");
    handler_bundle_id(&plist_json(&handlers)?)
}

/// The app LaunchServices opens https links with, falling back to http.
fn handler_bundle_id(handlers: &Value) -> Option<String> {
    let entries = handlers["LSHandlers"].as_array()?;
    ["https", "http"].iter().find_map(|scheme| {
        entries
            .iter()
            .find(|entry| entry["LSHandlerURLScheme"] == *scheme)?["LSHandlerRoleAll"]
            .as_str()
            .map(str::to_owned)
    })
}

/// `CFBundleIdentifier` of the app bundle holding `executable` (`X.app/Contents/MacOS/x`).
fn bundle_id(executable: &Path) -> Option<String> {
    let info = executable.parent()?.parent()?.join("Info.plist");
    plist_json(&info)?["CFBundleIdentifier"]
        .as_str()
        .map(str::to_owned)
}

/// The installed browser named like the desktop entry `xdg-settings` reports, as in `firefox.desktop`.
/// Edge Stable's desktop file is `microsoft-edge.desktop` while the executable is
/// `microsoft-edge-stable`. Brave's desktop file is `brave-browser.desktop` while the
/// executable on some distributions is `brave`.
fn desktop_entry_match<'a>(
    entry: &str,
    installed: &'a [InstalledBrowser],
) -> Option<&'a InstalledBrowser> {
    let stem = entry.trim().trim_end_matches(".desktop");
    let names: &[&str] = match stem {
        "microsoft-edge" => &[
            "microsoft-edge",
            "microsoft-edge-stable",
            "microsoft-edge-beta",
            "microsoft-edge-dev",
        ],
        "brave-browser" => &[
            "brave",
            "brave-browser",
            "brave-browser-stable",
            "brave-browser-beta",
            "brave-browser-nightly",
        ],
        other => return installed.iter().find(|browser| browser.name == other),
    };
    names
        .iter()
        .find_map(|name| installed.iter().find(|browser| browser.name == *name))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use serde_json::json;

    use super::*;

    #[test]
    fn finds_installed_browsers_chromium_first_once_each() {
        let installed = [
            "/Applications/Zen.app/Contents/MacOS/zen",
            "/Users/joe/Applications/Zen.app/Contents/MacOS/zen",
            "/Users/joe/Applications/Helium.app/Contents/MacOS/Helium",
        ];
        let found = installed_in(
            Platform::Macos,
            Some(Path::new("/Users/joe")),
            &OsString::new(),
            &|path| installed.iter().any(|known| path == Path::new(known)),
        );
        let names: Vec<(&str, Engine)> = found
            .iter()
            .map(|browser| (browser.name.as_str(), browser.engine))
            .collect();
        assert_eq!(
            names,
            [("Helium", Engine::Chromium), ("Zen", Engine::Firefox)]
        );
        assert_eq!(found[1].path, PathBuf::from(installed[0]));
    }

    #[test]
    fn reads_the_https_handler() {
        let handlers = json!({"LSHandlers": [
            {"LSHandlerContentType": "public.html", "LSHandlerRoleAll": "com.apple.safari"},
            {"LSHandlerURLScheme": "http", "LSHandlerRoleAll": "org.mozilla.firefox"},
            {"LSHandlerURLScheme": "https", "LSHandlerRoleAll": "app.zen-browser.zen"},
        ]});
        assert_eq!(
            handler_bundle_id(&handlers).as_deref(),
            Some("app.zen-browser.zen")
        );
        assert_eq!(handler_bundle_id(&json!({"LSHandlers": []})), None);
    }

    #[test]
    fn matches_the_linux_desktop_entry_by_name() {
        let installed = installed_in(
            Platform::Linux,
            None,
            &std::env::join_paths(["/usr/bin"]).unwrap(),
            &|path| path == Path::new("/usr/bin/chromium") || path == Path::new("/usr/bin/firefox"),
        );
        let found = desktop_entry_match("firefox.desktop\n", &installed).unwrap();
        assert_eq!(found.engine, Engine::Firefox);
        assert!(desktop_entry_match("org.gnome.Epiphany.desktop", &installed).is_none());
    }

    #[test]
    fn lists_edge_and_brave_and_matches_their_desktop_files() {
        let root = Path::new("/usr/bin");
        let installed = installed_in(
            Platform::Linux,
            None,
            &std::env::join_paths([root]).unwrap(),
            &|path| {
                path == root.join("microsoft-edge-stable")
                    || path == root.join("brave")
                    || path == root.join("firefox")
            },
        );
        let names: Vec<&str> = installed
            .iter()
            .map(|browser| browser.name.as_str())
            .collect();
        assert_eq!(names, ["microsoft-edge-stable", "brave", "firefox"]);
        assert_eq!(
            desktop_entry_match("microsoft-edge.desktop\n", &installed)
                .map(|browser| browser.name.as_str()),
            Some("microsoft-edge-stable")
        );
        assert_eq!(
            desktop_entry_match("brave-browser.desktop", &installed)
                .map(|browser| browser.name.as_str()),
            Some("brave")
        );
        assert_eq!(installed[0].engine, Engine::Chromium);
        assert_eq!(installed[1].engine, Engine::Chromium);
    }

    #[test]
    fn parses_the_saved_choice() {
        assert_eq!(SignInBrowser::parse(" \n"), SignInBrowser::Automatic);
        assert_eq!(
            SignInBrowser::parse("/Applications/Zen.app/Contents/MacOS/zen\n"),
            SignInBrowser::Program(PathBuf::from("/Applications/Zen.app/Contents/MacOS/zen"))
        );
    }
}
