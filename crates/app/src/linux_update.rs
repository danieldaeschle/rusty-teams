//! Linux self-update, always with consent: a newer GitHub release is offered in a
//! dialog; "Update" downloads it with progress and "Restart now" swaps the binary
//! and relaunches. Nothing restarts without a click. Settings > General turns the
//! periodic check off; "Check now" works either way.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use app_base::release_feed::{self, AvailableUpdate};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use crate::app_state::AppHandle;
use crate::i18n::{tr, tr_args};
use crate::theme;
use crate::updater;

const ENABLED_META_KEY: &str = "linux.updates.enabled";
/// How often an offer waits for a main window closed into the tray to come back.
const WINDOW_WAIT: Duration = Duration::from_secs(30);
/// How often the progress dialog redraws while downloading.
const PROGRESS_REDRAW: Duration = Duration::from_millis(250);
const DIALOG_WIDTH: f32 = 460.;

#[derive(Clone)]
enum Stage {
    Idle,
    Offer(AvailableUpdate),
    Downloading {
        update: AvailableUpdate,
        progress: Arc<AtomicU64>,
    },
    Ready(String),
    Failed(String),
}

struct LinuxUpdates {
    stage: Stage,
    dialog_open: bool,
    /// "Not now" holds for this run only; the next start asks again.
    declined: Option<String>,
}

impl Global for LinuxUpdates {}

fn stage(cx: &App) -> Stage {
    cx.try_global::<LinuxUpdates>()
        .map(|updates| updates.stage.clone())
        .unwrap_or(Stage::Idle)
}

fn set_stage(stage: Stage, cx: &mut App) {
    if cx.has_global::<LinuxUpdates>() {
        cx.global_mut::<LinuxUpdates>().stage = stage;
    }
    cx.refresh_windows();
}

fn with_store<R>(cx: &App, read: impl FnOnce(&store::Store) -> R) -> Option<R> {
    cx.try_global::<AppHandle>()
        .map(|handle| read(&handle.0.read(cx).store))
}

/// The periodic check; on by default.
pub fn enabled(cx: &App) -> bool {
    with_store(cx, |store| {
        store.meta(ENABLED_META_KEY).ok().flatten().as_deref() != Some("0")
    })
    .unwrap_or(true)
}

pub fn set_enabled(enabled: bool, cx: &mut App) {
    with_store(cx, |store| {
        let _ = store.set_meta(ENABLED_META_KEY, if enabled { "1" } else { "0" });
    });
    cx.refresh_windows();
}

fn declined(cx: &App) -> Option<String> {
    cx.try_global::<LinuxUpdates>()
        .and_then(|updates| updates.declined.clone())
}

fn decline(tag: &str, cx: &mut App) {
    if cx.has_global::<LinuxUpdates>() {
        cx.global_mut::<LinuxUpdates>().declined = Some(tag.to_owned());
    }
}

/// A newer release a previous run downloaded but did not install ("Later").
fn staged_update(running_tag: &str) -> Option<String> {
    let paths = updater::current_paths()?;
    if !updater::check(&paths) {
        return None;
    }
    let tag = std::fs::read_to_string(&paths.version_file).ok()?;
    let tag = tag.trim();
    if release_feed::is_newer(tag, running_tag) {
        return Some(tag.to_owned());
    }
    // Same or older than this build, e.g. after installing a release by hand.
    let _ = std::fs::remove_file(&paths.version_file);
    let _ = std::fs::remove_file(&paths.update_exe);
    None
}

fn args(name: &str, value: impl Into<String>) -> HashMap<String, String> {
    HashMap::from([(name.to_owned(), value.into())])
}

fn notice(text: String, cx: &mut App) {
    if let Some(handle) = cx.try_global::<AppHandle>() {
        handle
            .0
            .clone()
            .update(cx, |state, cx| state.raise_notice(text, None, cx));
    }
}

fn ensure_state(cx: &mut App) {
    if !cx.has_global::<LinuxUpdates>() {
        cx.set_global(LinuxUpdates {
            stage: Stage::Idle,
            dialog_open: false,
            declined: None,
        });
    }
}

/// Starts the periodic check. Only the installed copy of a release build updates.
pub fn start(cx: &mut App) {
    if crate::linux_install::updatable_release().is_none() {
        return;
    }
    ensure_state(cx);
    if let Some(running_tag) = crate::linux_install::updatable_release()
        && let Some(tag) = staged_update(running_tag)
    {
        set_stage(Stage::Ready(tag), cx);
        show_when_window_is_open(cx);
    }
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(release_feed::FIRST_CHECK_DELAY)
            .await;
        loop {
            let waiting = cx.update(|cx| matches!(stage(cx), Stage::Ready(_)));
            if !waiting && cx.update(|cx| enabled(cx)) {
                cx.update(|cx| check(false, cx));
            }
            cx.background_executor()
                .timer(release_feed::CHECK_INTERVAL)
                .await;
        }
    })
    .detach();
}

/// Looks for a newer release. `manual` (Settings > Check now) also offers a version
/// declined with "Not now" in this run and reports "up to date" and errors.
pub fn check(manual: bool, cx: &mut App) {
    let Some(running_tag) = crate::linux_install::updatable_release() else {
        if manual {
            notice(tr("install.updates_need_install"), cx);
        }
        return;
    };
    ensure_state(cx);
    match stage(cx) {
        Stage::Downloading { .. } | Stage::Ready(_) => {
            open_dialog(cx);
            return;
        }
        Stage::Idle | Stage::Offer(_) | Stage::Failed(_) => {}
    }
    if manual {
        notice(tr("install.update_checking"), cx);
    }
    let find = release_feed::find_update(running_tag, cx);
    cx.spawn(async move |cx| {
        let found = find.await;
        cx.update(|cx| match found {
            Ok(Some(update)) if manual || declined(cx).as_deref() != Some(&update.tag) => {
                offer(update, cx)
            }
            Ok(Some(_)) => {}
            Ok(None) if manual => notice(tr("install.up_to_date"), cx),
            Ok(None) => {}
            Err(error) if manual => {
                notice(tr_args("install.update_failed", &args("detail", error)), cx)
            }
            Err(error) => log::warn!("update check failed: {error}"),
        });
    })
    .detach();
}

fn offer(update: AvailableUpdate, cx: &mut App) {
    set_stage(Stage::Offer(update), cx);
    show_when_window_is_open(cx);
}

/// Opens the dialog for the current stage, waiting for the main window if it is
/// closed into the tray.
fn show_when_window_is_open(cx: &mut App) {
    cx.spawn(async move |cx| {
        while cx.update(|cx| crate::main_window::current(cx).is_none()) {
            cx.background_executor().timer(WINDOW_WAIT).await;
        }
        cx.update(|cx| {
            if matches!(stage(cx), Stage::Offer(_) | Stage::Ready(_)) {
                open_dialog(cx);
            }
        });
    })
    .detach();
}

fn download(update: AvailableUpdate, cx: &mut App) {
    let Some(paths) = updater::current_paths() else {
        return;
    };
    let progress = Arc::new(AtomicU64::new(0));
    set_stage(
        Stage::Downloading {
            update: update.clone(),
            progress: progress.clone(),
        },
        cx,
    );
    let tag = update.tag.clone();
    let task = release_feed::download_update(update, paths, progress, cx);
    cx.spawn(async move |cx| {
        let redraw = cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(PROGRESS_REDRAW).await;
                cx.update(|cx| cx.refresh_windows());
            }
        });
        let result = task.await;
        drop(redraw);
        cx.update(|cx| {
            match result {
                Ok(()) => set_stage(Stage::Ready(tag), cx),
                Err(error) => {
                    log::warn!("update download failed: {error}");
                    set_stage(Stage::Failed(error), cx);
                }
            }
            open_dialog(cx);
        });
    })
    .detach();
}

fn restart(cx: &mut App) {
    let Some(paths) = updater::current_paths() else {
        return;
    };
    match updater::install_and_relaunch(&paths) {
        Ok(()) => cx.quit(),
        Err(error) => set_stage(Stage::Failed(error), cx),
    }
}

/// Opens the update dialog on the main window unless it is already open. It redraws
/// from the current stage, so one dialog walks through offer, progress and restart.
fn open_dialog(cx: &mut App) {
    let Some(window) = crate::main_window::current(cx) else {
        return;
    };
    if !cx.has_global::<LinuxUpdates>() || cx.global::<LinuxUpdates>().dialog_open {
        return;
    }
    cx.global_mut::<LinuxUpdates>().dialog_open = true;
    let opened = window.update(cx, |_, window, cx| {
        window.open_dialog(cx, |dialog: Dialog, _, cx| build_dialog(dialog, cx));
    });
    if opened.is_err() {
        cx.global_mut::<LinuxUpdates>().dialog_open = false;
    }
}

fn close_dialog(window: &mut Window, cx: &mut App) {
    if cx.has_global::<LinuxUpdates>() {
        cx.global_mut::<LinuxUpdates>().dialog_open = false;
    }
    window.close_dialog(cx);
}

fn percent(progress: &AtomicU64, size: u64) -> u64 {
    (progress.load(Ordering::Relaxed) * 100)
        .checked_div(size)
        .unwrap_or(0)
        .min(100)
}

fn build_dialog(dialog: Dialog, cx: &App) -> Dialog {
    let running = app_base::version::release_tag().unwrap_or_default();
    let (title, body, footer) = match stage(cx) {
        Stage::Offer(update) => {
            let mut text_args = args("version", update.tag.clone());
            text_args.insert("current".to_owned(), running.to_owned());
            let tag = update.tag.clone();
            (
                tr("install.update_available_title"),
                tr_args("install.update_available_body", &text_args),
                DialogFooter::new()
                    .child(
                        Button::new("update-not-now")
                            .label(tr("install.not_now"))
                            .on_click(move |_, window, cx| {
                                decline(&tag, cx);
                                set_stage(Stage::Idle, cx);
                                close_dialog(window, cx);
                            }),
                    )
                    .child(
                        Button::new("update-start")
                            .primary()
                            .label(tr("install.update_now"))
                            .on_click(move |_, _, cx| download(update.clone(), cx)),
                    ),
            )
        }
        Stage::Downloading { update, progress } => {
            let mut text_args = args("version", update.tag.clone());
            text_args.insert(
                "percent".to_owned(),
                percent(&progress, update.asset.size).to_string(),
            );
            (
                tr("install.update_available_title"),
                tr_args("install.update_downloading", &text_args),
                DialogFooter::new().child(
                    Button::new("update-hide")
                        .label(tr("install.hide"))
                        .on_click(|_, window, cx| close_dialog(window, cx)),
                ),
            )
        }
        Stage::Ready(tag) => (
            tr("install.update_ready_title"),
            tr_args("install.update_ready_body", &args("version", tag)),
            DialogFooter::new()
                .child(
                    Button::new("update-later")
                        .label(tr("install.later"))
                        .on_click(|_, window, cx| close_dialog(window, cx)),
                )
                .child(
                    Button::new("update-restart")
                        .primary()
                        .label(tr("install.restart_now"))
                        .on_click(|_, _, cx| restart(cx)),
                ),
        ),
        Stage::Failed(error) => (
            tr("install.update_available_title"),
            tr_args("install.update_failed", &args("detail", error)),
            DialogFooter::new().child(
                Button::new("update-close")
                    .label(tr("common.close"))
                    .on_click(|_, window, cx| {
                        set_stage(Stage::Idle, cx);
                        close_dialog(window, cx);
                    }),
            ),
        ),
        Stage::Idle => (String::new(), String::new(), DialogFooter::new()),
    };
    // Only the buttons close it, so the stage and the open flag stay in step.
    dialog
        .title(title)
        .w(px(DIALOG_WIDTH))
        .keyboard(false)
        .overlay_closable(false)
        .close_button(false)
        .child(v_flex().gap(px(8.)).text_color(theme::text()).child(body))
        .footer(footer)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::percent;

    #[test]
    fn percent_is_clamped_and_safe_without_a_size() {
        let progress = AtomicU64::new(50);
        assert_eq!(percent(&progress, 200), 25);
        assert_eq!(percent(&progress, 0), 0);
        progress.store(500, Ordering::Relaxed);
        assert_eq!(percent(&progress, 200), 100);
    }
}
