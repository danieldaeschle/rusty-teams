use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::{VideoHub, VideoKey, VideoPicture};
use gpui_kit::*;
use image::{Frame, RgbaImage};

const STALE_AFTER: Duration = Duration::from_millis(2500);

struct Shown {
    image: Arc<RenderImage>,
    updated: Instant,
}

#[derive(Default)]
pub struct CallPictures {
    shown: HashMap<VideoKey, Shown>,
}

fn render_image(picture: VideoPicture) -> Option<Arc<RenderImage>> {
    let bytes = RgbaImage::from_raw(picture.width, picture.height, picture.bgra)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(bytes)])))
}

impl CallPictures {
    pub fn apply(&mut self, hub: &VideoHub, cx: &mut App) {
        let now = Instant::now();
        for (key, picture) in hub.drain() {
            let picture = Arc::try_unwrap(picture).unwrap_or_else(|shared| (*shared).clone());
            let Some(image) = render_image(picture) else { continue };
            if let Some(previous) = self.shown.insert(key, Shown { image, updated: now }) {
                cx.drop_image(previous.image, None);
            }
        }
    }

    pub fn live(&self, key: &VideoKey, now: Instant) -> Option<Arc<RenderImage>> {
        self.shown
            .get(key)
            .filter(|shown| now.saturating_duration_since(shown.updated) < STALE_AFTER)
            .map(|shown| shown.image.clone())
    }

    pub fn forget(&mut self, key: &VideoKey, cx: &mut App) {
        if let Some(previous) = self.shown.remove(key) {
            cx.drop_image(previous.image, None);
        }
    }

    pub fn clear(&mut self, cx: &mut App) {
        for (_, previous) in self.shown.drain() {
            cx.drop_image(previous.image, None);
        }
    }
}
