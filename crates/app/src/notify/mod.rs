mod agent_toast;
mod center;
mod incoming;
mod layout;
mod meeting_view;
mod ring_view;
mod rules;
mod settings;
mod settings_mcp;
mod settings_view;
mod stack;
mod text;
mod toast;

pub use app_base::notify::{badge, platform, ring_tone};
pub use center::{NotificationCenter, selection_for};
pub use incoming::{IncomingTracker, preview_of};
#[cfg(windows)]
pub use platform::native_handle;
#[cfg(test)]
pub use rules::ChatKind;
pub use rules::{Incoming, Preview, channel_alerts};
pub(crate) use settings_view::switch;
