use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{
    App, AppContext, Entity, EntityId, Global, ImageCacheError, ImageCacheItem, ImageSource,
    RenderImage, Resource, Window, hash,
};
use image::imageops::FilterType;
use image::{Frame, ImageFormat, ImageReader};

const BUDGET_BYTES: u64 = 48 * MEGABYTE;
const KEEP_RECENT: Duration = Duration::from_secs(10);
const REPORT_EVERY: Duration = Duration::from_secs(60);
const MEGABYTE: u64 = 1024 * 1024;
const PREVIEW_EDGE: u32 = 720;

/// An image source whose decoded pixels are freed again once the image has been off screen for a while and the budget is full.
pub fn bounded(source: impl Into<ImageSource>) -> ImageSource {
    match source.into() {
        ImageSource::Resource(resource) => ImageSource::Custom(Arc::new(move |window, cx| {
            load(Request::Full(&resource), window, cx)
        })),
        other => other,
    }
}

/// Like [`bounded`], but a still picture is decoded scaled down to at most [`PREVIEW_EDGE`] pixels, for thumbnails of full-size photos.
pub fn preview(path: PathBuf) -> ImageSource {
    let path: Arc<Path> = path.into();
    ImageSource::Custom(Arc::new(move |window, cx| {
        load(Request::Preview(&path), window, cx)
    }))
}

#[derive(Clone, Copy)]
enum Request<'a> {
    Full(&'a Resource),
    Preview(&'a Arc<Path>),
}

struct SharedImages(Entity<BoundedImages>);

impl Global for SharedImages {}

fn load(
    request: Request,
    window: &mut Window,
    cx: &mut App,
) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
    let images = match cx.try_global::<SharedImages>() {
        Some(shared) => shared.0.clone(),
        None => {
            let images = cx.new(|_| BoundedImages::default());
            cx.set_global(SharedImages(images.clone()));
            images
        }
    };
    let handle = images.clone();
    images.update(cx, |images, cx| images.load(request, &handle, window, cx))
}

enum Pixels {
    Item(ImageCacheItem),
    Decoding { waiting: Vec<EntityId> },
    Decoded(Arc<RenderImage>),
}

struct Entry {
    pixels: Pixels,
    last_use: Instant,
    bytes: u64,
}

#[derive(Default)]
struct BoundedImages {
    entries: HashMap<u64, Entry>,
    freed_bytes: u64,
    last_report: Option<Instant>,
}

impl BoundedImages {
    fn load(
        &mut self,
        request: Request,
        handle: &Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let now = Instant::now();
        let key = match request {
            Request::Full(resource) => hash(resource),
            Request::Preview(path) => hash(&(path, PREVIEW_EDGE)),
        };
        let entry = self.entries.entry(key).or_insert_with(|| Entry {
            pixels: match request {
                Request::Full(resource) => Pixels::Item(ImageCacheItem::new(resource, cx)),
                Request::Preview(path) => {
                    start_preview(handle, key, path.clone(), cx);
                    Pixels::Decoding {
                        waiting: Vec::new(),
                    }
                }
            },
            last_use: now,
            bytes: 0,
        });
        entry.last_use = now;
        let result = match &mut entry.pixels {
            Pixels::Item(item) => item.use_image(window),
            Pixels::Decoding { waiting } => {
                let view = window.current_view();
                if !waiting.contains(&view) {
                    waiting.push(view);
                }
                None
            }
            Pixels::Decoded(image) => Some(Ok(image.clone())),
        };
        if entry.bytes == 0
            && let Some(Ok(image)) = &result
        {
            entry.bytes = decoded_bytes(image);
            self.trim(now);
        }
        result
    }

    fn finish_preview(
        &mut self,
        key: u64,
        path: Arc<Path>,
        decoded: Option<RenderImage>,
        cx: &mut App,
    ) {
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        let pixels = match decoded {
            Some(image) => Pixels::Decoded(Arc::new(image)),
            None => Pixels::Item(ImageCacheItem::new(&Resource::Path(path), cx)),
        };
        if let Pixels::Decoding { waiting } = std::mem::replace(&mut entry.pixels, pixels) {
            for view in waiting {
                cx.notify(view);
            }
        }
    }

    fn trim(&mut self, now: Instant) {
        let sizes: Vec<(u64, Instant, u64)> = self
            .entries
            .iter()
            .map(|(key, entry)| (*key, entry.last_use, entry.bytes))
            .collect();
        let mut freed_bytes = 0;
        for key in evictions(sizes, now) {
            freed_bytes += self.entries.remove(&key).map_or(0, |entry| entry.bytes);
        }
        self.freed_bytes += freed_bytes;
        if self
            .last_report
            .is_none_or(|last| now.duration_since(last) >= REPORT_EVERY)
        {
            self.last_report = Some(now);
            let kept_bytes: u64 = self.entries.values().map(|entry| entry.bytes).sum();
            app_base::diagnostics::record(&format!(
                "images: {} kept ({} MB), {} MB freed so far",
                self.entries.len(),
                kept_bytes / MEGABYTE,
                self.freed_bytes / MEGABYTE
            ));
        }
    }
}

fn start_preview(handle: &Entity<BoundedImages>, key: u64, path: Arc<Path>, cx: &mut App) {
    let images = handle.downgrade();
    cx.spawn(async move |cx| {
        let source = path.clone();
        let decoded = cx
            .background_executor()
            .spawn(async move { decode_preview(&source) })
            .await;
        images
            .update(cx, |images, cx| {
                images.finish_preview(key, path, decoded, cx)
            })
            .ok();
    })
    .detach();
}

fn decode_preview(path: &Path) -> Option<RenderImage> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    if !matches!(
        reader.format()?,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Bmp
    ) {
        return None;
    }
    let decoded = reader.decode().ok()?;
    let scaled = if decoded.width().max(decoded.height()) > PREVIEW_EDGE {
        decoded.resize(PREVIEW_EDGE, PREVIEW_EDGE, FilterType::Triangle)
    } else {
        decoded
    };
    let mut pixels = scaled.to_rgba8();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(RenderImage::new(vec![Frame::new(pixels)]))
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

    #[test]
    fn the_oldest_images_go_until_the_rest_fits() {
        let now = Instant::now() + Duration::from_secs(600);
        let sizes = vec![
            (1, now - Duration::from_secs(300), 40 * MEGABYTE),
            (2, now - Duration::from_secs(200), 30 * MEGABYTE),
            (3, now - Duration::from_secs(100), 20 * MEGABYTE),
        ];
        assert_eq!(evictions(sizes, now), vec![1, 2]);
    }

    #[test]
    fn images_shown_just_now_stay_even_over_budget() {
        let now = Instant::now() + Duration::from_secs(600);
        let sizes = vec![
            (1, now - Duration::from_secs(2), 50 * MEGABYTE),
            (2, now, 50 * MEGABYTE),
        ];
        assert!(evictions(sizes, now).is_empty());
    }

    #[test]
    fn nothing_goes_under_budget() {
        let now = Instant::now() + Duration::from_secs(600);
        assert!(evictions(vec![(1, now - Duration::from_secs(300), MEGABYTE)], now).is_empty());
    }

    #[test]
    fn a_large_photo_is_decoded_scaled_down_and_an_animation_is_left_to_gpui() {
        let directory = tempfile::tempdir().unwrap();
        let photo = directory.path().join("photo");
        image::RgbaImage::new(3000, 2000)
            .save_with_format(&photo, ImageFormat::Png)
            .unwrap();
        let decoded = decode_preview(&photo).unwrap();
        assert_eq!(
            (decoded.size(0).width.0, decoded.size(0).height.0),
            (720, 480)
        );
        let animation = directory.path().join("animation");
        image::RgbaImage::new(10, 10)
            .save_with_format(&animation, ImageFormat::Gif)
            .unwrap();
        assert!(decode_preview(&animation).is_none());
    }
}
