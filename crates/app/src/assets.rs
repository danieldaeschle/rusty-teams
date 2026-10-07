use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

const SYMBOLS: [(&str, &[u8]); 3] = [
    ("symbols/done.svg", include_bytes!("../assets/symbols/done.svg")),
    ("symbols/done_all.svg", include_bytes!("../assets/symbols/done_all.svg")),
    ("symbols/keyboard_return.svg", include_bytes!("../assets/symbols/keyboard_return.svg")),
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
