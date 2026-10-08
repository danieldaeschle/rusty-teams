use std::sync::{Arc, mpsc};

use crate::transport::UiRequest;

#[derive(Debug, Clone)]
pub struct DialogSpec {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub background: u32,
    pub owner: Option<isize>,
    pub page_url: String,
    pub page_html: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogEvent {
    Message(String),
    Closed,
}

#[derive(Debug)]
pub(crate) enum DialogCommand {
    Post(String),
    Resize {
        width: Option<u32>,
        height: Option<u32>,
    },
    Close,
}

#[derive(Clone)]
pub struct DialogHandle {
    pub(crate) id: u64,
    pub(crate) requests: mpsc::Sender<UiRequest>,
    pub(crate) waker: Arc<dyn Fn() + Send + Sync>,
}

impl DialogHandle {
    pub fn post(&self, json: String) {
        self.send(DialogCommand::Post(json));
    }

    pub fn resize(&self, width: Option<u32>, height: Option<u32>) {
        self.send(DialogCommand::Resize { width, height });
    }

    pub fn close(&self) {
        self.send(DialogCommand::Close);
    }

    fn send(&self, command: DialogCommand) {
        let request = UiRequest::Dialog {
            id: self.id,
            command,
        };
        if self.requests.send(request).is_ok() {
            (self.waker)();
        }
    }
}
