use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;
use std::sync::OnceLock;

use gpui_kit::*;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use webview::{EmbedBounds, EmbedEvent, EmbedHost, EmbedSpec, MainEmbed};

use crate::notify::platform;
use crate::theme;

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

fn physical(bounds: Bounds<Pixels>, scale: f32) -> EmbedBounds {
    EmbedBounds::from_logical(
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
        scale,
    )
}

pub struct Native {
    embed: MainEmbed,
    placed: Cell<Option<(EmbedBounds, bool)>>,
    cut: RefCell<Vec<EmbedBounds>>,
}

impl Native {
    pub fn open(url: &str, window: &Window) -> Option<(Native, UnboundedReceiver<String>)> {
        let parent = platform::native_handle(window)?;
        let (embed, mut events) = host()?.open(EmbedSpec {
            parent,
            url: url.to_owned(),
            background: theme::background_hex(),
        })?;
        let (opened, new_windows) = unbounded_channel();
        std::thread::spawn(move || {
            while let Some(event) = events.blocking_recv() {
                match event {
                    EmbedEvent::NewWindow(target) => {
                        let _ = opened.send(target);
                    }
                    EmbedEvent::Closed => break,
                    EmbedEvent::Navigated(_) | EmbedEvent::Loaded => {}
                }
            }
        });
        let native = Native {
            embed,
            placed: Cell::new(None),
            cut: RefCell::new(Vec::new()),
        };
        Some((native, new_windows))
    }

    pub fn place(
        &self,
        bounds: Bounds<Pixels>,
        hidden: &[Bounds<Pixels>],
        scale: f32,
        visible: bool,
    ) {
        let cut: Vec<EmbedBounds> = hidden.iter().map(|part| physical(*part, scale)).collect();
        if *self.cut.borrow() != cut {
            self.embed.set_cutouts(cut.clone());
            *self.cut.borrow_mut() = cut;
        }
        let placement = (physical(bounds, scale), visible);
        if self.placed.get() != Some(placement) {
            self.placed.set(Some(placement));
            self.embed.place(placement.0, placement.1);
        }
    }

    pub fn close(&self) {
        self.embed.close();
    }
}
