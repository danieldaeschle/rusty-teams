use std::time::Duration;

use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

const PERSON_THRESHOLD: f32 = 0.5;
const MIN_REGION_AREA: f32 = 0.02;
const CROP_WIDTH_PER_SUBJECT_WIDTH: f32 = 1.8;
const HEADROOM: f32 = 0.12;
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 2.0;
const POSITION_DEADBAND: f32 = 0.06;
const SIZE_DEADBAND: f32 = 0.10;
const PAN_TIME_CONSTANT: f32 = 0.7;
const ZOOM_TIME_CONSTANT: f32 = 1.4;
const PAN_SPEED_CAP: f32 = 0.25;
const ZOOM_SPEED_CAP: f32 = 0.4;
const LOST_HOLD: f32 = 1.5;
const MAX_STEP: f32 = 0.25;
const SETTLED: f32 = 0.001;
const FULL_ZOOM_LIMIT: f32 = 1.001;
const ARRIVED: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subject {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// The crop is centered at (`center_x`, `center_y`) in frame fractions and keeps the frame's aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub center_x: f32,
    pub center_y: f32,
    pub zoom: f32,
}

pub const FULL_FRAME: Crop = Crop { center_x: 0.5, center_y: 0.5, zoom: MIN_ZOOM };

pub fn find_subject(mask: &[f32], width: usize, height: usize) -> Option<Subject> {
    let mut visited = vec![false; mask.len()];
    let mut stack = Vec::new();
    let mut best: Option<(usize, Subject)> = None;
    for start in 0..mask.len() {
        if visited[start] || mask[start] < PERSON_THRESHOLD {
            continue;
        }
        visited[start] = true;
        stack.push(start);
        let (mut area, mut left, mut right, mut top, mut bottom) = (0usize, width, 0usize, height, 0usize);
        while let Some(index) = stack.pop() {
            let (x, y) = (index % width, index / width);
            area += 1;
            (left, right, top, bottom) = (left.min(x), right.max(x), top.min(y), bottom.max(y));
            let neighbors = [(x > 0).then(|| index - 1), (x + 1 < width).then(|| index + 1), (y > 0).then(|| index - width), (y + 1 < height).then(|| index + width)];
            for next in neighbors.into_iter().flatten() {
                if !visited[next] && mask[next] >= PERSON_THRESHOLD {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
        let large_enough = area as f32 >= MIN_REGION_AREA * mask.len() as f32;
        if large_enough && best.is_none_or(|(largest, _)| area > largest) {
            let subject = Subject {
                left: left as f32 / width as f32,
                top: top as f32 / height as f32,
                right: (right + 1) as f32 / width as f32,
                bottom: (bottom + 1) as f32 / height as f32,
            };
            best = Some((area, subject));
        }
    }
    best.map(|(_, subject)| subject)
}

fn fit_center(center: f32, size: f32) -> f32 {
    center.clamp(size / 2., 1. - size / 2.)
}

pub fn crop_for(subject: &Subject) -> Crop {
    let crop_width = (subject.right - subject.left) * CROP_WIDTH_PER_SUBJECT_WIDTH;
    let zoom = (1. / crop_width).clamp(MIN_ZOOM, MAX_ZOOM);
    let size = 1. / zoom;
    let crop_top = subject.top - HEADROOM * size;
    Crop {
        center_x: fit_center((subject.left + subject.right) / 2., size),
        center_y: fit_center(crop_top + size / 2., size),
        zoom,
    }
}

fn ease(current: f32, target: f32, time_constant: f32, speed_cap: f32, seconds: f32) -> f32 {
    let weight = 1. - (-seconds / time_constant).exp();
    let cap = speed_cap * seconds;
    let remaining = target - current;
    if remaining.abs() < SETTLED {
        return target;
    }
    current + (remaining * weight).clamp(-cap, cap)
}

pub fn step_crop(current: Crop, target: Crop, seconds: f32, deadband: bool) -> Crop {
    let current_size = 1. / current.zoom;
    let offset = (target.center_x - current.center_x).abs().max((target.center_y - current.center_y).abs());
    let resize = (1. / target.zoom - current_size).abs();
    let pans = !deadband || offset / current_size > POSITION_DEADBAND;
    let zooms = !deadband || resize / current_size > SIZE_DEADBAND;
    let zoom = if zooms { ease(current.zoom, target.zoom, ZOOM_TIME_CONSTANT, ZOOM_SPEED_CAP, seconds) } else { current.zoom };
    let size = 1. / zoom;
    let (center_x, center_y) = if pans {
        (ease(current.center_x, target.center_x, PAN_TIME_CONSTANT, PAN_SPEED_CAP, seconds), ease(current.center_y, target.center_y, PAN_TIME_CONSTANT, PAN_SPEED_CAP, seconds))
    } else {
        (current.center_x, current.center_y)
    };
    Crop { center_x: fit_center(center_x, size), center_y: fit_center(center_y, size), zoom }
}

pub struct CropPlanner {
    crop: Crop,
    lost_seconds: f32,
    moving: bool,
}

impl Default for CropPlanner {
    fn default() -> Self {
        CropPlanner { crop: FULL_FRAME, lost_seconds: 0., moving: false }
    }
}

fn settled(current: &Crop, target: &Crop) -> bool {
    let size = 1. / current.zoom;
    let offset = (target.center_x - current.center_x).abs().max((target.center_y - current.center_y).abs());
    offset / size < ARRIVED && (1. / target.zoom - size).abs() / size < ARRIVED
}

impl CropPlanner {
    pub fn advance(&mut self, subject: Option<&Subject>, elapsed: Duration) -> Crop {
        let seconds = elapsed.as_secs_f32().min(MAX_STEP);
        match subject {
            Some(subject) => {
                self.lost_seconds = 0.;
                let target = crop_for(subject);
                let next = step_crop(self.crop, target, seconds, !self.moving);
                self.moving = next != self.crop && !settled(&next, &target);
                self.crop = next;
            }
            None => {
                self.lost_seconds += seconds;
                if self.lost_seconds >= LOST_HOLD {
                    self.crop = step_crop(self.crop, FULL_FRAME, seconds, false);
                }
            }
        }
        self.crop
    }

    pub fn reset(&mut self) {
        *self = CropPlanner::default();
    }
}

pub fn is_full_frame(crop: &Crop) -> bool {
    crop.zoom < FULL_ZOOM_LIMIT
}

pub fn crop_rectangle(crop: &Crop, width: usize, height: usize) -> (usize, usize, usize, usize) {
    let even = |value: f32| (value.round() as usize) & !1;
    let crop_width = even(width as f32 / crop.zoom).clamp(2, width);
    let crop_height = even(height as f32 / crop.zoom).clamp(2, height);
    let left = even(crop.center_x * width as f32 - crop_width as f32 / 2.).min(width - crop_width);
    let top = even(crop.center_y * height as f32 - crop_height as f32 / 2.).min(height - crop_height);
    (left, top, crop_width, crop_height)
}

fn copy_plane(source: &[u8], source_stride: usize, rectangle: (usize, usize, usize, usize), target: &mut [u8], target_stride: usize) {
    let (left, top, width, height) = rectangle;
    for row in 0..height {
        target[row * target_stride..][..width].copy_from_slice(&source[(top + row) * source_stride + left..][..width]);
    }
}

pub fn crop_and_scale(buffer: &I420Buffer, crop: &Crop) -> I420Buffer {
    let (width, height) = (buffer.width() as usize, buffer.height() as usize);
    let (left, top, crop_width, crop_height) = crop_rectangle(crop, width, height);
    let mut cropped = I420Buffer::new(crop_width as u32, crop_height as u32);
    let (source_strides, target_strides) = (buffer.strides(), cropped.strides());
    let (source_y, source_u, source_v) = buffer.data();
    let (target_y, target_u, target_v) = cropped.data_mut();
    copy_plane(source_y, source_strides.0 as usize, (left, top, crop_width, crop_height), target_y, target_strides.0 as usize);
    copy_plane(source_u, source_strides.1 as usize, (left / 2, top / 2, crop_width / 2, crop_height / 2), target_u, target_strides.1 as usize);
    copy_plane(source_v, source_strides.2 as usize, (left / 2, top / 2, crop_width / 2, crop_height / 2), target_v, target_strides.2 as usize);
    cropped.scale(width as i32, height as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 64;
    const HEIGHT: usize = 36;
    const FRAME: Duration = Duration::from_millis(33);

    fn mask_with(regions: &[(usize, usize, usize, usize)]) -> Vec<f32> {
        let mut mask = vec![0.; WIDTH * HEIGHT];
        for &(left, top, right, bottom) in regions {
            for y in top..bottom {
                for x in left..right {
                    mask[y * WIDTH + x] = 0.9;
                }
            }
        }
        mask
    }

    fn small_person() -> Subject {
        Subject { left: 0.4, top: 0.3, right: 0.5, bottom: 0.7 }
    }

    fn settle(planner: &mut CropPlanner, subject: Option<&Subject>, seconds: u32) -> Crop {
        let mut crop = planner.crop;
        for _ in 0..seconds * 30 {
            crop = planner.advance(subject, FRAME);
        }
        crop
    }

    #[test]
    fn the_largest_region_wins_and_small_ones_are_dropped() {
        let mask = mask_with(&[(10, 5, 30, 30), (40, 5, 46, 10), (50, 20, 54, 24)]);
        let subject = find_subject(&mask, WIDTH, HEIGHT).expect("a person");
        assert_eq!(subject, Subject { left: 10. / 64., top: 5. / 36., right: 30. / 64., bottom: 30. / 36. });
        assert!(find_subject(&mask_with(&[(40, 5, 46, 10)]), WIDTH, HEIGHT).is_none());
        assert!(find_subject(&vec![0.4; WIDTH * HEIGHT], WIDTH, HEIGHT).is_none());
        assert!(find_subject(&vec![0.6; WIDTH * HEIGHT], WIDTH, HEIGHT).is_some());
        assert!(find_subject(&vec![0.; WIDTH * HEIGHT], WIDTH, HEIGHT).is_none());
    }

    #[test]
    fn the_target_crop_is_wide_enough_has_headroom_and_stays_in_the_frame() {
        let crop = crop_for(&small_person());
        assert_eq!(crop.zoom, 2.);
        let size = 1. / crop.zoom;
        assert!(crop.center_y - size / 2. <= 0.3 - 0.12 * size + 1e-4);
        assert!(crop.center_x - size / 2. >= 0. && crop.center_y + size / 2. <= 1.);
        let wide = Subject { left: 0.1, top: 0.1, right: 0.9, bottom: 0.9 };
        assert_eq!(crop_for(&wide).zoom, 1.);
        let medium = Subject { left: 0.3, top: 0.2, right: 0.7, bottom: 0.9 };
        assert!((crop_for(&medium).zoom - 1. / 0.72).abs() < 1e-4);
    }

    #[test]
    fn the_crop_holds_while_the_target_is_inside_the_deadband() {
        let current = Crop { center_x: 0.5, center_y: 0.5, zoom: 1.5 };
        let size = 1. / current.zoom;
        let near = Crop { center_x: 0.5 + 0.05 * size, center_y: 0.5, zoom: 1. / (size * 1.09) };
        assert_eq!(step_crop(current, near, 0.033, true), current);
        let far = Crop { center_x: 0.5 + 0.2 * size, ..current };
        assert!(step_crop(current, far, 0.033, true).center_x > current.center_x);
        let bigger = Crop { zoom: 1. / (size * 1.2), ..current };
        assert!(step_crop(current, bigger, 0.033, true).zoom < current.zoom);
    }

    #[test]
    fn smoothing_converges_on_the_target_without_overshoot() {
        let mut planner = CropPlanner::default();
        let target = crop_for(&small_person());
        let crop = settle(&mut planner, Some(&small_person()), 20);
        assert!(settled(&crop, &target));
        assert!(crop.zoom <= MAX_ZOOM && crop.zoom >= MIN_ZOOM);
    }

    #[test]
    fn panning_never_exceeds_the_speed_cap() {
        let current = Crop { center_x: 0.3, center_y: 0.5, zoom: 2. };
        let target = Crop { center_x: 0.7, center_y: 0.5, zoom: 2. };
        let moved = step_crop(current, target, 0.1, true);
        assert!((moved.center_x - current.center_x) <= PAN_SPEED_CAP * 0.1 + 1e-6);
        assert!(moved.center_x > current.center_x);
        let zoomed = step_crop(FULL_FRAME, Crop { zoom: 2., ..FULL_FRAME }, 0.5, true);
        assert!(zoomed.zoom - 1. <= ZOOM_SPEED_CAP * 0.5 + 1e-6);
    }

    #[test]
    fn a_lost_subject_is_held_then_eased_out_to_the_full_frame() {
        let mut planner = CropPlanner::default();
        let zoomed = settle(&mut planner, Some(&small_person()), 20);
        assert!(zoomed.zoom > 1.5);
        let held = settle(&mut planner, None, 1);
        assert_eq!(held, zoomed);
        let easing = settle(&mut planner, None, 2);
        assert!(easing.zoom < zoomed.zoom);
        let full = settle(&mut planner, None, 10);
        assert_eq!(full, FULL_FRAME);
        planner.advance(Some(&small_person()), FRAME);
        assert_eq!(planner.lost_seconds, 0.);
    }

    #[test]
    fn the_crop_rectangle_is_even_inside_the_frame_and_scales_back() {
        let (left, top, width, height) = crop_rectangle(&Crop { center_x: 0.9, center_y: 0.1, zoom: 2. }, 1280, 720);
        assert_eq!((width, height), (640, 360));
        assert_eq!((left, top), (640, 0));
        assert!(left % 2 == 0 && top % 2 == 0);
        assert_eq!(crop_rectangle(&FULL_FRAME, 1280, 720), (0, 0, 1280, 720));
        let buffer = I420Buffer::new(64, 36);
        let scaled = crop_and_scale(&buffer, &Crop { center_x: 0.5, center_y: 0.5, zoom: 2. });
        assert_eq!((scaled.width(), scaled.height()), (64, 36));
    }
}
