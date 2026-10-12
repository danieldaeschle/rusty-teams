use gpui_kit::Window;

use calling::ShareKind;

use super::{NativeHandle, ScreenRect, ShareGeometry, TrayCommand, WorkArea};
use crate::notify::badge::Badge;
use crate::notify::ring_tone::Ringtone;

pub fn native_handle(_window: &Window) -> Option<NativeHandle> {
    None
}

pub fn prepare_toast_window(_handle: NativeHandle) {}

pub fn set_activatable(_handle: NativeHandle, _activatable: bool) {}

pub fn work_area(_handle: NativeHandle) -> Option<WorkArea> {
    None
}

pub fn work_area_at(_x: i32, _y: i32) -> Option<WorkArea> {
    None
}

pub fn place(_handle: NativeHandle, _x: i32, _y: i32, _width: i32, _height: i32) {}

pub fn exclude_from_capture(_handle: NativeHandle) {}

pub fn make_click_through(_handle: NativeHandle) {}

pub fn window_minimized(_handle: NativeHandle) -> bool {
    false
}

pub fn window_rect(_handle: NativeHandle) -> Option<ScreenRect> {
    None
}

pub fn share_geometry(_kind: ShareKind, _id: u64) -> Option<ShareGeometry> {
    None
}

pub fn system_quiet() -> bool {
    false
}

pub fn restore_and_raise(_handle: NativeHandle) {}

pub fn hide_window(_handle: NativeHandle) {}

pub fn flash(_handle: NativeHandle) {}

pub fn stop_flash(_handle: NativeHandle) {}

pub fn set_badge(_handle: NativeHandle, _badge: Option<&Badge>) {}

pub fn play_sound() {}

pub struct RingTone;

pub fn start_ring(_ringtone: Ringtone) -> Option<RingTone> {
    None
}

pub fn start_ringback() -> Option<RingTone> {
    None
}

pub fn play_preview(_ringtone: Ringtone) {}

pub struct Tray;

impl Tray {
    pub fn new(_sound_on: bool, _do_not_disturb: bool) -> Option<Self> {
        None
    }

    pub fn poll(&self) -> Vec<TrayCommand> {
        Vec::new()
    }

    pub fn sync(&self, _sound_on: bool, _do_not_disturb: bool, _mention: bool) {}
}

pub fn animations_enabled() -> bool {
    true
}
