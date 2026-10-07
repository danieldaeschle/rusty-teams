use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::{Config, Platform};
use crate::error::BrowserError;

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub headless: bool,
    pub start_urls: Vec<String>,
}

pub trait Launcher: Send + Sync {
    fn spawn(&self, request: &LaunchRequest) -> Result<(), BrowserError>;
    fn kill_listener(&self, port: u16) -> Result<(), BrowserError>;
}

pub struct ArgumentSpec<'a> {
    pub port: u16,
    pub profile: &'a str,
    pub headless: bool,
    pub user_agent: &'a str,
    pub start_urls: &'a [String],
}

const HEADLESS_MEMORY_FLAGS: [&str; 10] = [
    "--disable-features=Translate,MediaRouter,OptimizationHints,WebUIOmniboxPopup,WebUIOmniboxAimPopup",
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-component-extensions-with-background-pages",
    "--disable-default-apps",
    "--disable-sync",
    "--disable-breakpad",
    "--disable-client-side-phishing-detection",
    "--disable-extensions",
    "--disable-gpu",
];

pub fn chrome_arguments(spec: &ArgumentSpec) -> Vec<String> {
    let mut arguments = vec![
        format!("--remote-debugging-port={}", spec.port),
        format!("--user-data-dir={}", spec.profile),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
    ];
    if spec.headless {
        arguments.extend([
            "--headless=new".into(),
            format!("--user-agent={}", spec.user_agent),
            "--window-size=1400,1000".into(),
        ]);
        arguments.extend(HEADLESS_MEMORY_FLAGS.iter().map(|flag| (*flag).to_owned()));
    }
    arguments.extend(spec.start_urls.iter().cloned());
    arguments
}

pub fn user_agent(platform: Platform, major: u32) -> String {
    let system = if platform == Platform::Linux { "X11; Linux x86_64" } else { "Windows NT 10.0; Win64; x64" };
    format!("Mozilla/5.0 ({system}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36")
}

pub(crate) fn major_from_directory_names<'a>(names: impl Iterator<Item = &'a str>) -> Option<u32> {
    names
        .filter_map(|name| {
            let parts: Vec<&str> = name.split('.').collect();
            let numeric = parts.len() == 4 && parts.iter().all(|part| part.parse::<u32>().is_ok());
            numeric.then(|| parts[0].parse::<u32>().ok())?
        })
        .max()
}

pub(crate) fn major_from_version_output(output: &str) -> Option<u32> {
    output
        .split_whitespace()
        .find_map(|word| word.split('.').next()?.parse::<u32>().ok().filter(|_| word.contains('.')))
}

const FALLBACK_MAJOR: u32 = 150;

fn chrome_major(platform: Platform, executable: &Path) -> u32 {
    let detected = match platform {
        Platform::Linux => Command::new(executable)
            .arg("--version")
            .output()
            .ok()
            .and_then(|output| major_from_version_output(&String::from_utf8_lossy(&output.stdout))),
        _ => executable.parent().and_then(|parent| std::fs::read_dir(parent).ok()).and_then(|entries| {
            let names: Vec<String> =
                entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
            major_from_directory_names(names.iter().map(String::as_str))
        }),
    };
    detected.unwrap_or(FALLBACK_MAJOR)
}

pub struct SystemLauncher {
    config: Config,
}

impl SystemLauncher {
    pub fn new(config: Config) -> SystemLauncher {
        SystemLauncher { config }
    }
}

impl Launcher for SystemLauncher {
    fn spawn(&self, request: &LaunchRequest) -> Result<(), BrowserError> {
        let executable = self.config.chrome_executable()?;
        let agent = user_agent(self.config.platform, chrome_major(self.config.platform, &executable));
        let arguments = chrome_arguments(&ArgumentSpec {
            port: self.config.port,
            profile: &self.config.profile,
            headless: request.headless,
            user_agent: &agent,
            start_urls: &request.start_urls,
        });
        let mut command = Command::new(&executable);
        command.args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if self.config.platform == Platform::Wsl {
            command.current_dir("/mnt/c");
        }
        detach(&mut command);
        let mut child = command.spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    fn kill_listener(&self, port: u16) -> Result<(), BrowserError> {
        crate::process::kill_listener(self.config.platform, port)
    }
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec<'a>(headless: bool, urls: &'a [String]) -> ArgumentSpec<'a> {
        ArgumentSpec { port: 9222, profile: r"C:\p\rusty-teams-chrome", headless, user_agent: "UA", start_urls: urls }
    }

    #[test]
    fn headless_arguments() {
        let urls = vec!["https://teams.cloud.microsoft/".to_owned()];
        let arguments = chrome_arguments(&spec(true, &urls));
        assert_eq!(
            &arguments[..7],
            [
                "--remote-debugging-port=9222",
                r"--user-data-dir=C:\p\rusty-teams-chrome",
                "--no-first-run",
                "--no-default-browser-check",
                "--headless=new",
                "--user-agent=UA",
                "--window-size=1400,1000",
            ]
        );
        assert_eq!(&arguments[7..17], HEADLESS_MEMORY_FLAGS);
        assert_eq!(arguments[17], "https://teams.cloud.microsoft/");
        assert!(!arguments.iter().any(|argument| argument == "--process-per-site"));
    }

    #[test]
    fn visible_arguments_have_no_headless_flags_and_all_urls() {
        let urls = vec!["https://a/".to_owned(), "https://b/".to_owned()];
        let arguments = chrome_arguments(&spec(false, &urls));
        assert!(!arguments.iter().any(|argument| argument.contains("headless") || argument.contains("user-agent")));
        assert_eq!(&arguments[4..], ["https://a/", "https://b/"]);
    }

    #[test]
    fn user_agents_per_platform() {
        assert!(user_agent(Platform::Wsl, 154).contains("Windows NT 10.0; Win64; x64"));
        assert!(user_agent(Platform::Windows, 154).contains("Chrome/154.0.0.0"));
        assert!(user_agent(Platform::Linux, 140).contains("X11; Linux x86_64"));
    }

    #[test]
    fn major_from_version_directories() {
        let names = ["154.0.8037.98", "153.0.1.2", "Dictionaries", "chrome.exe", "1.2.3"];
        assert_eq!(major_from_directory_names(names.into_iter()), Some(154));
        assert_eq!(major_from_directory_names(["x"].into_iter()), None);
    }

    #[test]
    fn major_from_version_text() {
        assert_eq!(major_from_version_output("Google Chrome 154.0.8037.98 \n"), Some(154));
        assert_eq!(major_from_version_output("Chromium 131.0.6778.85 snap"), Some(131));
        assert_eq!(major_from_version_output("nothing"), None);
    }
}
