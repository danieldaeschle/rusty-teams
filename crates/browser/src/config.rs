use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::BrowserError;

pub const PROFILE_NAME: &str = "rusty-teams-chrome";
pub const FIREFOX_PROFILE_NAME: &str = "rusty-teams-firefox";
/// Path to the sign-in browser executable, overriding the search. Firefox-family names run over WebDriver BiDi.
pub const BROWSER_OVERRIDE_ENV: &str = "RUSTY_TEAMS_SIGN_IN_BROWSER";
pub const DEFAULT_PORT: u16 = 9222;
const CHROME_RELATIVE: &str = r"Google\Chrome\Application\chrome.exe";
const WINDOWS_VARIABLES: [&str; 4] = ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)", "TEMP"];
const LINUX_CHROME_NAMES: [&str; 5] = [
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
];
const MACOS_CHROME_NAMES: [&str; 3] = ["google-chrome", "chromium", "chrome"];
/// Chromium-based browsers in order of preference; each one gets its own profile, so the user's
/// everyday browser stays untouched.
const MACOS_CHROME_APP_PATHS: [&str; 6] = [
    "Google Chrome.app/Contents/MacOS/Google Chrome",
    "Chromium.app/Contents/MacOS/Chromium",
    "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "Brave Browser.app/Contents/MacOS/Brave Browser",
    "Helium.app/Contents/MacOS/Helium",
    "Vivaldi.app/Contents/MacOS/Vivaldi",
];
/// Firefox-based browsers, tried only when no Chromium browser is installed.
const MACOS_FIREFOX_APP_PATHS: [&str; 4] = [
    "Firefox.app/Contents/MacOS/firefox",
    "Zen.app/Contents/MacOS/zen",
    "LibreWolf.app/Contents/MacOS/librewolf",
    "Floorp.app/Contents/MacOS/floorp",
];
const LINUX_FIREFOX_NAMES: [&str; 5] = ["firefox", "zen-browser", "zen", "librewolf", "floorp"];

/// The protocol family of the sign-in browser: Chromium speaks CDP, Firefox WebDriver BiDi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Chromium,
    Firefox,
}

impl Engine {
    pub fn of_executable(path: &Path) -> Engine {
        if crate::process::is_firefox_image(&path.to_string_lossy()) {
            Engine::Firefox
        } else {
            Engine::Chromium
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Wsl,
    Windows,
    Linux,
    Macos,
}

impl Platform {
    pub fn detect() -> Platform {
        if cfg!(windows) {
            return Platform::Windows;
        }
        if cfg!(target_os = "macos") {
            return Platform::Macos;
        }
        let proc_version = std::fs::read_to_string("/proc/version").ok();
        Platform::from_proc_version(proc_version.as_deref())
    }

    pub fn from_proc_version(content: Option<&str>) -> Platform {
        match content {
            Some(text) if text.to_ascii_lowercase().contains("microsoft") => Platform::Wsl,
            _ => Platform::Linux,
        }
    }

    pub fn uses_unix_paths(self) -> bool {
        matches!(self, Platform::Linux | Platform::Macos)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum BrowserSearch {
    Files(Vec<PathBuf>),
    Named(Vec<String>),
    /// Absolute install paths first, then PATH names (macOS Application bundles + CLI).
    FilesThenNamed {
        files: Vec<PathBuf>,
        names: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub platform: Platform,
    pub engine: Engine,
    pub port: u16,
    pub profile: String,
    pub mode_file: PathBuf,
    profile_parent: String,
    mode_parent: PathBuf,
    pub(crate) search: BrowserSearch,
}

pub struct ConfigInputs<'a> {
    pub windows_variables: HashMap<String, String>,
    pub linux_data_dir: Option<PathBuf>,
    pub home_dir: Option<PathBuf>,
    pub to_local: &'a dyn Fn(&str) -> String,
}

impl Config {
    pub fn detect() -> Result<Config, BrowserError> {
        let platform = Platform::detect();
        let windows_variables = match platform {
            Platform::Linux | Platform::Macos => HashMap::new(),
            Platform::Windows => WINDOWS_VARIABLES
                .iter()
                .filter_map(|name| {
                    std::env::var(name)
                        .ok()
                        .map(|value| ((*name).to_owned(), value))
                })
                .collect(),
            Platform::Wsl => wsl_windows_variables()?,
        };
        let override_path = std::env::var_os(BROWSER_OVERRIDE_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let base = directories::BaseDirs::new();
        let linux_data_dir = base.as_ref().map(|dirs| dirs.data_dir().to_path_buf());
        let home_dir = base.as_ref().map(|dirs| dirs.home_dir().to_path_buf());
        let to_local = |windows_path: &str| match platform {
            Platform::Wsl => {
                wslpath(windows_path).unwrap_or_else(|| windows_path_to_wsl(windows_path))
            }
            _ => windows_path.to_owned(),
        };
        let config = Config::resolve(
            platform,
            DEFAULT_PORT,
            &ConfigInputs {
                windows_variables,
                linux_data_dir,
                home_dir: home_dir.clone(),
                to_local: &to_local,
            },
        )?;
        let path_variable = std::env::var_os("PATH").unwrap_or_default();
        Ok(config.select_browser(
            override_path,
            home_dir.as_deref(),
            &path_variable,
            &|path| path.is_file(),
        ))
    }

    /// Picks the sign-in browser: the override path, else the first Chromium browser, else the
    /// first Firefox-based one on Linux and macOS. A Firefox browser gets its own profile.
    pub(crate) fn select_browser(
        mut self,
        override_path: Option<PathBuf>,
        home_dir: Option<&Path>,
        path_variable: &std::ffi::OsStr,
        exists: &dyn Fn(&Path) -> bool,
    ) -> Config {
        if let Some(path) = override_path {
            self.engine = Engine::of_executable(&path);
            self.search = BrowserSearch::Files(vec![path]);
        } else if find_browser(&self.search, path_variable, exists).is_err()
            && let Some(search) = firefox_search(self.platform, home_dir)
            && find_browser(&search, path_variable, exists).is_ok()
        {
            self.engine = Engine::Firefox;
            self.search = search;
        }
        if self.engine == Engine::Firefox {
            self = self.with_profile_name(FIREFOX_PROFILE_NAME);
        }
        self
    }

    pub fn resolve(
        platform: Platform,
        port: u16,
        inputs: &ConfigInputs,
    ) -> Result<Config, BrowserError> {
        let to_local = inputs.to_local;
        let missing = |name: &str| BrowserError::Environment(format!("%{name}% is not set"));
        let config = match platform {
            Platform::Linux => {
                let data_dir = inputs
                    .linux_data_dir
                    .clone()
                    .ok_or_else(|| BrowserError::Environment("no XDG data directory".into()))?;
                Config {
                    platform,
                    engine: Engine::Chromium,
                    port,
                    // Built with the platform's '/' like with_profile_name, not the
                    // host's separator: the string is a Linux path whoever builds it.
                    profile: format!("{}/{}", path_text(&data_dir), PROFILE_NAME),
                    mode_file: data_dir.join(format!("{PROFILE_NAME}.mode")),
                    profile_parent: path_text(&data_dir),
                    mode_parent: data_dir,
                    search: BrowserSearch::Named(
                        LINUX_CHROME_NAMES
                            .iter()
                            .map(|name| (*name).to_owned())
                            .collect(),
                    ),
                }
            }
            Platform::Macos => {
                let data_dir = inputs.linux_data_dir.clone().ok_or_else(|| {
                    BrowserError::Environment("no Application Support directory".into())
                })?;
                let files = application_roots(inputs.home_dir.as_deref())
                    .flat_map(|root| MACOS_CHROME_APP_PATHS.map(|relative| root.join(relative)))
                    .collect();
                Config {
                    platform,
                    engine: Engine::Chromium,
                    port,
                    profile: format!("{}/{}", path_text(&data_dir), PROFILE_NAME),
                    mode_file: data_dir.join(format!("{PROFILE_NAME}.mode")),
                    profile_parent: path_text(&data_dir),
                    mode_parent: data_dir,
                    search: BrowserSearch::FilesThenNamed {
                        files,
                        names: MACOS_CHROME_NAMES
                            .iter()
                            .map(|name| (*name).to_owned())
                            .collect(),
                    },
                }
            }
            Platform::Windows | Platform::Wsl => {
                let local = inputs
                    .windows_variables
                    .get("LOCALAPPDATA")
                    .ok_or_else(|| missing("LOCALAPPDATA"))?;
                let candidates = ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
                    .iter()
                    .filter_map(|name| inputs.windows_variables.get(*name))
                    .map(|root| PathBuf::from(to_local(&format!("{root}\\{CHROME_RELATIVE}"))))
                    .collect();
                Config {
                    platform,
                    engine: Engine::Chromium,
                    port,
                    profile: format!("{local}\\{PROFILE_NAME}"),
                    mode_file: PathBuf::from(to_local(&format!("{local}\\{PROFILE_NAME}.mode"))),
                    profile_parent: local.clone(),
                    mode_parent: PathBuf::from(to_local(local)),
                    search: BrowserSearch::Files(candidates),
                }
            }
        };
        Ok(config)
    }

    pub fn with_port(mut self, port: u16) -> Config {
        self.port = port;
        self
    }

    pub fn with_profile_name(mut self, name: &str) -> Config {
        let separator = if self.platform.uses_unix_paths() {
            "/"
        } else {
            "\\"
        };
        self.profile = format!("{}{separator}{name}", self.profile_parent);
        self.mode_file = self.mode_parent.join(format!("{name}.mode"));
        self
    }

    pub fn executable(&self) -> Result<PathBuf, BrowserError> {
        let path_variable = std::env::var_os("PATH").unwrap_or_default();
        find_browser(&self.search, &path_variable, &|path| path.is_file())
    }
}

fn firefox_search(platform: Platform, home_dir: Option<&Path>) -> Option<BrowserSearch> {
    let names = || LINUX_FIREFOX_NAMES.iter().map(|name| (*name).to_owned());
    match platform {
        Platform::Linux => Some(BrowserSearch::Named(names().collect())),
        Platform::Macos => Some(BrowserSearch::FilesThenNamed {
            files: application_roots(home_dir)
                .flat_map(|root| MACOS_FIREFOX_APP_PATHS.map(|relative| root.join(relative)))
                .collect(),
            names: names().collect(),
        }),
        Platform::Windows | Platform::Wsl => None,
    }
}

fn application_roots(home_dir: Option<&Path>) -> impl Iterator<Item = PathBuf> {
    std::iter::once(PathBuf::from("/Applications"))
        .chain(home_dir.map(|home| home.join("Applications")))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn find_named_chrome(
    names: &[String],
    path_variable: &std::ffi::OsStr,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    std::env::split_paths(path_variable)
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| exists(candidate))
}

pub(crate) fn find_browser(
    search: &BrowserSearch,
    path_variable: &std::ffi::OsStr,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf, BrowserError> {
    match search {
        BrowserSearch::Files(candidates) => candidates
            .iter()
            .find(|candidate| exists(candidate))
            .cloned()
            .ok_or_else(|| {
                BrowserError::ChromeNotFound("not under Program Files or LocalAppData".into())
            }),
        BrowserSearch::Named(names) => {
            find_named_chrome(names, path_variable, exists).ok_or_else(|| {
                BrowserError::ChromeNotFound(format!("none of {} on PATH", names.join(", ")))
            })
        }
        BrowserSearch::FilesThenNamed { files, names } => files
            .iter()
            .find(|candidate| exists(candidate))
            .cloned()
            .or_else(|| find_named_chrome(names, path_variable, exists))
            .ok_or_else(|| {
                BrowserError::ChromeNotFound(
                    "no Chromium or Firefox browser under /Applications, ~/Applications, or PATH"
                        .into(),
                )
            }),
    }
}

pub fn windows_path_to_wsl(windows_path: &str) -> String {
    match windows_path.split_once(':') {
        Some((drive, rest)) if drive.len() == 1 => {
            format!(
                "/mnt/{}{}",
                drive.to_ascii_lowercase(),
                rest.replace('\\', "/")
            )
        }
        _ => windows_path.replace('\\', "/"),
    }
}

fn wslpath(windows_path: &str) -> Option<String> {
    let output = Command::new("wslpath")
        .arg("-u")
        .arg(windows_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

pub(crate) fn parse_windows_variables(output: &str) -> Option<HashMap<String, String>> {
    let line = output
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    let values: Vec<&str> = line.split(';').collect();
    if values.len() != WINDOWS_VARIABLES.len() {
        return None;
    }
    let map: HashMap<String, String> = WINDOWS_VARIABLES
        .iter()
        .zip(values)
        .filter(|(name, value)| *value != format!("%{name}%"))
        .map(|(name, value)| ((*name).to_owned(), value.to_owned()))
        .collect();
    map.contains_key("LOCALAPPDATA").then_some(map)
}

fn wsl_windows_variables() -> Result<HashMap<String, String>, BrowserError> {
    let echo = WINDOWS_VARIABLES
        .iter()
        .map(|name| format!("%{name}%"))
        .collect::<Vec<_>>()
        .join(";");
    let output = Command::new("/mnt/c/Windows/System32/cmd.exe")
        .args(["/c", &format!("echo {echo}")])
        .current_dir("/mnt/c")
        .output()
        .map_err(|error| BrowserError::Environment(format!("cmd.exe: {error}")))?;
    parse_windows_variables(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| BrowserError::Environment("cmd.exe returned no %LOCALAPPDATA%".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn windows_inputs(to_local: &dyn Fn(&str) -> String) -> ConfigInputs<'_> {
        let variables = [
            ("LOCALAPPDATA", r"C:\Users\me\AppData\Local"),
            ("PROGRAMFILES", r"C:\Program Files"),
            ("PROGRAMFILES(X86)", r"C:\Program Files (x86)"),
        ];
        ConfigInputs {
            windows_variables: variables
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            linux_data_dir: None,
            home_dir: None,
            to_local,
        }
    }

    #[test]
    fn detects_platform_from_proc_version() {
        let wsl = "Linux version 6.18.33.2-microsoft-standard-WSL2 (root@x)";
        assert_eq!(Platform::from_proc_version(Some(wsl)), Platform::Wsl);
        assert_eq!(
            Platform::from_proc_version(Some("Linux version 6.8.0-generic")),
            Platform::Linux
        );
        assert_eq!(Platform::from_proc_version(None), Platform::Linux);
    }

    #[test]
    fn converts_windows_paths() {
        assert_eq!(
            windows_path_to_wsl(r"C:\Users\me\x.mode"),
            "/mnt/c/Users/me/x.mode"
        );
        assert_eq!(windows_path_to_wsl(r"D:\a"), "/mnt/d/a");
    }

    #[test]
    fn resolves_wsl_config() {
        let to_local = |path: &str| windows_path_to_wsl(path);
        let config = Config::resolve(Platform::Wsl, 9222, &windows_inputs(&to_local)).unwrap();
        assert_eq!(
            config.profile,
            r"C:\Users\me\AppData\Local\rusty-teams-chrome"
        );
        assert_eq!(
            config.mode_file,
            PathBuf::from("/mnt/c/Users/me/AppData/Local/rusty-teams-chrome.mode")
        );
        let BrowserSearch::Files(files) = &config.search else {
            panic!("expected files")
        };
        assert_eq!(
            files[0],
            PathBuf::from("/mnt/c/Program Files/Google/Chrome/Application/chrome.exe")
        );
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn resolves_windows_config_without_conversion() {
        let to_local = |path: &str| path.to_owned();
        let config = Config::resolve(Platform::Windows, 9222, &windows_inputs(&to_local)).unwrap();
        assert_eq!(
            config.mode_file,
            PathBuf::from(r"C:\Users\me\AppData\Local\rusty-teams-chrome.mode")
        );
    }

    #[test]
    fn native_windows_uses_the_app_profile_and_chrome_install_dirs() {
        let to_local = |path: &str| path.to_owned();
        let config = Config::resolve(Platform::Windows, 9222, &windows_inputs(&to_local)).unwrap();
        assert_eq!(
            config.profile,
            r"C:\Users\me\AppData\Local\rusty-teams-chrome"
        );
        let BrowserSearch::Files(files) = &config.search else {
            panic!("expected files")
        };
        let expected = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Users\me\AppData\Local\Google\Chrome\Application\chrome.exe",
        ];
        assert_eq!(
            files
                .iter()
                .map(|file| file.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn native_windows_with_only_localappdata_still_finds_a_per_user_chrome() {
        let to_local = |path: &str| path.to_owned();
        let inputs = ConfigInputs {
            windows_variables: [("LOCALAPPDATA".to_owned(), r"D:\L".to_owned())].into(),
            linux_data_dir: None,
            home_dir: None,
            to_local: &to_local,
        };
        let config = Config::resolve(Platform::Windows, 9222, &inputs).unwrap();
        let BrowserSearch::Files(files) = &config.search else {
            panic!("expected files")
        };
        assert_eq!(
            files,
            &[PathBuf::from(r"D:\L\Google\Chrome\Application\chrome.exe")]
        );
        assert_eq!(config.with_profile_name("x").profile, r"D:\L\x");
    }

    #[test]
    fn wsl_profile_stays_a_windows_path_while_files_are_mounted() {
        let to_local = |path: &str| windows_path_to_wsl(path);
        let config = Config::resolve(Platform::Wsl, 9222, &windows_inputs(&to_local)).unwrap();
        assert!(config.profile.starts_with(r"C:\"));
        assert!(config.mode_file.starts_with("/mnt/c"));
    }

    #[test]
    fn resolves_linux_config_under_xdg_data_dir() {
        let to_local = |path: &str| path.to_owned();
        let inputs = ConfigInputs {
            windows_variables: HashMap::new(),
            linux_data_dir: Some(PathBuf::from("/home/u/.local/share")),
            home_dir: None,
            to_local: &to_local,
        };
        let config = Config::resolve(Platform::Linux, 9222, &inputs).unwrap();
        assert_eq!(config.profile, "/home/u/.local/share/rusty-teams-chrome");
        assert_eq!(
            config.mode_file,
            PathBuf::from("/home/u/.local/share/rusty-teams-chrome.mode")
        );
    }

    #[test]
    fn resolves_macos_config_under_application_support() {
        let to_local = |path: &str| path.to_owned();
        let inputs = ConfigInputs {
            windows_variables: HashMap::new(),
            linux_data_dir: Some(PathBuf::from("/Users/joe/Library/Application Support")),
            home_dir: Some(PathBuf::from("/Users/joe")),
            to_local: &to_local,
        };
        let config = Config::resolve(Platform::Macos, 9222, &inputs).unwrap();
        assert_eq!(
            config.profile,
            "/Users/joe/Library/Application Support/rusty-teams-chrome"
        );
        assert_eq!(
            config.mode_file,
            PathBuf::from("/Users/joe/Library/Application Support/rusty-teams-chrome.mode")
        );
        let BrowserSearch::FilesThenNamed { files, names } = &config.search else {
            panic!("expected files then named")
        };
        assert_eq!(
            files[0],
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        );
        assert_eq!(
            files[MACOS_CHROME_APP_PATHS.len()],
            PathBuf::from("/Users/joe/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        );
        assert!(files.contains(&PathBuf::from(
            "/Applications/Helium.app/Contents/MacOS/Helium"
        )));
        assert!(names.iter().any(|name| name == "google-chrome"));
        assert_eq!(
            config.with_profile_name("x").profile,
            "/Users/joe/Library/Application Support/x"
        );
    }

    #[test]
    fn missing_localappdata_is_an_error() {
        let to_local = |path: &str| path.to_owned();
        let inputs = ConfigInputs {
            windows_variables: HashMap::new(),
            linux_data_dir: None,
            home_dir: None,
            to_local: &to_local,
        };
        assert!(Config::resolve(Platform::Windows, 9222, &inputs).is_err());
    }

    #[test]
    fn finds_macos_chrome_under_applications_before_path() {
        let search = BrowserSearch::FilesThenNamed {
            files: vec![
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
                "/Users/joe/Applications/Chromium.app/Contents/MacOS/Chromium".into(),
            ],
            names: vec!["google-chrome".into()],
        };
        let path = std::env::join_paths(["/usr/local/bin"]).unwrap();
        let found = find_browser(&search, &path, &|candidate| {
            candidate.ends_with("Google Chrome") || candidate.ends_with("google-chrome")
        })
        .unwrap();
        assert_eq!(
            found,
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        );
    }

    #[test]
    fn overrides_port_and_profile_name() {
        let to_local = |path: &str| windows_path_to_wsl(path);
        let config = Config::resolve(Platform::Wsl, 9222, &windows_inputs(&to_local))
            .unwrap()
            .with_port(9333)
            .with_profile_name("rusty-teams-chrome-test");
        assert_eq!(config.port, 9333);
        assert_eq!(
            config.profile,
            r"C:\Users\me\AppData\Local\rusty-teams-chrome-test"
        );
        assert!(config.mode_file.ends_with("rusty-teams-chrome-test.mode"));
    }

    #[test]
    fn parses_cmd_output_with_crlf_and_unset_variables() {
        let output = "\r\nC:\\L;C:\\PF;%PROGRAMFILES(X86)%;C:\\T\r\n";
        let map = parse_windows_variables(output).unwrap();
        assert_eq!(map["LOCALAPPDATA"], r"C:\L");
        assert_eq!(map["TEMP"], r"C:\T");
        assert!(!map.contains_key("PROGRAMFILES(X86)"));
        assert!(parse_windows_variables("%LOCALAPPDATA%;a;b;c").is_none());
        assert!(parse_windows_variables("garbage").is_none());
    }

    #[test]
    fn finds_first_existing_candidate() {
        let search = BrowserSearch::Files(vec!["/a/chrome.exe".into(), "/b/chrome.exe".into()]);
        let found =
            find_browser(&search, &OsString::new(), &|path| path.starts_with("/b")).unwrap();
        assert_eq!(found, PathBuf::from("/b/chrome.exe"));
        assert!(find_browser(&search, &OsString::new(), &|_| false).is_err());
    }

    #[test]
    fn finds_named_chrome_on_path_in_name_priority_per_directory() {
        let search = BrowserSearch::Named(vec!["google-chrome".into(), "chromium".into()]);
        // join_paths picks the host's PATH separator so split_paths splits it here too.
        let path = std::env::join_paths(["/usr/local/bin", "/usr/bin"]).unwrap();
        let found = find_browser(&search, &path, &|candidate| {
            candidate == Path::new("/usr/bin/chromium")
                || candidate == Path::new("/usr/bin/google-chrome")
        })
        .unwrap();
        assert_eq!(found, PathBuf::from("/usr/bin/google-chrome"));
    }

    fn macos_config() -> Config {
        let to_local = |path: &str| path.to_owned();
        let inputs = ConfigInputs {
            windows_variables: HashMap::new(),
            linux_data_dir: Some(PathBuf::from("/Users/joe/Library/Application Support")),
            home_dir: Some(PathBuf::from("/Users/joe")),
            to_local: &to_local,
        };
        Config::resolve(Platform::Macos, 9222, &inputs).unwrap()
    }

    fn select(installed: &[&str], override_path: Option<&str>) -> Config {
        macos_config().select_browser(
            override_path.map(PathBuf::from),
            Some(Path::new("/Users/joe")),
            &OsString::new(),
            &|path| {
                installed
                    .iter()
                    .any(|installed| path == Path::new(installed))
            },
        )
    }

    #[test]
    fn a_chromium_browser_wins_over_firefox() {
        let config = select(
            &[
                "/Applications/Helium.app/Contents/MacOS/Helium",
                "/Applications/Zen.app/Contents/MacOS/zen",
            ],
            None,
        );
        assert_eq!(config.engine, Engine::Chromium);
        assert!(config.profile.ends_with(PROFILE_NAME));
    }

    #[test]
    fn firefox_is_the_fallback_with_its_own_profile() {
        let zen = "/Users/joe/Applications/Zen.app/Contents/MacOS/zen";
        let config = select(&[zen], None);
        assert_eq!(config.engine, Engine::Firefox);
        assert_eq!(
            config.profile,
            "/Users/joe/Library/Application Support/rusty-teams-firefox"
        );
        let found = find_browser(&config.search, &OsString::new(), &|path| {
            path == Path::new(zen)
        });
        assert_eq!(found.unwrap(), PathBuf::from(zen));
    }

    #[test]
    fn the_override_picks_the_browser_and_its_engine() {
        let helium = "/Applications/Helium.app/Contents/MacOS/Helium";
        let firefox = select(&[helium], Some("/opt/zen/zen"));
        assert_eq!(firefox.engine, Engine::Firefox);
        let BrowserSearch::Files(files) = &firefox.search else {
            panic!("expected the override path")
        };
        assert_eq!(files, &[PathBuf::from("/opt/zen/zen")]);
        assert_eq!(select(&[], Some(helium)).engine, Engine::Chromium);
    }
}
