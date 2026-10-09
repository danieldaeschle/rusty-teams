use std::path::Path;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, Rgba, RgbaImage};
use teams_core::Gif;

pub const RECEIVED_GIF_KEY: &str = "demo://party-gif";
pub const RECEIVED_GIF_NAME: &str = "party";
pub const RECEIVED_GIF_SIZE: (u32, u32) = (200, 150);
const FRAME_COUNT: u32 = 10;
const FRAME_DELAY_MILLIS: u32 = 90;

struct DemoGif {
    name: &'static str,
    title: &'static str,
    size: (u32, u32),
    hue: f32,
}

const DEMO_GIFS: [DemoGif; 9] = [
    DemoGif {
        name: "thumbs-up",
        title: "Thumbs up",
        size: (160, 160),
        hue: 210.,
    },
    DemoGif {
        name: "applause",
        title: "Applause",
        size: (200, 112),
        hue: 30.,
    },
    DemoGif {
        name: RECEIVED_GIF_NAME,
        title: "Party",
        size: RECEIVED_GIF_SIZE,
        hue: 320.,
    },
    DemoGif {
        name: "wave",
        title: "Wave",
        size: (150, 200),
        hue: 150.,
    },
    DemoGif {
        name: "laughing",
        title: "Laughing",
        size: (180, 135),
        hue: 50.,
    },
    DemoGif {
        name: "thank-you",
        title: "Thank you",
        size: (200, 100),
        hue: 0.,
    },
    DemoGif {
        name: "coffee",
        title: "Coffee",
        size: (140, 140),
        hue: 20.,
    },
    DemoGif {
        name: "dancing",
        title: "Dancing",
        size: (160, 200),
        hue: 270.,
    },
    DemoGif {
        name: "well-done",
        title: "Well done",
        size: (200, 120),
        hue: 180.,
    },
];

pub fn file_name(name: &str) -> String {
    format!("{name}.gif")
}

/// Writes the demo GIFs into `directory` and describes them like search results, with file paths as URLs.
pub fn generate(directory: &Path) -> Vec<Gif> {
    DEMO_GIFS
        .iter()
        .filter_map(|demo| {
            let path = directory.join(file_name(demo.name));
            std::fs::write(&path, encode(demo)).ok()?;
            let location = path.to_string_lossy().into_owned();
            Some(Gif {
                url: location.clone(),
                preview_url: location,
                title: demo.title.to_owned(),
                width: demo.size.0,
                height: demo.size.1,
                preview_width: demo.size.0,
                preview_height: demo.size.1,
            })
        })
        .collect()
}

fn encode(demo: &DemoGif) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut bytes, 30);
        let _ = encoder.set_repeat(Repeat::Infinite);
        for index in 0..FRAME_COUNT {
            let frame = Frame::from_parts(
                frame_image(demo, index),
                0,
                0,
                Delay::from_numer_denom_ms(FRAME_DELAY_MILLIS, 1),
            );
            let _ = encoder.encode_frame(frame);
        }
    }
    bytes
}

fn frame_image(demo: &DemoGif, index: u32) -> RgbaImage {
    let (width, height) = demo.size;
    let progress = index as f32 / FRAME_COUNT as f32;
    let center_x = width as f32 * (0.5 + 0.3 * (progress * std::f32::consts::TAU).sin());
    let center_y = height as f32 * (0.5 + 0.2 * (progress * std::f32::consts::TAU).cos());
    let radius = width.min(height) as f32 * 0.22;
    RgbaImage::from_fn(width, height, |x, y| {
        let distance = ((x as f32 - center_x).powi(2) + (y as f32 - center_y).powi(2)).sqrt();
        if distance < radius {
            return Rgba([255, 255, 255, 255]);
        }
        let shade = 0.35 + 0.35 * (y as f32 / height as f32);
        let [red, green, blue] = hsv(demo.hue + progress * 40., 0.6, shade + 0.15);
        Rgba([red, green, blue, 255])
    })
}

fn hsv(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let hue = hue.rem_euclid(360.) / 60.;
    let chroma = value * saturation;
    let second = chroma * (1. - (hue % 2. - 1.).abs());
    let (red, green, blue) = match hue as u32 {
        0 => (chroma, second, 0.),
        1 => (second, chroma, 0.),
        2 => (0., chroma, second),
        3 => (0., second, chroma),
        4 => (second, 0., chroma),
        _ => (chroma, 0., second),
    };
    let offset = value - chroma;
    [red, green, blue].map(|channel| ((channel + offset) * 255.).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_gifs_are_animated_files_with_their_declared_size() {
        let directory = tempfile::tempdir().unwrap();
        let gifs = generate(directory.path());
        assert_eq!(gifs.len(), DEMO_GIFS.len());
        let first = std::fs::read(&gifs[0].url).unwrap();
        assert!(first.starts_with(b"GIF89a"));
        assert_eq!(
            image::image_dimensions(&gifs[0].url).unwrap(),
            (gifs[0].width, gifs[0].height)
        );
    }
}
