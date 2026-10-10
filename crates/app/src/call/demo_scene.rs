use calling::VideoPicture;
use calling::blur::{MASK_HEIGHT, MASK_WIDTH, compose};

pub const SCENE_WIDTH: usize = 640;
pub const SCENE_HEIGHT: usize = 360;
const STRIPE: usize = 18;
const HEAD: (f32, f32, f32, f32) = (0.5, 0.36, 0.11, 0.2);
const SHOULDERS: (f32, f32, f32, f32) = (0.5, 0.92, 0.3, 0.38);
const SKIN: [u8; 3] = [226, 172, 140];
const SHIRT: [u8; 3] = [52, 92, 168];

fn inside(x: f32, y: f32, (center_x, center_y, radius_x, radius_y): (f32, f32, f32, f32)) -> bool {
    let (dx, dy) = ((x - center_x) / radius_x, (y - center_y) / radius_y);
    dx * dx + dy * dy <= 1.
}

fn person_at(x: f32, y: f32) -> Option<[u8; 3]> {
    if inside(x, y, HEAD) {
        Some(SKIN)
    } else if inside(x, y, SHOULDERS) {
        Some(SHIRT)
    } else {
        None
    }
}

fn background_at(column: usize, row: usize, tick: usize) -> [u8; 3] {
    let stripe = ((column + tick) / STRIPE + row / STRIPE).is_multiple_of(2);
    let shade = (row * 60 / SCENE_HEIGHT) as u8;
    if stripe { [214 - shade, 120 + shade, 60] } else { [48, 150 - shade, 168 + shade / 2] }
}

fn scene(tick: usize) -> Vec<u8> {
    let mut rgba = vec![255u8; SCENE_WIDTH * SCENE_HEIGHT * 4];
    for row in 0..SCENE_HEIGHT {
        for column in 0..SCENE_WIDTH {
            let (x, y) = ((column as f32 + 0.5) / SCENE_WIDTH as f32, (row as f32 + 0.5) / SCENE_HEIGHT as f32);
            let color = person_at(x, y).unwrap_or_else(|| background_at(column, row, tick));
            rgba[(row * SCENE_WIDTH + column) * 4..][..3].copy_from_slice(&color);
        }
    }
    rgba
}

fn person_mask() -> Vec<f32> {
    (0..MASK_WIDTH * MASK_HEIGHT)
        .map(|index| {
            let (x, y) = (((index % MASK_WIDTH) as f32 + 0.5) / MASK_WIDTH as f32, ((index / MASK_WIDTH) as f32 + 0.5) / MASK_HEIGHT as f32);
            if person_at(x, y).is_some() { 1. } else { 0. }
        })
        .collect()
}

/// A drawn head-and-shoulders scene; the mask comes from the drawing, not from the model.
pub fn self_view(tick: usize, blur: bool) -> VideoPicture {
    let mut rgba = scene(tick);
    if blur {
        compose(&mut rgba, SCENE_WIDTH, SCENE_HEIGHT, &person_mask());
    }
    let bgra = rgba.as_chunks::<4>().0.iter().flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]]).collect();
    VideoPicture { width: SCENE_WIDTH as u32, height: SCENE_HEIGHT as u32, bgra }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roughness(picture: &VideoPicture, x: usize, y: usize) -> i32 {
        let at = |column: usize| i32::from(picture.bgra[(y * SCENE_WIDTH + column) * 4]);
        (0..12).map(|step| (at(x + step + 1) - at(x + step)).abs()).sum()
    }

    #[test]
    fn blurring_the_scene_smooths_the_stripes_and_keeps_the_person() {
        let sharp = self_view(0, false);
        let blurred = self_view(0, true);
        assert!(roughness(&blurred, 30, 20) * 3 < roughness(&sharp, 30, 20));
        let center = (SCENE_HEIGHT / 2 * SCENE_WIDTH + SCENE_WIDTH / 2) * 4;
        assert_eq!(blurred.bgra[center..center + 3], sharp.bgra[center..center + 3]);
    }
}
