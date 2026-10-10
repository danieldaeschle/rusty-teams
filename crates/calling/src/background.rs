use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::Value;
use session::{Request, Scope, Session};

use crate::error::{Error, Result};

pub const CATALOG_URL: &str = "https://statics.teams.cdn.office.net/evergreen-assets/backgroundimages/config.json?v=7";
pub const CDN_ROOT: &str = "https://statics.teams.cdn.office.net";
const FETCH_SCOPE: &str = "Chat.Read";
const MAX_SIDE: u32 = 1920;
const CATALOG_FILE: &str = "config.json";
const THUMBNAIL_DIR: &str = "thumbnails";
const IMAGE_DIR: &str = "images";
const CHANNELS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundChoice {
    None,
    Blur,
    Image(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultBackground {
    pub id: String,
    pub name: String,
    pub image_url: String,
    pub thumbnail_url: String,
}

fn absolute(source: &str) -> String {
    if source.starts_with("http") { source.to_owned() } else { format!("{CDN_ROOT}{source}") }
}

fn file_id(id: &str) -> String {
    id.chars().map(|letter| if letter.is_ascii_alphanumeric() || letter == '-' || letter == '_' { letter } else { '_' }).collect()
}

fn extension_of(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit_once('.').map(|(_, extension)| extension).filter(|extension| (2..=4).contains(&extension.len())).unwrap_or("jpg")
}

pub fn parse_catalog(body: &Value) -> Vec<DefaultBackground> {
    body["videoBackgroundImages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let source = entry["src"].as_str().filter(|source| !source.is_empty())?;
            let id = entry["id"].as_str().filter(|id| !id.is_empty())?;
            Some(DefaultBackground {
                id: id.to_owned(),
                name: entry["name"].as_str().unwrap_or(id).to_owned(),
                image_url: absolute(source),
                thumbnail_url: absolute(entry["thumb_src"].as_str().filter(|thumb| !thumb.is_empty()).unwrap_or(source)),
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct BackgroundCache {
    directory: PathBuf,
}

impl BackgroundCache {
    pub fn new(directory: PathBuf) -> Self {
        BackgroundCache { directory }
    }

    pub fn thumbnail_path(&self, background: &DefaultBackground) -> PathBuf {
        self.directory.join(THUMBNAIL_DIR).join(format!("{}.{}", file_id(&background.id), extension_of(&background.thumbnail_url)))
    }

    pub fn image_path(&self, background: &DefaultBackground) -> PathBuf {
        self.directory.join(IMAGE_DIR).join(format!("{}.{}", file_id(&background.id), extension_of(&background.image_url)))
    }

    pub fn cached_catalog(&self) -> Vec<DefaultBackground> {
        std::fs::read(self.directory.join(CATALOG_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .map(|body| parse_catalog(&body))
            .unwrap_or_default()
    }

    pub async fn refresh_catalog(&self, session: &Session) -> Result<Vec<DefaultBackground>> {
        let bytes = download(session, CATALOG_URL).await?;
        let body: Value = serde_json::from_slice(&bytes).map_err(|error| Error::Signaling(format!("background list is not JSON: {error}")))?;
        let catalog = parse_catalog(&body);
        if !catalog.is_empty() {
            std::fs::create_dir_all(&self.directory).map_err(|error| Error::Webrtc(format!("background cache: {error}")))?;
            let _ = std::fs::write(self.directory.join(CATALOG_FILE), &bytes);
        }
        Ok(catalog)
    }

    pub async fn ensure_thumbnail(&self, session: &Session, background: &DefaultBackground) -> Result<PathBuf> {
        fetch_once(session, &background.thumbnail_url, self.thumbnail_path(background)).await
    }

    pub async fn ensure_image(&self, session: &Session, background: &DefaultBackground) -> Result<PathBuf> {
        fetch_once(session, &background.image_url, self.image_path(background)).await
    }
}

async fn fetch_once(session: &Session, url: &str, path: PathBuf) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path);
    }
    let bytes = download(session, url).await?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| Error::Webrtc(format!("background cache: {error}")))?;
    }
    let partial = path.with_extension("part");
    std::fs::write(&partial, bytes).and_then(|()| std::fs::rename(&partial, &path)).map_err(|error| Error::Webrtc(format!("background cache: {error}")))?;
    Ok(path)
}

async fn download(session: &Session, url: &str) -> Result<Vec<u8>> {
    let answer = session.send(Request::anonymous_binary_get(url, Vec::new()), &Scope::graph(FETCH_SCOPE)).await?;
    let encoded = answer.body["base64"].as_str().ok_or_else(|| Error::Signaling("download without a body".into()))?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| Error::Signaling(format!("download is not base64: {error}")))
}

#[derive(Debug, Clone)]
pub struct BackgroundPicture {
    rgb: Vec<u8>,
    width: usize,
    height: usize,
}

impl BackgroundPicture {
    pub fn from_rgb(rgb: Vec<u8>, width: usize, height: usize) -> Option<Self> {
        (width > 0 && height > 0 && rgb.len() == width * height * CHANNELS).then_some(BackgroundPicture { rgb, width, height })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let mut picture = image::open(path).map_err(|error| Error::Webrtc(format!("background image: {error}")))?;
        if picture.width().max(picture.height()) > MAX_SIDE {
            picture = picture.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
        }
        let rgb = picture.to_rgb8();
        let (width, height) = (rgb.width() as usize, rgb.height() as usize);
        BackgroundPicture::from_rgb(rgb.into_raw(), width, height).ok_or_else(|| Error::Webrtc("background image is empty".into()))
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn sample(&self, x: f32, y: f32, channel: usize) -> f32 {
        let (x, y) = (x.clamp(0., (self.width - 1) as f32), y.clamp(0., (self.height - 1) as f32));
        let (left, top) = (x.floor() as usize, y.floor() as usize);
        let (right, bottom) = ((left + 1).min(self.width - 1), (top + 1).min(self.height - 1));
        let (across, down) = (x - left as f32, y - top as f32);
        let at = |column: usize, row: usize| f32::from(self.rgb[(row * self.width + column) * CHANNELS + channel]);
        let upper = at(left, top) * (1. - across) + at(right, top) * across;
        let lower = at(left, bottom) * (1. - across) + at(right, bottom) * across;
        upper * (1. - down) + lower * down
    }

    pub fn covering(&self, width: usize, height: usize) -> Vec<u8> {
        let scale = (width as f32 / self.width as f32).max(height as f32 / self.height as f32);
        let offset_x = (self.width as f32 * scale - width as f32) / 2.;
        let offset_y = (self.height as f32 * scale - height as f32) / 2.;
        let mut covered = vec![0u8; width * height * CHANNELS];
        for row in 0..height {
            let source_y = (row as f32 + 0.5 + offset_y) / scale - 0.5;
            for column in 0..width {
                let source_x = (column as f32 + 0.5 + offset_x) / scale - 0.5;
                for channel in 0..CHANNELS {
                    covered[(row * width + column) * CHANNELS + channel] = (self.sample(source_x, source_y, channel) + 0.5) as u8;
                }
            }
        }
        covered
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn catalog() -> Value {
        json!({"videoBackgroundImages": [
            {"id": "office_01", "name": "Modern office", "filetype": "jpg", "src": "/evergreen-assets/backgroundimages/office_01.jpg", "thumb_src": "/evergreen-assets/backgroundimages/office_01_thumb.jpg"},
            {"id": "beach", "name": "Beach", "src": "https://cdn.example/beach.png"},
            {"id": "", "src": "/x.jpg"},
            {"id": "no_source"},
        ]})
    }

    #[test]
    fn the_catalog_lists_images_with_absolute_urls_and_skips_broken_entries() {
        let images = parse_catalog(&catalog());
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].id, "office_01");
        assert_eq!(images[0].name, "Modern office");
        assert_eq!(images[0].image_url, "https://statics.teams.cdn.office.net/evergreen-assets/backgroundimages/office_01.jpg");
        assert_eq!(images[0].thumbnail_url, "https://statics.teams.cdn.office.net/evergreen-assets/backgroundimages/office_01_thumb.jpg");
        assert_eq!(images[1].image_url, "https://cdn.example/beach.png");
        assert_eq!(images[1].thumbnail_url, "https://cdn.example/beach.png");
        assert!(parse_catalog(&json!({})).is_empty());
    }

    #[test]
    fn cached_files_live_under_the_cache_directory_by_id() {
        let cache = BackgroundCache::new(PathBuf::from("cache"));
        let images = parse_catalog(&catalog());
        assert_eq!(cache.thumbnail_path(&images[0]), Path::new("cache/thumbnails/office_01.jpg"));
        assert_eq!(cache.image_path(&images[1]), Path::new("cache/images/beach.png"));
        let odd = DefaultBackground { id: "a/b c".into(), name: String::new(), image_url: "https://x/y".into(), thumbnail_url: "https://x/y.webp?v=3".into() };
        assert_eq!(cache.thumbnail_path(&odd), Path::new("cache/thumbnails/a_b_c.webp"));
        assert_eq!(cache.image_path(&odd), Path::new("cache/images/a_b_c.jpg"));
    }

    #[test]
    fn a_saved_catalog_is_read_back_and_a_missing_one_is_empty() {
        let directory = tempfile_directory("catalog");
        let cache = BackgroundCache::new(directory.clone());
        assert!(cache.cached_catalog().is_empty());
        std::fs::write(directory.join("config.json"), catalog().to_string()).unwrap();
        assert_eq!(cache.cached_catalog().len(), 2);
        let _ = std::fs::remove_dir_all(directory);
    }

    fn tempfile_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("calling-background-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn halves(width: usize, height: usize) -> BackgroundPicture {
        let rgb = (0..width * height).flat_map(|index| if index % width < width / 2 { [200, 0, 0] } else { [0, 0, 200] }).collect();
        BackgroundPicture::from_rgb(rgb, width, height).unwrap()
    }

    #[test]
    fn a_wide_picture_covers_a_narrow_frame_and_crops_the_sides_evenly() {
        let covered = halves(40, 10).covering(10, 10);
        assert_eq!(covered.len(), 10 * 10 * 3);
        assert_eq!(&covered[(5 * 10) * 3..(5 * 10) * 3 + 3], &[200, 0, 0]);
        assert_eq!(&covered[(5 * 10 + 9) * 3..(5 * 10 + 9) * 3 + 3], &[0, 0, 200]);
    }

    #[test]
    fn a_picture_of_the_frame_shape_is_only_resampled() {
        let covered = halves(8, 4).covering(16, 8);
        assert_eq!(&covered[0..3], &[200, 0, 0]);
        assert_eq!(&covered[15 * 3..15 * 3 + 3], &[0, 0, 200]);
        assert_eq!(covered.len(), 16 * 8 * 3);
    }

    #[test]
    fn pictures_need_pixels_for_their_size() {
        assert!(BackgroundPicture::from_rgb(vec![0; 5], 2, 2).is_none());
        assert!(BackgroundPicture::from_rgb(Vec::new(), 0, 0).is_none());
    }

    #[test]
    fn a_picture_file_is_loaded_and_missing_ones_fail() {
        let directory = tempfile_directory("load");
        let path = directory.join("pic.png");
        image::RgbImage::from_pixel(6, 4, image::Rgb([10, 20, 30])).save(&path).unwrap();
        let picture = BackgroundPicture::load(&path).unwrap();
        assert_eq!(picture.size(), (6, 4));
        assert!(BackgroundPicture::load(&directory.join("none.png")).is_err());
        let _ = std::fs::remove_dir_all(directory);
    }
}
