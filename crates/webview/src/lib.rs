#[cfg(windows)]
mod dialog;
#[cfg(windows)]
mod embed;
#[cfg(windows)]
mod fanout;
mod geometry;
#[cfg(windows)]
mod host;
#[cfg(windows)]
mod host_dialog;
#[cfg(windows)]
mod main_embed;
#[cfg(windows)]
mod transport;

#[cfg(windows)]
pub use dialog::{DialogEvent, DialogHandle, DialogSpec};
#[cfg(windows)]
pub use embed::{EmbedEvent, EmbedSpec};
pub use geometry::{EmbedBounds, local_cutouts};
#[cfg(windows)]
pub use host::{HostConfig, start};
#[cfg(windows)]
pub use main_embed::{EmbedHost, MainEmbed};
#[cfg(windows)]
pub use transport::{HostState, WebViewTransport};

#[cfg(windows)]
pub(crate) const FORWARDED_EVENTS: [&str; 2] = ["Runtime.bindingCalled", "Page.frameNavigated"];

#[cfg(windows)]
// A hidden window counts as occluded, so Chromium would otherwise throttle the Trouter keep-alive timers.
pub(crate) const BROWSER_ARGUMENTS: &str = concat!(
    "--disable-features=Translate,MediaRouter,OptimizationHints ",
    "--disable-background-networking --disable-component-update --disable-default-apps ",
    "--disable-breakpad --disable-client-side-phishing-detection --disable-gpu ",
    "--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows",
);
