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

pub use dialog::{SettingsDialog, SettingsDialogEvent};
#[cfg(test)]
pub use dialog::metrics;
pub(crate) use rows::switch;
