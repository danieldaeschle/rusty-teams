use std::sync::{Arc, mpsc};

use crate::transport::UiRequest;

#[derive(Debug, Clone)]
pub struct EmbedSpec {
    pub parent: isize,
    pub url: String,
    pub background: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedEvent {
    Navigated(String),
    NewWindow(String),
    Closed,
}

#[derive(Debug)]
pub(crate) enum EmbedCommand {
    Bounds(EmbedBounds),
    Visible(bool),
    Navigate(String),
    Close,
}

#[derive(Clone)]
pub struct EmbedHandle {
    pub(crate) id: u64,
    pub(crate) requests: mpsc::Sender<UiRequest>,
    pub(crate) waker: Arc<dyn Fn() + Send + Sync>,
}

impl EmbedHandle {
    pub fn set_bounds(&self, bounds: EmbedBounds) {
        self.send(EmbedCommand::Bounds(bounds));
    }

    pub fn set_visible(&self, visible: bool) {
        self.send(EmbedCommand::Visible(visible));
    }

    pub fn navigate(&self, url: String) {
        self.send(EmbedCommand::Navigate(url));
    }

    pub fn close(&self) {
        self.send(EmbedCommand::Close);
    }

    fn send(&self, command: EmbedCommand) {
        let request = UiRequest::Embed {
            id: self.id,
            command,
        };
        if self.requests.send(request).is_ok() {
            (self.waker)();
        }
    }
}
