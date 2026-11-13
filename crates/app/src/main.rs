#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod active_appearance;
mod active_locale;
mod agent_direct;
pub mod agents;
pub mod call;
mod i18n;
mod inline_video;
#[cfg(test)]
mod layout_guard;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod linux_browser;
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
use crate::call::remote_command::{
    ADMIT_ALL_FLAG, CALL_FLAG, HANG_UP_FLAG, JOIN_FLAG, RemoteCommand, UNMUTE_FLAG,
};
use views::shell::{AppShell, OpenTarget, Startup, bind_keys};

const LINUX_APP_ID: &str = "rusty-teams";
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
#[cfg(windows)]
const DEMO_WEBVIEW_FOLDER: &str = "rusty-teams-demo-WebView2";

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
    autostart: bool,
    demo_sync: Option<Duration>,
    remote_command: Option<RemoteCommand>,
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
            autostart::AUTOSTART_FLAG => arguments.autostart = true,
            "--demo-sync" => {
                arguments.demo_sync = input
                    .next()
                    .and_then(|value| value.parse().ok())
                    .map(Duration::from_secs_f32)
            }
            JOIN_FLAG => arguments.remote_command = input.next().map(RemoteCommand::Join),
            HANG_UP_FLAG => arguments.remote_command = Some(RemoteCommand::HangUp),
            ADMIT_ALL_FLAG => arguments.remote_command = Some(RemoteCommand::AdmitAll),
            UNMUTE_FLAG => arguments.remote_command = Some(RemoteCommand::Unmute),
            CALL_FLAG => {
                arguments.remote_command =
                    input.next().and_then(|text| RemoteCommand::decode(&text))
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

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

fn data_path(name: &str) -> PathBuf {
    ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn transport(
    endpoint: Option<&str>,
    database: Option<&std::path::Path>,
    #[cfg(windows)] sign_in_title: String,
) -> Arc<dyn session::Transport> {
    #[cfg(windows)]
    if endpoint.is_none() {
        let user_data_folder = match database.and_then(std::path::Path::parent) {
            Some(directory) => directory.join(WEBVIEW_FOLDER),
            None => data_path(WEBVIEW_FOLDER),
        };
        inline_video::configure(user_data_folder.clone());
        let transport = webview::start(webview::HostConfig {
            user_data_folder,
            window_title: sign_in_title,
            log: |line| app_base::diagnostics::record(line),
        });
        task_dialog::install_host(transport.clone());
        return transport;
    }
    #[cfg(not(windows))]
    let _ = database;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if endpoint.is_none() {
        return linux_browser::start(session::DEFAULT_ENDPOINT);
    }
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
    session::ensure_crypto_provider();
    app_base::version::set_build_version(VERSION_MARKER.trim_start_matches("teams-build:"));
    if let Some(tag) = option_env!("TEAMS_RELEASE_TAG") {
        app_base::version::set_release_tag(tag);
    }
    crash_log::install(data_path(crash_log::CRASH_LOG_FILE));
    app_base::native_dumps::install(
        data_path("native-dumps"),
        data_path(crash_log::CRASH_LOG_FILE),
    );
    diagnostics::install(data_path(diagnostics::DIAGNOSTICS_FILE));
    #[cfg(target_os = "linux")]
    app_base::linux_fonts::hide_system_color_emoji();
    let arguments = parse_arguments();
    if !arguments.demo && arguments.database.is_none() {
        let relaunch_value = std::env::var(updater::RELAUNCH_ENV).ok();
        if let Some(value) = &relaunch_value {
            updater::wait_for_previous_exit(value);
        }
        let relaunched = relaunch_value.is_some();
        let command = arguments
            .remote_command
            .as_ref()
            .map(RemoteCommand::encode)
            .unwrap_or_default();
        if single_instance::claim(SINGLE_INSTANCE_NAME, relaunched, &command)
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
    #[cfg(windows)]
    if arguments.demo {
        inline_video::configure(std::env::temp_dir().join(DEMO_WEBVIEW_FOLDER));
    }
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
    app_base::ui_locale::apply_locale(app_base::ui_locale::effective_locale(
        app_base::ui_locale::load(&store),
    ));
    #[cfg(windows)]
    let sign_in_title = format!(
        "{} - {}",
        app_base::APP_NAME,
        app_base::ui_locale::tr("auth.sign_in")
    );
    let user_input = user_activity::UserInput::new();
    let receiver = (!arguments.demo).then(|| {
        let (ui_events, ui_receiver) = tokio::sync::mpsc::unbounded_channel();
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        linux_browser::attach_ui_events(ui_events.clone());
        #[cfg(windows)]
        let transport = transport(
            arguments.endpoint.as_deref(),
            arguments.database.as_deref(),
            sign_in_title.clone(),
        );
        #[cfg(not(windows))]
        let transport = transport(arguments.endpoint.as_deref(), arguments.database.as_deref());
        let activity = user_activity::track(user_input.clone());
        let mut backend_events =
            backend::start(store.clone(), transport, !arguments.read_only, activity);
        app_base::runtime::handle().spawn(async move {
            while let Some(event) = backend_events.recv().await {
                if ui_events.send(event).is_err() {
                    break;
                }
            }
        });
        ui_receiver
    });

    let startup_zoom = appearance::load(&store).zoom;
    let platform = zoom::wrap(gpui_kit::platform::current_platform(false), startup_zoom);
    let mut application = Application::with_platform(platform);
    if let Ok(http_client) = reqwest_client::ReqwestClient::user_agent(HTTP_USER_AGENT) {
        application = application.with_http_client(Arc::new(http_client));
    }

    application
        .with_assets(app_base::assets::AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.on_app_quit(|_| {
                updater::exit_after_grace();
                async {}
            })
            .detach();
            theme::load_fonts(cx);
            active_appearance::install(&store, cx);
            active_locale::install(&store, cx);
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
            agent_direct::install(state.clone(), cx);
            if arguments.demo {
                demo::start_session(&state, cx);
            }
            if let Some(mut receiver) = receiver {
                let state = state.clone();
                cx.spawn(async move |cx| {
                    while let Some(event) = receiver.recv().await {
                        let kind = event.kind();
                        let started = std::time::Instant::now();
                        #[cfg(any(target_os = "linux", target_os = "macos"))]
                        linux_browser::note_backend_event(&event);
                        state.update(cx, |state, cx| state.apply(event, cx));
                        app_base::diagnostics::record_if_slow(&kind, started.elapsed());
                    }
                })
                .detach();
            }

            let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
            let mut options = TitleBar::window_options();
            options.window_bounds = Some(WindowBounds::Windowed(bounds));
            options.window_min_size = Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT)));
            options.app_id = Some(LINUX_APP_ID.to_owned());
            if let Some(titlebar) = options.titlebar.as_mut() {
                titlebar.title = Some(APP_NAME.into());
            }
            let open_target = arguments.open_target;
            let startup = Startup {
                switcher_query: arguments.switcher_query,
                composer_text: arguments.composer_text,
                reply_to_last: arguments.reply_to_last,
                launched_by_autostart: arguments.autostart,
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
            if !arguments.autostart {
                cx.activate(true);
            }
            cx.spawn(async move |cx| {
                loop {
                    app_base::native_dumps::heartbeat();
                    cx.background_executor().timer(HEARTBEAT_INTERVAL).await;
                }
            })
            .detach();
        });
    presence_link::unregister_on_exit(ENDPOINT_UNREGISTER_TIMEOUT);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    linux_browser::stop();
    #[cfg(windows)]
    updater::exit_now();
}
