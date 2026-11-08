use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use uuid::Uuid;

/// Teams web crops announcement backgrounds to this size.
pub const BANNER_IMAGE_WIDTH: u32 = 1317;
pub const BANNER_IMAGE_HEIGHT: u32 = 366;
const BANNER_DIRECTORY: &str = "banners";

/// The centered box with the banner's aspect ratio: `(x, y, width, height)`.
pub fn crop_box(width: u32, height: u32) -> (u32, u32, u32, u32) {
    let wanted_height = (u64::from(width) * u64::from(BANNER_IMAGE_HEIGHT) / u64::from(BANNER_IMAGE_WIDTH)) as u32;
    if wanted_height <= height {
        (0, (height - wanted_height) / 2, width, wanted_height.max(1))
    } else {
        let wanted_width = (u64::from(height) * u64::from(BANNER_IMAGE_WIDTH) / u64::from(BANNER_IMAGE_HEIGHT)) as u32;
        ((width - wanted_width) / 2, 0, wanted_width.max(1), height)
    }
}

/// Crops and scales `source` to the banner size and saves it as a PNG under the app data folder.
pub fn prepare_banner_image(source: &Path) -> Result<PathBuf, String> {
    let picked = image::open(source).map_err(|error| error.to_string())?;
    let (x, y, width, height) = crop_box(picked.width(), picked.height());
    let banner = picked.crop_imm(x, y, width, height).resize_exact(
        BANNER_IMAGE_WIDTH,
        BANNER_IMAGE_HEIGHT,
        FilterType::Triangle,
    );
    let directory = directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().join(BANNER_DIRECTORY))
        .ok_or("no data folder")?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let target = directory.join(format!("{}.png", Uuid::new_v4().simple()));
    banner
        .to_rgb8()
        .save_with_format(&target, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::crop_box;

    #[test]
    fn crop_keeps_the_middle_band_of_a_tall_or_wide_picture() {
        assert_eq!(crop_box(1317, 1000), (0, 317, 1317, 366));
        assert_eq!(crop_box(4000, 366), (1341, 0, 1317, 366));
        assert_eq!(crop_box(1317, 366), (0, 0, 1317, 366));
    }
}
