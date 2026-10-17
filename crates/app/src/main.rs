#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod active_appearance;
pub mod agents;
pub mod call;
#[cfg(test)]
mod layout_guard;
pub mod notify;
mod settings;
mod views;

pub use state::*;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use directories::ProjectDirs;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use store::Store;

use crate::app_state::{AppState, Mode};
use views::shell::{AppShell, OpenTarget, Startup, bind_keys};

const VERSION_MARKER: &str = concat!("teams-build:", env!("TEAMS_BUILD_VERSION"));
const HTTP_USER_AGENT: &str = "teams-linux";
const WINDOW_WIDTH: f32 = 1240.;
const WINDOW_HEIGHT: f32 = 820.;
const WINDOW_MIN_WIDTH: f32 = 640.;
const WINDOW_MIN_HEIGHT: f32 = 480.;
const ENDPOINT_UNREGISTER_TIMEOUT: Duration = Duration::from_millis(800);
const SINGLE_INSTANCE_NAME: &str = "rusty-teams";
const DATABASE_FILE: &str = "cache.sqlite3";
#[cfg(windows)]
const WEBVIEW_FOLDER: &str = "WebView2";

#[derive(Default)]
struct Arguments {
    demo: bool,
    read_only: bool,
    database: Option<PathBuf>,
    endpoint: Option<String>,
    open_target: Option<OpenTarget>,
    switcher_query: Option<String>,
    composer_text: Option<String>,
    reply_to_last: bool,
    channels_tab: bool,
    demo_sync: Option<Duration>,
}

fn parse_arguments() -> Arguments {
    let mut arguments = Arguments::default();
    let mut input = std::env::args().skip(1);
    while let Some(flag) = input.next() {
        match flag.as_str() {
            "--demo" => arguments.demo = true,
            "--read-only" => arguments.read_only = true,
            "--switcher" => {
                arguments.switcher_query.get_or_insert_with(String::new);
            }
            "--query" => arguments.switcher_query = input.next(),
            "--type" => arguments.composer_text = input.next(),
            "--reply-last" => arguments.reply_to_last = true,
            "--channels-tab" => arguments.channels_tab = true,
            "--demo-sync" => {
                arguments.demo_sync = input
                    .next()
                    .and_then(|value| value.parse().ok())
                    .map(Duration::from_secs_f32)
            }
            "--database" => arguments.database = input.next().map(PathBuf::from),
            "--endpoint" => arguments.endpoint = input.next(),
            "--open" => {
                arguments.open_target = input
                    .next()
                    .and_then(|value| value.parse().ok())
                    .map(OpenTarget::Chat)
            }
            "--open-channel" => {
                arguments.open_target = input
                    .next()
                    .and_then(|value| value.parse().ok())
                    .map(OpenTarget::Channel)
            }
            _ => {}
        }
    }
    arguments
}

fn data_path(name: &str) -> PathBuf {
    ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn transport(
    endpoint: Option<&str>,
    database: Option<&std::path::Path>,
) -> Arc<dyn session::Transport> {
    #[cfg(windows)]
    if endpoint.is_none() {
        let user_data_folder = match database.and_then(std::path::Path::parent) {
            Some(directory) => directory.join(WEBVIEW_FOLDER),
            None => data_path(WEBVIEW_FOLDER),
        };
        let transport = webview::start(webview::HostConfig {
            user_data_folder,
            window_title: format!("{APP_NAME} - Sign in"),
        });
        task_dialog::install_host(transport.clone());
        return transport;
    }
    #[cfg(not(windows))]
    let _ = database;
    Arc::new(session::CdpTransport::new(
        endpoint.unwrap_or(session::DEFAULT_ENDPOINT),
    ))
}

const STORE_OPEN_ATTEMPTS: u32 = 20;
const STORE_OPEN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

fn open_store(path: &std::path::Path) -> Store {
    for _ in 1..STORE_OPEN_ATTEMPTS {
        match Store::open(path) {
            Ok(store) => return store,
            Err(_) => std::thread::sleep(STORE_OPEN_INTERVAL),
        }
    }
    Store::open(path).expect("cache database")
}

fn main() {
    app_base::version::set_build_version(VERSION_MARKER.trim_start_matches("teams-build:"));
    crash_log::install(data_path(crash_log::CRASH_LOG_FILE));
    diagnostics::install(data_path(diagnostics::DIAGNOSTICS_FILE));
    let arguments = parse_arguments();
    if !arguments.demo && arguments.database.is_none() {
        let relaunch_value = std::env::var(updater::RELAUNCH_ENV).ok();
        if let Some(value) = &relaunch_value {
            updater::wait_for_previous_exit(value);
        }
        let relaunched = relaunch_value.is_some();
        if single_instance::claim(SINGLE_INSTANCE_NAME, relaunched)
            == single_instance::Claim::Forwarded
        {
            return;
        }
    }
    #[cfg(windows)]
    updater::clean_up_old_binary();
    let mode = Mode {
        demo: arguments.demo,
        read_only: arguments.read_only,
        demo_sync: arguments.demo_sync,
    };
    let store = if arguments.demo {
        let store = Store::open_in_memory().expect("in-memory store");
        demo::seed(&store);
        store
    } else {
        let path = arguments
            .database
            .clone()
            .unwrap_or_else(|| data_path(DATABASE_FILE));
        open_store(&path)
    };
    let store = Arc::new(store);
    let user_input = user_activity::UserInput::new();
    let receiver = (!arguments.demo).then(|| {
        let transport = transport(arguments.endpoint.as_deref(), arguments.database.as_deref());
        let activity = user_activity::track(user_input.clone());
        backend::start(store.clone(), transport, !arguments.read_only, activity)
    });

    let mut application = gpui_kit::application();
    if let Ok(http_client) = reqwest_client::ReqwestClient::user_agent(HTTP_USER_AGENT) {
        application = application.with_http_client(Arc::new(http_client));
    }

    application.with_assets(app_base::assets::AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        cx.on_app_quit(|_| {
            updater::exit_after_grace();
            async {}
        })
        .detach();
        theme::load_fonts(cx);
        active_appearance::install(&store, cx);
        theme::apply(cx);
        bind_keys(cx);
        let state = cx.new(|_| {
            let mut state = AppState::new(store, mode);
            state.start_on_channels = arguments.channels_tab;
            state
        });
        views::shell::install_globals(&state, user_input, cx);
        agent_api::init(state.read(cx).store.clone(), arguments.demo, cx);
        agents::install(state.clone(), cx);
        if arguments.demo {
            demo::start_session(&state, cx);
        }
        if let Some(mut receiver) = receiver {
            let state = state.clone();
            cx.spawn(async move |cx| {
                while let Some(event) = receiver.recv().await {
                    state.update(cx, |state, cx| state.apply(event, cx));
                }
            })
            .detach();
        }

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let mut options = TitleBar::window_options();
        options.window_bounds = Some(WindowBounds::Windowed(bounds));
        options.window_min_size = Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT)));
        if let Some(titlebar) = options.titlebar.as_mut() {
            titlebar.title = Some(APP_NAME.into());
        }
        let open_target = arguments.open_target;
        let startup = Startup {
            switcher_query: arguments.switcher_query,
            composer_text: arguments.composer_text,
            reply_to_last: arguments.reply_to_last,
        };
        let (main_window, _) = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| AppShell::new(state, open_target, startup, window, cx))
        })
        .expect("failed to open window");
        let main_window_id = main_window.window_id();
        cx.on_window_closed(move |cx, closed| {
            if closed == main_window_id {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    presence_link::unregister_on_exit(ENDPOINT_UNREGISTER_TIMEOUT);
    #[cfg(windows)]
    updater::exit_now();
}
