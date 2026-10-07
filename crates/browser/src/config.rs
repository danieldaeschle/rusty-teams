use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::BrowserError;

pub const PROFILE_NAME: &str = "rusty-teams-chrome";
pub const DEFAULT_PORT: u16 = 9222;
const CHROME_RELATIVE: &str = r"Google\Chrome\Application\chrome.exe";
const WINDOWS_VARIABLES: [&str; 4] = ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)", "TEMP"];
const LINUX_CHROME_NAMES: [&str; 5] =
    ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Wsl,
    Windows,
    Linux,
}

impl Platform {
    pub fn detect() -> Platform {
        if cfg!(windows) {
            return Platform::Windows;
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
}

#[derive(Debug, Clone)]
pub(crate) enum ChromeSearch {
    Files(Vec<PathBuf>),
    Named(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub platform: Platform,
    pub port: u16,
    pub profile: String,
    pub mode_file: PathBuf,
    profile_parent: String,
    mode_parent: PathBuf,
    pub(crate) chrome_search: ChromeSearch,
}

pub struct ConfigInputs<'a> {
    pub windows_variables: HashMap<String, String>,
    pub linux_data_dir: Option<PathBuf>,
    pub to_local: &'a dyn Fn(&str) -> String,
}

impl Config {
    pub fn detect() -> Result<Config, BrowserError> {
        let platform = Platform::detect();
        let windows_variables = match platform {
            Platform::Linux => HashMap::new(),
            Platform::Windows => WINDOWS_VARIABLES
                .iter()
                .filter_map(|name| std::env::var(name).ok().map(|value| ((*name).to_owned(), value)))
                .collect(),
            Platform::Wsl => wsl_windows_variables()?,
        };
        let linux_data_dir = directories::BaseDirs::new().map(|dirs| dirs.data_dir().to_path_buf());
        let to_local = |windows_path: &str| match platform {
            Platform::Wsl => wslpath(windows_path).unwrap_or_else(|| windows_path_to_wsl(windows_path)),
            _ => windows_path.to_owned(),
        };
        Config::resolve(
            platform,
            DEFAULT_PORT,
            &ConfigInputs { windows_variables, linux_data_dir, to_local: &to_local },
        )
    }

    pub fn resolve(platform: Platform, port: u16, inputs: &ConfigInputs) -> Result<Config, BrowserError> {
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
                    port,
                    profile: path_text(&data_dir.join(PROFILE_NAME)),
                    mode_file: data_dir.join(format!("{PROFILE_NAME}.mode")),
                    profile_parent: path_text(&data_dir),
                    mode_parent: data_dir,
                    chrome_search: ChromeSearch::Named(
                        LINUX_CHROME_NAMES.iter().map(|name| (*name).to_owned()).collect(),
                    ),
                }
            }
            Platform::Windows | Platform::Wsl => {
                let local = inputs.windows_variables.get("LOCALAPPDATA").ok_or_else(|| missing("LOCALAPPDATA"))?;
                let candidates = ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
                    .iter()
                    .filter_map(|name| inputs.windows_variables.get(*name))
                    .map(|root| PathBuf::from(to_local(&format!("{root}\\{CHROME_RELATIVE}"))))
                    .collect();
                Config {
                    platform,
                    port,
                    profile: format!("{local}\\{PROFILE_NAME}"),
                    mode_file: PathBuf::from(to_local(&format!("{local}\\{PROFILE_NAME}.mode"))),
                    profile_parent: local.clone(),
                    mode_parent: PathBuf::from(to_local(local)),
                    chrome_search: ChromeSearch::Files(candidates),
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
        let separator = if self.platform == Platform::Linux { "/" } else { "\\" };
        self.profile = format!("{}{separator}{name}", self.profile_parent);
        self.mode_file = self.mode_parent.join(format!("{name}.mode"));
        self
    }

    pub fn chrome_executable(&self) -> Result<PathBuf, BrowserError> {
        let path_variable = std::env::var_os("PATH").unwrap_or_default();
        find_chrome(&self.chrome_search, &path_variable, &|path| path.is_file())
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn find_chrome(
    search: &ChromeSearch,
    path_variable: &std::ffi::OsStr,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf, BrowserError> {
    match search {
        ChromeSearch::Files(candidates) => candidates
            .iter()
            .find(|candidate| exists(candidate))
            .cloned()
            .ok_or_else(|| BrowserError::ChromeNotFound("not under Program Files or LocalAppData".into())),
        ChromeSearch::Named(names) => std::env::split_paths(path_variable)
            .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
            .find(|candidate| exists(candidate))
            .ok_or_else(|| BrowserError::ChromeNotFound(format!("none of {} on PATH", names.join(", ")))),
    }
}

pub fn windows_path_to_wsl(windows_path: &str) -> String {
    match windows_path.split_once(':') {
        Some((drive, rest)) if drive.len() == 1 => {
            format!("/mnt/{}{}", drive.to_ascii_lowercase(), rest.replace('\\', "/"))
        }
        _ => windows_path.replace('\\', "/"),
    }
}

fn wslpath(windows_path: &str) -> Option<String> {
    let output = Command::new("wslpath").arg("-u").arg(windows_path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

pub(crate) fn parse_windows_variables(output: &str) -> Option<HashMap<String, String>> {
    let line = output.lines().map(str::trim).rfind(|line| !line.is_empty())?;
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
    let echo = WINDOWS_VARIABLES.iter().map(|name| format!("%{name}%")).collect::<Vec<_>>().join(";");
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
            windows_variables: variables.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
            linux_data_dir: None,
            to_local,
        }
    }

    #[test]
    fn detects_platform_from_proc_version() {
        let wsl = "Linux version 6.18.33.2-microsoft-standard-WSL2 (root@x)";
        assert_eq!(Platform::from_proc_version(Some(wsl)), Platform::Wsl);
        assert_eq!(Platform::from_proc_version(Some("Linux version 6.8.0-generic")), Platform::Linux);
        assert_eq!(Platform::from_proc_version(None), Platform::Linux);
    }

    #[test]
    fn converts_windows_paths() {
        assert_eq!(windows_path_to_wsl(r"C:\Users\me\x.mode"), "/mnt/c/Users/me/x.mode");
        assert_eq!(windows_path_to_wsl(r"D:\a"), "/mnt/d/a");
    }

    #[test]
    fn resolves_wsl_config() {
        let to_local = |path: &str| windows_path_to_wsl(path);
        let config = Config::resolve(Platform::Wsl, 9222, &windows_inputs(&to_local)).unwrap();
        assert_eq!(config.profile, r"C:\Users\me\AppData\Local\rusty-teams-chrome");
        assert_eq!(
            config.mode_file,
            PathBuf::from("/mnt/c/Users/me/AppData/Local/rusty-teams-chrome.mode")
        );
        let ChromeSearch::Files(files) = &config.chrome_search else { panic!("expected files") };
        assert_eq!(files[0], PathBuf::from("/mnt/c/Program Files/Google/Chrome/Application/chrome.exe"));
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn resolves_windows_config_without_conversion() {
        let to_local = |path: &str| path.to_owned();
        let config = Config::resolve(Platform::Windows, 9222, &windows_inputs(&to_local)).unwrap();
        assert_eq!(config.mode_file, PathBuf::from(r"C:\Users\me\AppData\Local\rusty-teams-chrome.mode"));
    }

    #[test]
    fn native_windows_uses_the_app_profile_and_chrome_install_dirs() {
        let to_local = |path: &str| path.to_owned();
        let config = Config::resolve(Platform::Windows, 9222, &windows_inputs(&to_local)).unwrap();
        assert_eq!(config.profile, r"C:\Users\me\AppData\Local\rusty-teams-chrome");
        let ChromeSearch::Files(files) = &config.chrome_search else { panic!("expected files") };
        let expected = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Users\me\AppData\Local\Google\Chrome\Application\chrome.exe",
        ];
        assert_eq!(files.iter().map(|file| file.to_string_lossy().into_owned()).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn native_windows_with_only_localappdata_still_finds_a_per_user_chrome() {
        let to_local = |path: &str| path.to_owned();
        let inputs = ConfigInputs {
            windows_variables: [("LOCALAPPDATA".to_owned(), r"D:\L".to_owned())].into(),
            linux_data_dir: None,
            to_local: &to_local,
        };
        let config = Config::resolve(Platform::Windows, 9222, &inputs).unwrap();
        let ChromeSearch::Files(files) = &config.chrome_search else { panic!("expected files") };
        assert_eq!(files, &[PathBuf::from(r"D:\L\Google\Chrome\Application\chrome.exe")]);
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
            to_local: &to_local,
        };
        let config = Config::resolve(Platform::Linux, 9222, &inputs).unwrap();
        assert_eq!(config.profile, "/home/u/.local/share/rusty-teams-chrome");
        assert_eq!(config.mode_file, PathBuf::from("/home/u/.local/share/rusty-teams-chrome.mode"));
    }

    #[test]
    fn missing_localappdata_is_an_error() {
        let to_local = |path: &str| path.to_owned();
        let inputs = ConfigInputs { windows_variables: HashMap::new(), linux_data_dir: None, to_local: &to_local };
        assert!(Config::resolve(Platform::Windows, 9222, &inputs).is_err());
    }

    #[test]
    fn overrides_port_and_profile_name() {
        let to_local = |path: &str| windows_path_to_wsl(path);
        let config = Config::resolve(Platform::Wsl, 9222, &windows_inputs(&to_local))
            .unwrap()
            .with_port(9333)
            .with_profile_name("rusty-teams-chrome-test");
        assert_eq!(config.port, 9333);
        assert_eq!(config.profile, r"C:\Users\me\AppData\Local\rusty-teams-chrome-test");
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
        let search = ChromeSearch::Files(vec!["/a/chrome.exe".into(), "/b/chrome.exe".into()]);
        let found = find_chrome(&search, &OsString::new(), &|path| path.starts_with("/b")).unwrap();
        assert_eq!(found, PathBuf::from("/b/chrome.exe"));
        assert!(find_chrome(&search, &OsString::new(), &|_| false).is_err());
    }

    #[test]
    fn finds_named_chrome_on_path_in_name_priority_per_directory() {
        let search = ChromeSearch::Named(vec!["google-chrome".into(), "chromium".into()]);
        let path = OsString::from("/usr/local/bin:/usr/bin");
        let found = find_chrome(&search, &path, &|candidate| {
            candidate == Path::new("/usr/bin/chromium") || candidate == Path::new("/usr/bin/google-chrome")
        })
        .unwrap();
        assert_eq!(found, PathBuf::from("/usr/bin/google-chrome"));
    }
}
