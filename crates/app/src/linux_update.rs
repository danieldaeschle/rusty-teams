//! A newer release is offered in a dialog. Accepting opens the release page.
//! Nothing is downloaded and the running binary is left in place.
//! Flatpak never checks: the package owns updates.

use std::collections::HashMap;
use std::time::Duration;

use app_base::release_feed::{self, AvailableUpdate};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use crate::app_state::AppHandle;
use crate::i18n::{tr, tr_args};
use crate::theme;

const ENABLED_META_KEY: &str = "updates.enabled";
/// How often an offer waits for a main window closed into the tray to come back.
const WINDOW_WAIT: Duration = Duration::from_secs(30);
const DIALOG_WIDTH: f32 = 460.;

struct ReleaseNotice {
    offer: Option<AvailableUpdate>,
    dialog_window: Option<WindowId>,
    /// "Not now" holds for this run only; the next start asks again.
    declined: Option<String>,
}

impl Global for ReleaseNotice {}

fn offer(cx: &App) -> Option<AvailableUpdate> {
    cx.try_global::<ReleaseNotice>()
        .and_then(|notice| notice.offer.clone())
}

fn set_offer(offer: Option<AvailableUpdate>, cx: &mut App) {
    if cx.has_global::<ReleaseNotice>() {
        cx.global_mut::<ReleaseNotice>().offer = offer;
    }
    cx.refresh_windows();
}

fn with_store<R>(cx: &App, read: impl FnOnce(&store::Store) -> R) -> Option<R> {
    cx.try_global::<AppHandle>()
        .map(|handle| read(&handle.0.read(cx).store))
}

/// The periodic check; on by default. One key on every OS.
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

/// Flatpak updates through the package. Other builds may look at releases.
pub fn checks_releases() -> bool {
    #[cfg(target_os = "linux")]
    {
        let flatpak = std::env::var_os("FLATPAK_ID");
        if flatpak.is_some_and(|id| !id.is_empty()) {
            return false;
        }
    }
    true
}

fn declined(cx: &App) -> Option<String> {
    cx.try_global::<ReleaseNotice>()
        .and_then(|notice| notice.declined.clone())
}

fn decline(tag: &str, cx: &mut App) {
    if cx.has_global::<ReleaseNotice>() {
        cx.global_mut::<ReleaseNotice>().declined = Some(tag.to_owned());
    }
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
    if !cx.has_global::<ReleaseNotice>() {
        cx.set_global(ReleaseNotice {
            offer: None,
            dialog_window: None,
            declined: None,
        });
    }
}

/// Starts the periodic check. Release-tagged builds only. Flatpak never starts it.
pub fn start(cx: &mut App) {
    if !checks_releases() {
        return;
    }
    if app_base::version::release_tag().is_none() {
        return;
    }
    ensure_state(cx);
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(release_feed::FIRST_CHECK_DELAY)
            .await;
        loop {
            let waiting = cx.update(|cx| offer(cx).is_some());
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
    if !checks_releases() {
        return;
    }
    let Some(running_tag) = app_base::version::release_tag() else {
        if manual {
            notice(tr("install.updates_dev_build"), cx);
        }
        return;
    };
    ensure_state(cx);
    if offer(cx).is_some() {
        open_dialog(cx);
        return;
    }
    if manual {
        notice(tr("install.update_checking"), cx);
    }
    let find = release_feed::find_update(running_tag, cx);
    cx.spawn(async move |cx| {
        let found = find.await;
        cx.update(|cx| match found {
            Ok(Some(update)) if manual || declined(cx).as_deref() != Some(&update.tag) => {
                show(update, cx)
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

fn show(update: AvailableUpdate, cx: &mut App) {
    set_offer(Some(update), cx);
    show_when_window_is_open(cx);
}

/// Opens the dialog, waiting for the main window if it is closed into the tray.
fn show_when_window_is_open(cx: &mut App) {
    cx.spawn(async move |cx| {
        while cx.update(|cx| crate::main_window::current(cx).is_none()) {
            cx.background_executor().timer(WINDOW_WAIT).await;
        }
        cx.update(|cx| {
            if offer(cx).is_some() {
                open_dialog(cx);
            }
        });
    })
    .detach();
}

fn open_release(update: &AvailableUpdate, cx: &mut App) {
    let url = release_feed::release_page_url(&update.tag);
    decline(&update.tag, cx);
    set_offer(None, cx);
    cx.open_url(&url);
}

fn open_dialog(cx: &mut App) {
    let Some(window) = crate::main_window::current(cx) else {
        return;
    };
    if !cx.has_global::<ReleaseNotice>()
        || cx.global::<ReleaseNotice>().dialog_window == Some(window.window_id())
    {
        return;
    }
    cx.global_mut::<ReleaseNotice>().dialog_window = Some(window.window_id());
    let opened = window.update(cx, |_, window, cx| {
        window.open_dialog(cx, |dialog: Dialog, _, cx| build_dialog(dialog, cx));
    });
    if opened.is_err() {
        cx.global_mut::<ReleaseNotice>().dialog_window = None;
    }
}

fn close_dialog(window: &mut Window, cx: &mut App) {
    if cx.has_global::<ReleaseNotice>() {
        cx.global_mut::<ReleaseNotice>().dialog_window = None;
    }
    window.close_dialog(cx);
}

fn build_dialog(dialog: Dialog, cx: &App) -> Dialog {
    let Some(update) = offer(cx) else {
        return dialog;
    };
    let running = app_base::version::release_tag().unwrap_or_default();
    let mut text_args = args("version", update.tag.clone());
    text_args.insert("current".to_owned(), running.to_owned());
    let tag = update.tag.clone();
    let open = update.clone();
    dialog
        .title(tr("install.update_available_title"))
        .w(px(DIALOG_WIDTH))
        .keyboard(false)
        .overlay_closable(false)
        .close_button(false)
        .child(
            v_flex()
                .gap(px(8.))
                .text_color(theme::text())
                .child(tr_args("install.update_available_body", &text_args)),
        )
        .footer(
            DialogFooter::new()
                .child(
                    Button::new("update-not-now")
                        .label(tr("install.not_now"))
                        .on_click(move |_, window, cx| {
                            decline(&tag, cx);
                            set_offer(None, cx);
                            close_dialog(window, cx);
                        }),
                )
                .child(
                    Button::new("update-open")
                        .primary()
                        .label(tr("install.open_release"))
                        .on_click(move |_, window, cx| {
                            open_release(&open, cx);
                            close_dialog(window, cx);
                        }),
                ),
        )
}
