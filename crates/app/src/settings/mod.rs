mod about;
mod appearance;
mod calls;
mod chats;
mod devices;
mod dialog;
mod general;
mod mcp;
mod nav;
mod notifications;
mod rows;

#[cfg(test)]
pub use dialog::metrics;
pub use dialog::{SettingsDialog, SettingsDialogEvent};
#[cfg(test)]
pub use nav::{item_selector, label, label_selector};
pub(crate) use rows::{select_selector, switch};
