use gpui_kit::*;
use tokio::sync::mpsc::UnboundedReceiver;

pub fn available() -> bool {
    false
}

pub struct Native;

impl Native {
    pub fn open(_url: &str, _window: &Window) -> Option<(Native, UnboundedReceiver<String>)> {
        None
    }

    pub fn place(
        &self,
        _bounds: Bounds<Pixels>,
        _hidden: &[Bounds<Pixels>],
        _scale: f32,
        _visible: bool,
    ) {
    }

    pub fn close(&self) {}
}
