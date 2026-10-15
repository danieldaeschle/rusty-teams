use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, Asset, ImageCacheError, RenderImage, SharedUri};
use image::{Delay, Frame, ImageFormat, RgbaImage};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StripSource {
    pub url: SharedUri,
    pub frame_duration: Duration,
}

pub struct StripImages {
    pub animated: Arc<RenderImage>,
    pub still: Arc<RenderImage>,
}

pub enum StripLoader {}

impl Asset for StripLoader {
    type Source = StripSource;
    type Output = Result<Arc<StripImages>, ImageCacheError>;

    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let client = cx.http_client();
        async move {
            use futures_util::AsyncReadExt as _;
            let mut response = client.get(source.url.as_ref(), ().into(), true).await?;
            let mut body = Vec::new();
            response.body_mut().read_to_end(&mut body).await?;
            if !response.status().is_success() {
                return Err(ImageCacheError::BadStatus {
                    uri: source.url,
                    status: response.status(),
                    body: String::new(),
                });
            }
            let frames = split_strip(&body, source.frame_duration)?;
            let still = RenderImage::new(frames[..1].to_vec());
            Ok(Arc::new(StripImages {
                animated: Arc::new(RenderImage::new(frames)),
                still: Arc::new(still),
            }))
        }
    }
}

pub fn split_strip(
    bytes: &[u8],
    frame_duration: Duration,
) -> Result<Vec<Frame>, image::ImageError> {
    let strip = image::load(Cursor::new(bytes), ImageFormat::Png)?.into_rgba8();
    let side = strip.width().max(1);
    let count = (strip.height() / side).max(1);
    let delay = Delay::from_saturating_duration(frame_duration);
    Ok((0..count)
        .map(|index| {
            let mut frame: RgbaImage =
                image::imageops::crop_imm(&strip, 0, index * side, side, side.min(strip.height()))
                    .to_image();
            for pixel in frame.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            Frame::from_parts(frame, 0, 0, delay)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba};

    fn strip(side: u32, colors: &[[u8; 3]]) -> Vec<u8> {
        let mut image = RgbaImage::new(side, side * colors.len() as u32);
        for (index, color) in colors.iter().enumerate() {
            for y in 0..side {
                for x in 0..side {
                    image.put_pixel(
                        x,
                        index as u32 * side + y,
                        Rgba([color[0], color[1], color[2], 255]),
                    );
                }
            }
        }
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn a_strip_splits_into_square_bgra_frames_with_the_given_delay() {
        let bytes = strip(4, &[[255, 0, 0], [0, 255, 0], [0, 0, 255]]);
        let frames = split_strip(&bytes, Duration::from_millis(40)).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].buffer().dimensions(), (4, 4));
        assert_eq!(frames[0].buffer().get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(frames[2].buffer().get_pixel(3, 3).0, [255, 0, 0, 255]);
        assert_eq!(Duration::from(frames[1].delay()), Duration::from_millis(40));
    }
}
