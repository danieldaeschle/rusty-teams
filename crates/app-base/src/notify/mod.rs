#[cfg_attr(not(windows), allow(dead_code))]
pub mod badge;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod platform;
pub mod sound_cache;
pub mod tray_status;

pub use chatsvc::sounds;
