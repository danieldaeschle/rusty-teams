pub mod desktop_payload;
pub mod incoming;
pub mod layout;
pub mod pause;
pub mod rules;
pub mod settings;
pub mod stack;
pub mod taskbar;
pub mod text;
pub mod urgent_repeat;

pub use desktop_payload::{
    DesktopAction, DesktopKey, DesktopPayload, MeetingLabels, MessageLabels, RingLabels, Urgency,
    action, meeting_payload, message_payload, ring_payload,
};

pub use app_base::notify::{badge, platform, sound_cache, sounds};
pub use chatsvc::sound_settings;
pub use incoming::{IncomingTracker, LateArrivals, preview_of};
#[cfg(windows)]
pub use platform::native_handle;
pub use rules::{ChatKind, Incoming, Preview, alerts};
pub use taskbar::taskbar_badge_count;
