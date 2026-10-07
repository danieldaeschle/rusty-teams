#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app_state;
mod avatar_image;
mod backend;
mod data;
mod demo;
mod emoji;
mod format;
mod assets;
mod frame_log;
mod fuzzy;
mod render;
mod rows;
mod runtime;
mod sidebar_model;
mod theme;
mod updater;
mod views;

use std::path::PathBuf;
use std::sync::Arc;

use directories::ProjectDirs;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use store::Store;

use app_state::{AppState, Mode};
use views::shell::{AppShell, OpenTarget, Startup, bind_keys};

pub const APP_NAME: &str = "Rusty Teams";
const WINDOW_WIDTH: f32 = 1240.;
const WINDOW_HEIGHT: f32 = 820.;
const WINDOW_MIN_WIDTH: f32 = 640.;
const WINDOW_MIN_HEIGHT: f32 = 480.;
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

fn transport(endpoint: Option<&str>) -> Arc<dyn session::Transport> {
    #[cfg(windows)]
    if endpoint.is_none() {
        return webview::start(webview::HostConfig {
            user_data_folder: data_path(WEBVIEW_FOLDER),
            window_title: format!("{APP_NAME} - Anmeldung"),
        });
    }
    Arc::new(session::CdpTransport::new(endpoint.unwrap_or(session::DEFAULT_ENDPOINT)))
}

fn main() {
    let arguments = parse_arguments();
    #[cfg(windows)]
    updater::clean_up_old_binary();
    let mode = Mode {
        demo: arguments.demo,
        read_only: arguments.read_only,
    };
    let store = if arguments.demo {
        let store = Store::open_in_memory().expect("in-memory store");
        demo::seed(&store);
        store
    } else {
        let path = arguments.database.clone().unwrap_or_else(|| data_path(DATABASE_FILE));
        Store::open(&path).expect("cache database")
    };
    let store = Arc::new(store);
    let receiver = (!arguments.demo)
        .then(|| backend::start(store.clone(), transport(arguments.endpoint.as_deref())));

    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::load_fonts(cx);
            theme::apply(cx);
            bind_keys(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let state = cx.new(|_| {
                let mut state = AppState::new(store, mode);
                state.start_on_channels = arguments.channels_tab;
                state
            });
            if arguments.demo {
                state.update(cx, |state, cx| {
                    demo::seed_directory(state);
                    state.select(demo::first_selection(), cx);
                });
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
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| AppShell::new(state, open_target, startup, window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
