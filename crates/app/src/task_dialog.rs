use std::sync::Arc;

use gpui_kit::*;

use crate::backend::Engine;
use crate::card_state::CardScope;

#[cfg(windows)]
mod hosted;
#[cfg_attr(not(windows), allow(dead_code))]
mod sdk;

#[cfg(windows)]
pub use hosted::{host, install_host};

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub struct UrlDialog {
    pub title: String,
    pub url: String,
    pub fallback_url: String,
    pub width: u32,
    pub height: u32,
    pub app_id: String,
    pub bot_id: String,
    pub scope: CardScope,
    pub web_application_resource: Option<String>,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlDialogNext {
    Close,
    Navigate {
        url: String,
        width: u32,
        height: u32,
    },
}

#[cfg(windows)]
pub fn open_url_dialog(dialog: UrlDialog, engine: Option<Arc<Engine>>, cx: &mut App) {
    hosted::open(dialog, engine, cx);
}

#[cfg(not(windows))]
pub fn open_url_dialog(dialog: UrlDialog, _engine: Option<Arc<Engine>>, cx: &mut App) {
    cx.open_url(&dialog.fallback_url);
}
