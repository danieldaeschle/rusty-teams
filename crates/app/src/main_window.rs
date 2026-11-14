//! Owns the main window so it can be closed into the tray and opened again.
//!
//! GPUI cannot hide a window on Wayland, so on Linux "close to tray" really closes
//! the main window while the app (tray, notifications, calls) keeps running; the tray
//! or a second start opens a fresh one. Windows hides the window natively instead.

use std::rc::Rc;

use gpui_kit::*;

type Opener = Rc<dyn Fn(&mut App) -> Option<AnyWindowHandle>>;

pub struct MainWindow {
    handle: Option<AnyWindowHandle>,
    open: Opener,
    hiding: bool,
}

impl Global for MainWindow {}

/// Opens the first main window with `open` and remembers how to open the next one.
pub fn install(
    open: impl Fn(&mut App) -> Option<AnyWindowHandle> + 'static,
    cx: &mut App,
) -> Option<AnyWindowHandle> {
    let open: Opener = Rc::new(open);
    cx.set_global(MainWindow {
        handle: None,
        open: open.clone(),
        hiding: false,
    });
    let handle = open(cx);
    cx.global_mut::<MainWindow>().handle = handle;
    handle
}

fn main_mut(cx: &mut App) -> Option<&mut MainWindow> {
    cx.has_global::<MainWindow>()
        .then(|| cx.global_mut::<MainWindow>())
}

pub fn current(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<MainWindow>().and_then(|main| main.handle)
}

/// The next close of the main window goes to the tray instead of quitting.
pub fn mark_hiding(cx: &mut App) {
    if let Some(main) = main_mut(cx) {
        main.hiding = true;
    }
}

/// Closes the main window into the tray. Deferred: the window's views may be on the stack.
pub fn hide(cx: &mut App) {
    let Some(handle) = current(cx) else {
        return;
    };
    mark_hiding(cx);
    cx.defer(move |cx| {
        if handle
            .update(cx, |_, window, _| window.remove_window())
            .is_err()
        {
            note_closed(handle.window_id(), cx);
        }
    });
}

/// Brings the main window to the front, opening a new one if it was closed into the tray.
/// Deferred: building the shell updates entities that may be on the stack.
pub fn show(cx: &mut App) {
    cx.defer(|cx| {
        if let Some(handle) = current(cx)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let Some(open) = cx.try_global::<MainWindow>().map(|main| main.open.clone()) else {
            return;
        };
        let handle = open(cx);
        if let Some(main) = main_mut(cx) {
            main.handle = handle;
            main.hiding = false;
        }
        if let Some(handle) = handle {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .ok();
        }
        cx.activate(true);
    });
}

/// Call from `on_window_closed`. Returns true when the app should quit: the main
/// window closed and it was not closed into the tray.
pub fn note_closed(window_id: WindowId, cx: &mut App) -> bool {
    let Some(main) = main_mut(cx) else {
        return false;
    };
    if main.handle.map(|handle| handle.window_id()) != Some(window_id) {
        return false;
    }
    main.handle = None;
    !std::mem::take(&mut main.hiding)
}

/// Whether the main window is currently closed into the tray.
pub fn is_hidden(cx: &App) -> bool {
    cx.try_global::<MainWindow>()
        .is_some_and(|main| main.handle.is_none())
}
