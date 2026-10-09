#![allow(dead_code)]

use gpui_kit::*;
use tokio::sync::mpsc::UnboundedReceiver;

pub enum NativeEvent {
    Navigated(String),
    NewWindow(String),
    Loaded,
    Closed,
}

pub fn available() -> bool {
    false
}

#[derive(Clone)]
pub struct Native;

impl Native {
    pub fn open(_url: &str, _window: &Window) -> Option<(Native, UnboundedReceiver<NativeEvent>)> {
        None
    }

    pub fn place(&self, _bounds: Bounds<Pixels>, _scale: f32, _visible: bool) {}

    pub fn close(&self) {}
}
