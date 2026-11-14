//! Linux desktop integration on start: offer to install the binary into
//! `~/.local/share/rusty-teams` with a menu entry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use app_base::linux_desktop::{self, Layout};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use crate::app_state::AppHandle;
use crate::i18n::{tr, tr_args};
use crate::theme;

const DIALOG_WIDTH: f32 = 460.;

pub fn start(cx: &mut App) {
    let (Ok(executable), Some(layout)) = (std::env::current_exe(), Layout::current()) else {
        return;
    };
    if linux_desktop::is_installed(&executable, &layout) {
        // Keeps the menu entry pointing at this file, e.g. after it was deleted by hand.
        if !layout.desktop_file().is_file()
            && linux_desktop::write_desktop_entry(&layout, &executable).is_ok()
        {
            linux_desktop::refresh_desktop_caches(&layout);
        }
        return;
    }
    let dont_ask = cx
        .try_global::<AppHandle>()
        .is_some_and(|handle| linux_desktop::dont_ask_again(&handle.0.read(cx).store));
    if linux_desktop::should_offer_install(&executable, &layout, dont_ask) {
        // After the shell's first frame, so the dialog sits on top of the splash.
        cx.defer(move |cx| offer_install(executable, layout, cx));
    }
}

fn offer_install(executable: PathBuf, layout: Layout, cx: &mut App) {
    let Some(window) = crate::main_window::current(cx) else {
        return;
    };
    let folder = layout.install_dir();
    let mut args = HashMap::new();
    args.insert("folder".to_owned(), display_home(&folder));
    let body = tr_args("install.body", &args);
    window
        .update(cx, move |_, window, cx| {
            window.open_dialog(cx, move |dialog: Dialog, _, _| {
                let (install_from, install_layout) = (executable.clone(), layout.clone());
                let footer = DialogFooter::new()
                    .child(
                        Button::new("install-dont-ask")
                            .label(tr("install.dont_ask"))
                            .on_click(|_, window, cx| {
                                if let Some(handle) = cx.try_global::<AppHandle>() {
                                    linux_desktop::set_dont_ask_again(&handle.0.read(cx).store);
                                }
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("install-not-now")
                            .label(tr("install.not_now"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("install-move")
                            .primary()
                            .label(tr("install.move"))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                install(&install_from, &install_layout, cx);
                            }),
                    );
                dialog
                    .title(tr("install.title"))
                    .w(px(DIALOG_WIDTH))
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .text_color(theme::text())
                            .child(body.clone()),
                    )
                    .footer(footer)
            });
        })
        .ok();
}

fn install(executable: &Path, layout: &Layout, cx: &mut App) {
    match linux_desktop::install_binary(executable, layout) {
        Ok(installed) => relaunch(&installed, cx),
        Err(error) => {
            log::warn!("install failed: {error}");
            notice(failure("install.failed", &error.to_string()), cx);
        }
    }
}

fn failure(key: &str, detail: &str) -> String {
    let mut args = HashMap::new();
    args.insert("detail".to_owned(), detail.to_owned());
    tr_args(key, &args)
}

fn notice(text: String, cx: &mut App) {
    if let Some(handle) = cx.try_global::<AppHandle>() {
        handle
            .0
            .clone()
            .update(cx, |state, cx| state.raise_notice(text, None, cx));
    }
}

/// Where this process stands relative to the install folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installation {
    /// This process is the installed copy.
    Running,
    /// Another binary is installed; this one runs from elsewhere.
    OtherCopy,
    NotInstalled,
}

pub fn installation() -> Installation {
    let (Ok(executable), Some(layout)) = (std::env::current_exe(), Layout::current()) else {
        return Installation::NotInstalled;
    };
    if linux_desktop::is_installed(&executable, &layout) {
        Installation::Running
    } else if layout.installed_binary().is_file() {
        Installation::OtherCopy
    } else {
        Installation::NotInstalled
    }
}

pub fn install_folder() -> String {
    Layout::current()
        .map(|layout| display_home(&layout.install_dir()))
        .unwrap_or_default()
}

/// Settings > General: installs (or replaces the installed copy with) this binary.
pub fn install_now(cx: &mut App) {
    let (Ok(executable), Some(layout)) = (std::env::current_exe(), Layout::current()) else {
        return;
    };
    if let Some(handle) = cx.try_global::<AppHandle>() {
        linux_desktop::ask_again(&handle.0.read(cx).store);
    }
    install(&executable, &layout, cx);
}

/// Settings > General: removes the menu entry and the installed binary.
pub fn remove(cx: &mut App) {
    let Some(layout) = Layout::current() else {
        return;
    };
    match linux_desktop::uninstall(&layout) {
        Ok(()) => notice(tr("install.removed"), cx),
        Err(error) => notice(failure("install.remove_failed", &error.to_string()), cx),
    }
    cx.refresh_windows();
}

pub fn asks_on_start(cx: &App) -> bool {
    cx.try_global::<AppHandle>()
        .is_some_and(|handle| !linux_desktop::dont_ask_again(&handle.0.read(cx).store))
}

pub fn set_asks_on_start(ask: bool, cx: &mut App) {
    if let Some(handle) = cx.try_global::<AppHandle>() {
        let store = &handle.0.read(cx).store;
        if ask {
            linux_desktop::ask_again(store);
        } else {
            linux_desktop::set_dont_ask_again(store);
        }
    }
    cx.refresh_windows();
}

/// The release this build counts as: its tag in CI release builds, the workspace
/// version (kept equal to the latest release) in a local build.
pub fn running_release() -> &'static str {
    app_base::version::release_tag().unwrap_or(concat!("v", env!("CARGO_PKG_VERSION")))
}

/// The release to update from, when this process is the installed copy.
pub fn updatable_release() -> Option<&'static str> {
    crate::updater::self_update_allowed().then(running_release)
}

/// Starts the installed copy and quits; the new process waits for this one to let go
/// of the single-instance socket.
fn relaunch(installed: &Path, cx: &mut App) {
    let started = std::process::Command::new(installed)
        .env(crate::updater::RELAUNCH_ENV, std::process::id().to_string())
        .spawn();
    match started {
        Ok(_) => cx.quit(),
        Err(error) => log::warn!("could not start {}: {error}", installed.display()),
    }
}

fn display_home(path: &Path) -> String {
    match directories::BaseDirs::new().and_then(|base| {
        path.strip_prefix(base.home_dir())
            .ok()
            .map(Path::to_path_buf)
    }) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}
