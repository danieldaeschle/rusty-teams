use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;
use tokio::sync::mpsc::UnboundedReceiver;
use webview::{EmbedBounds, EmbedHandle, EmbedSpec};

use crate::{notify, task_dialog, theme};

pub use webview::EmbedEvent as NativeEvent;

pub fn available() -> bool {
    task_dialog::host().is_some()
}

#[derive(Clone)]
pub struct Native {
    handle: EmbedHandle,
    placed: Rc<Cell<Option<(EmbedBounds, bool)>>>,
}

impl Native {
    pub fn open(url: &str, window: &Window) -> Option<(Native, UnboundedReceiver<NativeEvent>)> {
        let host = task_dialog::host()?;
        let parent = notify::native_handle(window)?;
        let (handle, events) = host.open_embed(EmbedSpec {
            parent,
            url: url.to_owned(),
            background: theme::BACKGROUND,
        });
        let native = Native {
            handle,
            placed: Rc::new(Cell::new(None)),
        };
        Some((native, events))
    }

    pub fn place(&self, bounds: Bounds<Pixels>, scale: f32, visible: bool) {
        let physical = |value: Pixels| (f32::from(value) * scale).round() as i32;
        let placement = (
            EmbedBounds {
                x: physical(bounds.origin.x),
                y: physical(bounds.origin.y),
                width: physical(bounds.size.width),
                height: physical(bounds.size.height),
            },
            visible,
        );
        if self.placed.get() == Some(placement) {
            return;
        }
        self.placed.set(Some(placement));
        self.handle.set_bounds(placement.0);
        self.handle.set_visible(placement.1);
    }

    pub fn close(&self) {
        self.handle.close();
    }
}
