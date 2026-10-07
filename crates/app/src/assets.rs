use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

macro_rules! symbols {
    ($($name:literal),* $(,)?) => {
        [$((concat!("symbols/", $name, ".svg"), include_bytes!(concat!("../assets/symbols/", $name, ".svg")) as &[u8])),*]
    };
}

const SYMBOLS: [(&str, &[u8]); 12] = symbols![
    "done",
    "done_all",
    "keyboard_return",
    "schedule",
    "close",
    "reply",
    "group",
    "tag",
    "image",
    "check_circle",
    "error",
    "notifications",
];

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = SYMBOLS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names = gpui_kit::assets::AllAssets.list(path)?;
        names.extend(
            SYMBOLS
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        Ok(names)
    }
}
