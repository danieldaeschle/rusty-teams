#![cfg(windows)]

mod dialog;
mod embed;
mod fanout;
mod host;
mod host_dialog;
mod host_embed;
mod transport;

pub use dialog::{DialogEvent, DialogHandle, DialogSpec};
pub use embed::{EmbedBounds, EmbedEvent, EmbedHandle, EmbedSpec};
pub use host::{HostConfig, start};
pub use transport::{HostState, WebViewTransport};

pub(crate) const FORWARDED_EVENTS: [&str; 2] = ["Runtime.bindingCalled", "Page.frameNavigated"];

// A hidden window counts as occluded, so Chromium would otherwise throttle the Trouter keep-alive timers.
pub(crate) const BROWSER_ARGUMENTS: &str = concat!(
    "--disable-features=Translate,MediaRouter,OptimizationHints ",
    "--disable-background-networking --disable-component-update --disable-default-apps ",
    "--disable-breakpad --disable-client-side-phishing-detection --disable-gpu ",
    "--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows",
);
