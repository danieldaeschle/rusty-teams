use std::path::{Path, PathBuf};

use calling::{BackgroundCache, BackgroundChoice, DefaultBackground};

pub const BACKGROUND_META_KEY: &str = "call_background";
pub const CUSTOM_BACKGROUNDS_META_KEY: &str = "call_background_custom";
pub const SHOWN_IMAGES: usize = 12;
const DEFAULT_PREFIX: &str = "default:";
const FILE_PREFIX: &str = "file:";
const CACHE_DIR: &str = "call-backgrounds";
const MAX_CUSTOM: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BackgroundPick {
    #[default]
    None,
    Blur,
    Default(String),
    Custom(PathBuf),
}

impl BackgroundPick {
    pub fn to_meta(&self) -> String {
        match self {
            BackgroundPick::None => "none".to_owned(),
            BackgroundPick::Blur => "blur".to_owned(),
            BackgroundPick::Default(id) => format!("{DEFAULT_PREFIX}{id}"),
            BackgroundPick::Custom(path) => format!("{FILE_PREFIX}{}", path.display()),
        }
    }

    pub fn from_meta(value: &str) -> Self {
        match value {
            "blur" | "1" => BackgroundPick::Blur,
            other => match (other.strip_prefix(DEFAULT_PREFIX), other.strip_prefix(FILE_PREFIX)) {
                (Some(id), _) if !id.is_empty() => BackgroundPick::Default(id.to_owned()),
                (_, Some(path)) if !path.is_empty() => BackgroundPick::Custom(PathBuf::from(path)),
                _ => BackgroundPick::None,
            },
        }
    }

    pub fn is_blur(&self) -> bool {
        *self == BackgroundPick::Blur
    }
}

pub fn load_background(store: &store::Store) -> BackgroundPick {
    store.meta(BACKGROUND_META_KEY).ok().flatten().or_else(|| store.meta("call_background_blur").ok().flatten()).map_or(BackgroundPick::None, |value| BackgroundPick::from_meta(&value))
}

pub fn load_custom_backgrounds(store: &store::Store) -> Vec<PathBuf> {
    store
        .meta(CUSTOM_BACKGROUNDS_META_KEY)
        .ok()
        .flatten()
        .map(|value| value.lines().filter(|line| !line.is_empty()).map(PathBuf::from).collect())
        .unwrap_or_default()
}

pub fn save_custom_backgrounds(store: &store::Store, customs: &[PathBuf]) {
    let joined: Vec<String> = customs.iter().map(|path| path.display().to_string()).collect();
    let _ = store.set_meta(CUSTOM_BACKGROUNDS_META_KEY, &joined.join("\n"));
}

pub fn default_cache_directory() -> PathBuf {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(CACHE_DIR))
        .unwrap_or_else(|| PathBuf::from(CACHE_DIR))
}

pub struct BackgroundLibrary {
    pub cache: BackgroundCache,
    pub images: Vec<DefaultBackground>,
    pub customs: Vec<PathBuf>,
    pub refreshing: bool,
}

impl BackgroundLibrary {
    pub fn new(directory: PathBuf) -> Self {
        let cache = BackgroundCache::new(directory);
        let images = cache.cached_catalog();
        BackgroundLibrary { cache, images, customs: Vec::new(), refreshing: false }
    }

    pub fn shown(&self) -> &[DefaultBackground] {
        &self.images[..self.images.len().min(SHOWN_IMAGES)]
    }

    pub fn thumbnail(&self, image: &DefaultBackground) -> Option<PathBuf> {
        Some(self.cache.thumbnail_path(image)).filter(|path| path.exists())
    }

    pub fn cached_image(&self, id: &str) -> Option<PathBuf> {
        let image = self.images.iter().find(|image| image.id == id)?;
        Some(self.cache.image_path(image)).filter(|path| path.exists())
    }

    pub fn find(&self, id: &str) -> Option<&DefaultBackground> {
        self.images.iter().find(|image| image.id == id)
    }

    pub fn add_custom(&mut self, path: PathBuf) {
        self.customs.retain(|known| *known != path);
        self.customs.insert(0, path);
        self.customs.truncate(MAX_CUSTOM);
    }

    pub fn needs_refresh(&self) -> bool {
        !self.refreshing && (self.images.is_empty() || self.shown().iter().any(|image| self.thumbnail(image).is_none()))
    }

    pub fn choice_for(&self, pick: &BackgroundPick) -> Option<BackgroundChoice> {
        match pick {
            BackgroundPick::None => Some(BackgroundChoice::None),
            BackgroundPick::Blur => Some(BackgroundChoice::Blur),
            BackgroundPick::Custom(path) => Some(BackgroundChoice::Image(path.clone())),
            BackgroundPick::Default(id) => self.cached_image(id).map(BackgroundChoice::Image),
        }
    }
}

const DEMO_NAMES: [&str; SHOWN_IMAGES] = [
    "Office", "Loft", "Library", "Beach", "Forest", "Sunset", "Studio", "Garden", "Cafe", "Mountains", "Cabin", "City",
];
const DEMO_THUMBNAIL: (u32, u32) = (96, 54);
const DEMO_IMAGE: (u32, u32) = (640, 360);

fn demo_gradient(index: usize, (width, height): (u32, u32)) -> image::RgbaImage {
    let hue = index as f32 / SHOWN_IMAGES as f32;
    let channel = |offset: f32, row: f32| (((hue + offset).fract() * 0.55 + 0.25 + row * 0.2) * 255.).min(255.) as u8;
    image::RgbaImage::from_fn(width, height, |_, row| {
        let row = row as f32 / height as f32;
        image::Rgba([channel(0., row), channel(0.33, row), channel(0.66, 1. - row), 255])
    })
}

pub fn demo_library() -> BackgroundLibrary {
    let directory = std::env::temp_dir().join(format!("teams-demo-backgrounds-{}", std::process::id()));
    let mut library = BackgroundLibrary::new(directory);
    library.images = DEMO_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| DefaultBackground {
            id: format!("demo_{index}"),
            name: (*name).to_owned(),
            image_url: format!("https://demo.invalid/images/demo_{index}.png"),
            thumbnail_url: format!("https://demo.invalid/thumbnails/demo_{index}.png"),
        })
        .collect();
    for (index, image) in library.images.iter().enumerate() {
        for (path, size) in [(library.cache.thumbnail_path(image), DEMO_THUMBNAIL), (library.cache.image_path(image), DEMO_IMAGE)] {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = demo_gradient(index, size).save(path);
        }
    }
    library
}

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| ["jpg", "jpeg", "png"].iter().any(|known| extension.eq_ignore_ascii_case(known)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_survives_the_round_trip_through_the_store_value() {
        for pick in [
            BackgroundPick::None,
            BackgroundPick::Blur,
            BackgroundPick::Default("office_01".into()),
            BackgroundPick::Custom(PathBuf::from("/home/me/pictures/wall.jpg")),
        ] {
            assert_eq!(BackgroundPick::from_meta(&pick.to_meta()), pick);
        }
    }

    #[test]
    fn the_old_blur_switch_and_unknown_values_still_read() {
        assert_eq!(BackgroundPick::from_meta("1"), BackgroundPick::Blur);
        assert_eq!(BackgroundPick::from_meta("0"), BackgroundPick::None);
        assert_eq!(BackgroundPick::from_meta("default:"), BackgroundPick::None);
        assert_eq!(BackgroundPick::from_meta("nonsense"), BackgroundPick::None);
    }

    #[test]
    fn the_store_keeps_the_choice_and_the_own_images() {
        let store = store::Store::open_in_memory().unwrap();
        assert_eq!(load_background(&store), BackgroundPick::None);
        store.set_meta(BACKGROUND_META_KEY, &BackgroundPick::Default("beach".into()).to_meta()).unwrap();
        assert_eq!(load_background(&store), BackgroundPick::Default("beach".into()));
        save_custom_backgrounds(&store, &[PathBuf::from("/a.png"), PathBuf::from("/b.jpg")]);
        assert_eq!(load_custom_backgrounds(&store), vec![PathBuf::from("/a.png"), PathBuf::from("/b.jpg")]);
    }

    #[test]
    fn the_legacy_blur_key_is_read_until_a_new_choice_is_saved() {
        let store = store::Store::open_in_memory().unwrap();
        store.set_meta("call_background_blur", "1").unwrap();
        assert_eq!(load_background(&store), BackgroundPick::Blur);
    }

    #[test]
    fn own_images_come_first_without_duplicates_and_stay_few() {
        let mut library = BackgroundLibrary::new(std::env::temp_dir().join("calls-test-no-such-cache"));
        for index in 0..8 {
            library.add_custom(PathBuf::from(format!("/pictures/{index}.png")));
        }
        library.add_custom(PathBuf::from("/pictures/3.png"));
        assert_eq!(library.customs.len(), MAX_CUSTOM);
        assert_eq!(library.customs[0], PathBuf::from("/pictures/3.png"));
        assert_eq!(library.customs.iter().filter(|path| path.as_path() == Path::new("/pictures/3.png")).count(), 1);
    }

    #[test]
    fn a_default_image_is_only_usable_once_it_is_cached() {
        let library = BackgroundLibrary::new(std::env::temp_dir().join("calls-test-no-such-cache"));
        assert_eq!(library.choice_for(&BackgroundPick::Default("office_01".into())), None);
        assert_eq!(library.choice_for(&BackgroundPick::Blur), Some(BackgroundChoice::Blur));
        assert!(library.needs_refresh());
    }

    #[test]
    fn only_pictures_are_accepted_as_own_backgrounds() {
        assert!(is_image_file(Path::new("/a/b.JPG")));
        assert!(is_image_file(Path::new("c.png")));
        assert!(!is_image_file(Path::new("d.gif")));
        assert!(!is_image_file(Path::new("noextension")));
    }
}
