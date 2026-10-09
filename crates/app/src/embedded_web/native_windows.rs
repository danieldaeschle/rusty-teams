use std::cell::{Cell, OnceCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;

use gpui_kit::*;
use tokio::sync::mpsc::UnboundedReceiver;
use webview::{EmbedBounds, EmbedHost, EmbedSpec, MainEmbed};

use crate::{notify, theme};

pub use webview::EmbedEvent as NativeEvent;

static USER_DATA_FOLDER: OnceLock<PathBuf> = OnceLock::new();

thread_local! {
    static HOST: OnceCell<EmbedHost> = const { OnceCell::new() };
}

pub fn configure(user_data_folder: PathBuf) {
    let _ = USER_DATA_FOLDER.set(user_data_folder);
}

pub fn available() -> bool {
    USER_DATA_FOLDER.get().is_some()
}

fn host() -> Option<EmbedHost> {
    let folder = USER_DATA_FOLDER.get()?;
    Some(HOST.with(|host| host.get_or_init(|| EmbedHost::new(folder.clone())).clone()))
}

#[derive(Clone)]
pub struct Native {
    embed: MainEmbed,
    placed: Rc<Cell<Option<(EmbedBounds, bool)>>>,
}

impl Native {
    pub fn open(url: &str, window: &Window) -> Option<(Native, UnboundedReceiver<NativeEvent>)> {
        let parent = notify::native_handle(window)?;
        let (embed, events) = host()?.open(EmbedSpec {
            parent,
            url: url.to_owned(),
            background: theme::BACKGROUND,
        })?;
        let native = Native {
            embed,
            placed: Rc::new(Cell::new(None)),
        };
        Some((native, events))
    }

    pub fn place(&self, bounds: Bounds<Pixels>, scale: f32, visible: bool) {
        let placement = (
            EmbedBounds::from_logical(
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
                scale,
            ),
            visible,
        );
        if self.placed.get() == Some(placement) {
            return;
        }
        self.placed.set(Some(placement));
        self.embed.place(placement.0, placement.1);
    }

    pub fn close(&self) {
        self.embed.close();
    }
}
