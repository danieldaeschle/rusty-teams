#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::*;

#[cfg(target_os = "linux")]
mod linux_notifications;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod linux_sound;
#[cfg(target_os = "linux")]
mod linux_tray;
#[cfg(not(windows))]
mod stub;
#[cfg(target_os = "linux")]
mod x11_input;
#[cfg(target_os = "linux")]
pub use linux_notifications::{
    BusNotification, DesktopNotifications, NotificationEvent, NotificationImage,
    notifications_inhibited,
};
#[cfg(target_os = "linux")]
pub use linux_tray::Tray;
#[cfg(not(windows))]
pub use stub::*;

pub type NativeHandle = isize;

#[cfg_attr(not(windows), allow(dead_code))]
mod stack_plan;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShareGeometry {
    pub target: ScreenRect,
    pub work: WorkArea,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Toggle,
    Open,
    SetStatus(chatsvc::ForcedKind),
    SetStatusDuration(crate::status_choice::StatusDuration),
    ResetStatus,
    Notifications,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayPresence {
    pub title: String,
    pub forced: Option<chatsvc::ForcedKind>,
    pub mention: bool,
}

impl TrayPresence {
    pub fn new(
        title: impl Into<String>,
        forced: Option<chatsvc::ForcedKind>,
        mention: bool,
    ) -> Self {
        Self {
            title: title.into(),
            forced,
            mention,
        }
    }
}
