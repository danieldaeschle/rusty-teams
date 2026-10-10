#[cfg_attr(not(windows), allow(dead_code))]
mod badge;
mod center;
mod incoming;
mod layout;
#[cfg_attr(not(windows), allow(dead_code))]
mod platform;
#[cfg_attr(not(windows), allow(dead_code))]
mod ring_tone;
mod ring_view;
mod rules;
mod settings;
mod settings_view;
mod stack;
mod text;
mod toast;

pub use center::{NotificationCenter, selection_for};
pub use incoming::{IncomingTracker, preview_of};
pub use rules::{Incoming, Preview, channel_alerts};
#[cfg(test)]
pub use rules::ChatKind;
#[cfg(windows)]
pub use platform::native_handle;
