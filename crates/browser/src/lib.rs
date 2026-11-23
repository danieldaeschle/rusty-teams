mod bidi;
mod bidi_transport;
mod browser;
mod config;
mod devtools_port;
mod error;
mod host;
mod http;
mod launch;
mod process;
mod sign_in_choice;
mod targets;
mod watchdog;

pub use bidi::{Bidi, Context, remote_value_to_json};
pub use bidi_transport::{BidiTransport, session_transport};
pub use browser::{Browser, RendererState, Status, Timeouts};
pub use config::{
    Config, ConfigInputs, DEFAULT_PORT, Engine, FIREFOX_PROFILE_NAME, PROFILE_NAME, Platform,
    windows_path_to_wsl,
};
pub use error::BrowserError;
pub use launch::{
    ArgumentSpec, LaunchRequest, Launcher, SystemLauncher, chrome_arguments, firefox_arguments,
    user_agent,
};
pub use sign_in_choice::{InstalledBrowser, SignInBrowser, automatic_browser, installed_browsers};
pub use targets::{APPS, App, LoginState, Mode, Tab};
pub use watchdog::{BrowserEvent, Watchdog, WatchdogConfig};
