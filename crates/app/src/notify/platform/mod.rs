#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::*;

#[cfg(not(windows))]
mod stub;
#[cfg(not(windows))]
pub use stub::*;

pub type NativeHandle = isize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Toggle,
    Open,
    ToggleSound,
    ToggleDoNotDisturb,
    Settings,
    Quit,
}
