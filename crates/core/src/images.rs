use std::path::{Path, PathBuf};

use chrono::Utc;
use store::{ImageFileCache, ImageRecord};

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::events::CoreEvent;
use crate::image_size::sniff;
use crate::remote::Remote;
use crate::stored::ImageRef;

pub const IMAGE_CACHE_MAX_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredImage {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl<R: Remote> SyncEngine<R> {
    /// Keeps downloaded images as files in `directory` (least recently used evicted above `max_total_bytes`) instead of the database.
    pub fn with_image_dir(mut self, directory: &Path, max_total_bytes: u64) -> Result<Self> {
        self.image_files = Some(ImageFileCache::open(directory, max_total_bytes)?);
        Ok(self)
    }

    /// Cached file of the image, `None` when not downloaded yet or no image directory is set.
    pub fn image_path(&self, image: &ImageRef) -> Option<PathBuf> {
        self.image_files.as_ref()?.path(image.key())
    }

    pub fn image(&self, key: &str) -> Option<StoredImage> {
        let (bytes, content_type) = match &self.image_files {
            Some(files) => (std::fs::read(files.path(key)?).ok()?, String::new()),
            None => {
                let record = self.store.image(key).ok().flatten()?;
                (record.bytes, record.content_type)
            }
        };
        let size = sniff(&bytes);
        Some(StoredImage {
            content_type: if content_type.is_empty() {
                size.as_ref()
                    .map_or_else(String::new, |info| info.content_type.to_owned())
            } else {
                content_type
            },
            width: size.as_ref().map(|info| info.width),
            height: size.as_ref().map(|info| info.height),
            bytes,
        })
    }

    /// Downloads once. Returns the local file when an image directory is set; `None` when another fetch of the same image is running.
    /// Emits `ImagesChanged` once stored.
    pub async fn fetch_image(&self, image: &ImageRef) -> Result<Option<PathBuf>> {
        let key = image.key();
        if let Some(path) = self.image_path(image) {
            return Ok(Some(path));
        }
        if self.image_files.is_none() && self.store.has_image(key)? {
            return Ok(None);
        }
        if !self.claim_image(key) {
            return Ok(None);
        }
        let outcome = self.download_image(key).await;
        self.release_image(key);
        outcome
    }

    async fn download_image(&self, key: &str) -> Result<Option<PathBuf>> {
        let photo = self.remote.hosted_content(key).await?;
        let path = match &self.image_files {
            Some(files) => {
                let extension = file_extension(&photo.content_type, &photo.bytes);
                Some(files.put(key, &photo.bytes, extension)?)
            }
            None => {
                self.store.put_image(
                    &ImageRecord {
                        key: key.to_owned(),
                        bytes: photo.bytes,
                        content_type: photo.content_type,
                        fetched_at: Utc::now(),
                    },
                    IMAGE_CACHE_MAX_BYTES,
                )?;
                None
            }
        };
        let _ = self.events.send(CoreEvent::ImagesChanged {
            keys: vec![key.to_owned()],
        });
        Ok(path)
    }

    fn claim_image(&self, key: &str) -> bool {
        self.images_in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.to_owned())
    }

    fn release_image(&self, key: &str) {
        self.images_in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(key);
    }
}

fn file_extension(content_type: &str, bytes: &[u8]) -> &'static str {
    let kind = if content_type.is_empty() {
        sniff(bytes).map_or("", |info| info.content_type)
    } else {
        content_type
    };
    match kind {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "bin",
    }
}
