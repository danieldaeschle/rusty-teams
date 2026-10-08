#[cfg_attr(not(windows), allow(dead_code))]
mod badge;
mod center;
mod incoming;
mod layout;
#[cfg_attr(not(windows), allow(dead_code))]
mod platform;
mod rules;
mod settings;
mod settings_view;
mod stack;
mod text;
mod toast;

pub use center::NotificationCenter;
#[cfg(windows)]
pub use platform::native_handle;
