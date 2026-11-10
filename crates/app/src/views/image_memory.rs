use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{
    App, AppContext, Entity, Global, ImageCacheError, ImageCacheItem, ImageSource, RenderImage, Resource, Window,
    hash,
};

const BUDGET_BYTES: u64 = 64 * 1024 * 1024;
const KEEP_RECENT: Duration = Duration::from_secs(10);

/// An image source whose decoded pixels are freed again once the image has been off screen for a while and the budget is full.
pub fn bounded(source: impl Into<ImageSource>) -> ImageSource {
    match source.into() {
        ImageSource::Resource(resource) => ImageSource::Custom(Arc::new(move |window, cx| load(&resource, window, cx))),
        other => other,
    }
}

struct SharedImages(Entity<BoundedImages>);

impl Global for SharedImages {}

fn load(resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
    let images = match cx.try_global::<SharedImages>() {
        Some(shared) => shared.0.clone(),
        None => {
            let images = cx.new(|_| BoundedImages::default());
            cx.set_global(SharedImages(images.clone()));
            images
        }
    };
    images.update(cx, |images, cx| images.load(resource, window, cx))
}

struct Entry {
    item: ImageCacheItem,
    last_use: Instant,
    bytes: u64,
}

#[derive(Default)]
struct BoundedImages {
    entries: HashMap<u64, Entry>,
}

impl BoundedImages {
    fn load(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let now = Instant::now();
        let key = hash(resource);
        let entry = self.entries.entry(key).or_insert_with(|| Entry { item: ImageCacheItem::new(resource, cx), last_use: now, bytes: 0 });
        entry.last_use = now;
        let result = entry.item.use_image(window);
        if entry.bytes == 0
            && let Some(Ok(image)) = &result
        {
            entry.bytes = decoded_bytes(image);
            self.trim(now);
        }
        result
    }

    fn trim(&mut self, now: Instant) {
        let sizes: Vec<(u64, Instant, u64)> =
            self.entries.iter().map(|(key, entry)| (*key, entry.last_use, entry.bytes)).collect();
        for key in evictions(sizes, now) {
            self.entries.remove(&key);
        }
    }
}

fn decoded_bytes(image: &RenderImage) -> u64 {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            u64::from(size.width.0.max(0) as u32) * u64::from(size.height.0.max(0) as u32) * 4
        })
        .sum::<u64>()
        .max(1)
}

fn evictions(mut sizes: Vec<(u64, Instant, u64)>, now: Instant) -> Vec<u64> {
    let mut total: u64 = sizes.iter().map(|(_, _, bytes)| bytes).sum();
    sizes.sort_by_key(|(_, last_use, _)| *last_use);
    let mut evicted = Vec::new();
    for (key, last_use, bytes) in sizes {
        if total <= BUDGET_BYTES || now.duration_since(last_use) < KEEP_RECENT {
            break;
        }
        total -= bytes;
        evicted.push(key);
    }
    evicted
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEGABYTE: u64 = 1024 * 1024;

    #[test]
    fn the_oldest_images_go_until_the_rest_fits() {
        let now = Instant::now() + Duration::from_secs(600);
        let sizes = vec![
            (1, now - Duration::from_secs(300), 40 * MEGABYTE),
            (2, now - Duration::from_secs(200), 30 * MEGABYTE),
            (3, now - Duration::from_secs(100), 20 * MEGABYTE),
        ];
        assert_eq!(evictions(sizes, now), vec![1]);
    }

    #[test]
    fn images_shown_just_now_stay_even_over_budget() {
        let now = Instant::now() + Duration::from_secs(600);
        let sizes = vec![(1, now - Duration::from_secs(2), 50 * MEGABYTE), (2, now, 50 * MEGABYTE)];
        assert!(evictions(sizes, now).is_empty());
    }

    #[test]
    fn nothing_goes_under_budget() {
        let now = Instant::now() + Duration::from_secs(600);
        assert!(evictions(vec![(1, now - Duration::from_secs(300), MEGABYTE)], now).is_empty());
    }
}
