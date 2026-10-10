use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use libwebrtc::native::yuv_helper::i420_to_abgr;
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};
use tract_onnx::prelude::*;

use crate::background::BackgroundPicture;
use crate::camera::i420_from_rgba;
use crate::error::{Error, Result};

const MODEL: &[u8] = include_bytes!("../assets/selfie_segmentation_landscape.onnx");
pub const MASK_WIDTH: usize = 256;
pub const MASK_HEIGHT: usize = 144;
const CHANNELS: usize = 4;
const BACKGROUND_SCALE: usize = 4;
const BLUR_RADIUS: usize = 3;
const BLUR_PASSES: usize = 2;
const EDGE_LOW: f32 = 0.3;
const EDGE_HIGH: f32 = 0.7;
const OPAQUE: f32 = 0.985;
const MASK_SMOOTHING: f32 = 0.35;
const SLOW_INFERENCE: Duration = Duration::from_millis(22);
const VERY_SLOW_INFERENCE: Duration = Duration::from_millis(45);
const AVERAGE_WEIGHT: f64 = 0.1;
const MAX_BANDS: usize = 4;

type Plan = std::sync::Arc<TypedRunnableModel>;

/// MediaPipe selfie segmentation (landscape, 256x144); answers the probability of "person" per mask pixel.
pub struct Segmenter {
    plan: Plan,
}

impl Segmenter {
    pub fn new() -> Result<Self> {
        let plan = Self::load().map_err(|error| Error::Webrtc(format!("segmentation model: {error}")))?;
        Ok(Segmenter { plan })
    }

    fn load() -> TractResult<Plan> {
        let model = tract_onnx::onnx().model_for_read(&mut Cursor::new(MODEL))?;
        let batch = model.symbols.sym("batch_size");
        let one_image = std::collections::HashMap::from([(batch, TDim::from(1))]);
        model.into_typed()?.set_symbols(&one_image)?.into_optimized()?.into_runnable()
    }

    pub fn mask(&self, rgba: &[u8], width: usize, height: usize) -> Result<Vec<f32>> {
        let input = model_input(rgba, width, height);
        let outputs = self.plan.run(tvec!(input.into())).map_err(|error| Error::Webrtc(format!("segmentation: {error}")))?;
        let values = outputs[0].to_plain_array_view::<f32>().map_err(|error| Error::Webrtc(format!("segmentation output: {error}")))?;
        if values.len() != MASK_WIDTH * MASK_HEIGHT {
            return Err(Error::Webrtc(format!("segmentation output has {} values", values.len())));
        }
        Ok(values.iter().map(|value| value.clamp(0., 1.)).collect())
    }
}

fn model_input(rgba: &[u8], width: usize, height: usize) -> Tensor {
    let plane = MASK_WIDTH * MASK_HEIGHT;
    let mut data = vec![0f32; 3 * plane];
    for row in 0..MASK_HEIGHT {
        let top = (row * 2 * height / (MASK_HEIGHT * 2)).min(height - 1);
        let bottom = ((row * 2 + 1) * height / (MASK_HEIGHT * 2)).min(height - 1);
        for column in 0..MASK_WIDTH {
            let left = (column * 2 * width / (MASK_WIDTH * 2)).min(width - 1);
            let right = ((column * 2 + 1) * width / (MASK_WIDTH * 2)).min(width - 1);
            for channel in 0..3 {
                let sum: u32 = [(top, left), (top, right), (bottom, left), (bottom, right)]
                    .iter()
                    .map(|&(y, x)| u32::from(rgba[(y * width + x) * CHANNELS + channel]))
                    .sum();
                data[channel * plane + row * MASK_WIDTH + column] = sum as f32 / (4. * 255.);
            }
        }
    }
    Tensor::from_shape(&[1, 3, MASK_HEIGHT, MASK_WIDTH], &data).expect("shape matches data")
}

fn smoothstep(value: f32) -> f32 {
    let t = ((value - EDGE_LOW) / (EDGE_HIGH - EDGE_LOW)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

struct Axis {
    first: Vec<usize>,
    second: Vec<usize>,
    weight: Vec<f32>,
}

fn axis(output: usize, input: usize) -> Axis {
    let mut axis = Axis { first: Vec::with_capacity(output), second: Vec::with_capacity(output), weight: Vec::with_capacity(output) };
    for index in 0..output {
        let position = ((index as f32 + 0.5) * input as f32 / output as f32 - 0.5).clamp(0., (input - 1) as f32);
        let first = position.floor() as usize;
        axis.first.push(first);
        axis.second.push((first + 1).min(input - 1));
        axis.weight.push(position - first as f32);
    }
    axis
}

fn downscaled(rgba: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let small_width = width.div_ceil(BACKGROUND_SCALE);
    let small_height = height.div_ceil(BACKGROUND_SCALE);
    let mut small = vec![0u8; small_width * small_height * 3];
    for row in 0..small_height {
        for column in 0..small_width {
            let mut sums = [0u32; 3];
            let mut count = 0u32;
            for y in row * BACKGROUND_SCALE..((row + 1) * BACKGROUND_SCALE).min(height) {
                for x in column * BACKGROUND_SCALE..((column + 1) * BACKGROUND_SCALE).min(width) {
                    for (channel, sum) in sums.iter_mut().enumerate() {
                        *sum += u32::from(rgba[(y * width + x) * CHANNELS + channel]);
                    }
                    count += 1;
                }
            }
            for (channel, sum) in sums.iter().enumerate() {
                small[(row * small_width + column) * 3 + channel] = (sum / count) as u8;
            }
        }
    }
    (small, small_width, small_height)
}

fn blur_pass(input: &[u8], output: &mut [u8], lines: usize, length: usize, line_stride: usize, step: usize) {
    let radius = BLUR_RADIUS as isize;
    let window = (BLUR_RADIUS * 2 + 1) as u32;
    for line in 0..lines {
        for channel in 0..3 {
            let base = line * line_stride + channel;
            let read = |index: isize| u32::from(input[base + index.clamp(0, length as isize - 1) as usize * step]);
            let mut sum: u32 = (-radius..=radius).map(read).sum();
            for index in 0..length {
                output[base + index * step] = (sum / window) as u8;
                sum = sum + read(index as isize + radius + 1) - read(index as isize - radius);
            }
        }
    }
}

fn box_blur(image: &mut [u8], width: usize, height: usize, scratch: &mut [u8]) {
    blur_pass(image, scratch, height, width, width * 3, 3);
    blur_pass(scratch, image, width, height, 3, width * 3);
}

fn blurred_background(rgba: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let (mut small, small_width, small_height) = downscaled(rgba, width, height);
    let mut scratch = vec![0u8; small.len()];
    for _ in 0..BLUR_PASSES {
        box_blur(&mut small, small_width, small_height, &mut scratch);
    }
    (small, small_width, small_height)
}

struct Composer<'a> {
    background: &'a [u8],
    mask: &'a [f32],
    width: usize,
    mask_x: Axis,
    mask_y: Axis,
    small_x: Axis,
    small_y: Axis,
    small_width: usize,
}

impl Composer<'_> {
    fn rows(&self, rgba: &mut [u8], first_row: usize) {
        for (offset, line) in rgba.chunks_exact_mut(self.width * CHANNELS).enumerate() {
            self.row(line, first_row + offset);
        }
    }

    fn row(&self, line: &mut [u8], row: usize) {
        let (mask_top, mask_bottom, mask_fraction) = (self.mask_y.first[row] * MASK_WIDTH, self.mask_y.second[row] * MASK_WIDTH, self.mask_y.weight[row]);
        let (small_top, small_bottom, small_fraction) = (self.small_y.first[row] * self.small_width, self.small_y.second[row] * self.small_width, self.small_y.weight[row]);
        for (column, pixel) in line.as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
            let (left, right, across) = (self.mask_x.first[column], self.mask_x.second[column], self.mask_x.weight[column]);
            let top = self.mask[mask_top + left] * (1. - across) + self.mask[mask_top + right] * across;
            let bottom = self.mask[mask_bottom + left] * (1. - across) + self.mask[mask_bottom + right] * across;
            let person = smoothstep(top * (1. - mask_fraction) + bottom * mask_fraction);
            if person >= OPAQUE {
                continue;
            }
            let (left, right, across) = (self.small_x.first[column], self.small_x.second[column], self.small_x.weight[column]);
            for (channel, value) in pixel.iter_mut().take(3).enumerate() {
                let sample = |line: usize, index: usize| f32::from(self.background[(line + index) * 3 + channel]);
                let upper = sample(small_top, left) * (1. - across) + sample(small_top, right) * across;
                let lower = sample(small_bottom, left) * (1. - across) + sample(small_bottom, right) * across;
                let blurred = upper * (1. - small_fraction) + lower * small_fraction;
                *value = (f32::from(*value) * person + blurred * (1. - person) + 0.5) as u8;
            }
        }
    }
}

fn compose_over(rgba: &mut [u8], width: usize, height: usize, mask: &[f32], background: &[u8], background_width: usize, background_height: usize) {
    let composer = Composer {
        background,
        mask,
        width,
        mask_x: axis(width, MASK_WIDTH),
        mask_y: axis(height, MASK_HEIGHT),
        small_x: axis(width, background_width),
        small_y: axis(height, background_height),
        small_width: background_width,
    };
    let bands = std::thread::available_parallelism().map_or(1, usize::from).min(MAX_BANDS);
    let rows_per_band = height.div_ceil(bands);
    std::thread::scope(|scope| {
        for (index, band) in rgba.chunks_mut(rows_per_band * width * CHANNELS).enumerate() {
            let composer = &composer;
            scope.spawn(move || composer.rows(band, index * rows_per_band));
        }
    });
}

/// Keeps the person (mask near 1) sharp and replaces the rest with a blurred copy; `mask` is `MASK_WIDTH` x `MASK_HEIGHT`.
pub fn compose(rgba: &mut [u8], width: usize, height: usize, mask: &[f32]) {
    let (background, small_width, small_height) = blurred_background(rgba, width, height);
    compose_over(rgba, width, height, mask, &background, small_width, small_height);
}

pub fn compose_image(rgba: &mut [u8], width: usize, height: usize, mask: &[f32], covered: &[u8]) {
    compose_over(rgba, width, height, mask, covered, width, height);
}

/// Shared between the UI command and the camera thread.
#[derive(Default)]
pub struct BlurSettings {
    enabled: AtomicBool,
    average_micros: AtomicU32,
    picture: Mutex<Option<Arc<BackgroundPicture>>>,
}

impl BlurSettings {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_picture(&self, picture: Option<Arc<BackgroundPicture>>) {
        *self.picture.lock().expect("picture lock") = picture;
    }

    fn picture(&self) -> Option<Arc<BackgroundPicture>> {
        self.picture.lock().expect("picture lock").clone()
    }

    pub fn average_ms(&self) -> f32 {
        self.average_micros.load(Ordering::Relaxed) as f32 / 1000.
    }
}

/// Runs on the capture thread; loads the model on first use and stays out of the way while the setting is off.
pub struct BlurStage {
    settings: Arc<BlurSettings>,
    blur: Option<BackgroundBlur>,
    unavailable: bool,
    covered: Option<CoveredPicture>,
}

struct CoveredPicture {
    picture: Arc<BackgroundPicture>,
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

impl BlurStage {
    pub fn new(settings: Arc<BlurSettings>) -> Self {
        BlurStage { settings, blur: None, unavailable: false, covered: None }
    }

    fn refresh_covered(&mut self, width: usize, height: usize) {
        let Some(picture) = self.settings.picture() else {
            self.covered = None;
            return;
        };
        let fresh = self.covered.as_ref().is_some_and(|covered| Arc::ptr_eq(&covered.picture, &picture) && covered.width == width && covered.height == height);
        if !fresh {
            let rgb = picture.covering(width, height);
            self.covered = Some(CoveredPicture { picture, width, height, rgb });
        }
    }

    pub fn apply_rgba(&mut self, rgba: &mut [u8], width: usize, height: usize) {
        if !self.settings.enabled() || self.unavailable {
            return;
        }
        if self.blur.is_none() {
            match BackgroundBlur::new() {
                Ok(blur) => self.blur = Some(blur),
                Err(_) => {
                    self.unavailable = true;
                    return;
                }
            }
        }
        self.refresh_covered(width, height);
        if let Some(blur) = self.blur.as_mut() {
            blur.apply(rgba, width, height, self.covered.as_ref().map(|covered| covered.rgb.as_slice()));
            self.settings.average_micros.store((blur.average_ms() * 1000.) as u32, Ordering::Relaxed);
        }
    }

    pub fn apply_i420(&mut self, buffer: I420Buffer) -> I420Buffer {
        if !self.settings.enabled() || self.unavailable {
            return buffer;
        }
        let (width, height) = (buffer.width() as usize, buffer.height() as usize);
        let (stride_y, stride_u, stride_v) = buffer.strides();
        let (plane_y, plane_u, plane_v) = buffer.data();
        let mut rgba = vec![0u8; width * height * CHANNELS];
        i420_to_abgr(plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, &mut rgba, (width * CHANNELS) as u32, width as i32, height as i32);
        self.apply_rgba(&mut rgba, width, height);
        i420_from_rgba(&rgba, width as u32, height as u32)
    }
}

/// Background blur for camera frames: segments every `stride` frames and slows down when inference is slow.
pub struct BackgroundBlur {
    segmenter: Segmenter,
    mask: Vec<f32>,
    frames: u64,
    stride: u64,
    average_ms: f64,
    processed: u64,
}

impl BackgroundBlur {
    pub fn new() -> Result<Self> {
        Ok(BackgroundBlur { segmenter: Segmenter::new()?, mask: Vec::new(), frames: 0, stride: 1, average_ms: 0., processed: 0 })
    }

    pub fn apply(&mut self, rgba: &mut [u8], width: usize, height: usize, covered: Option<&[u8]>) {
        let started = Instant::now();
        if self.frames.is_multiple_of(self.stride) || self.mask.is_empty() {
            self.segment(rgba, width, height, started);
        }
        self.frames += 1;
        match covered {
            Some(covered) => compose_image(rgba, width, height, &self.mask, covered),
            None => compose(rgba, width, height, &self.mask),
        }
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        self.average_ms = if self.processed == 0 { elapsed } else { self.average_ms * (1. - AVERAGE_WEIGHT) + elapsed * AVERAGE_WEIGHT };
        self.processed += 1;
    }

    fn segment(&mut self, rgba: &[u8], width: usize, height: usize, started: Instant) {
        let Ok(fresh) = self.segmenter.mask(rgba, width, height) else {
            return;
        };
        let inference = started.elapsed();
        self.stride = if inference > VERY_SLOW_INFERENCE {
            3
        } else if inference > SLOW_INFERENCE {
            2
        } else {
            1
        };
        if self.mask.is_empty() {
            self.mask = fresh;
            return;
        }
        for (current, new) in self.mask.iter_mut().zip(fresh) {
            *current += (new - *current) * (1. - MASK_SMOOTHING);
        }
    }

    pub fn average_ms(&self) -> f64 {
        self.average_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 160;
    const HEIGHT: usize = 90;

    fn checkerboard() -> Vec<u8> {
        let mut rgba = vec![255u8; WIDTH * HEIGHT * CHANNELS];
        for row in 0..HEIGHT {
            for column in 0..WIDTH {
                let value = if (row / 3 + column / 3) % 2 == 0 { 20 } else { 235 };
                rgba[(row * WIDTH + column) * CHANNELS..][..3].fill(value);
            }
        }
        rgba
    }

    fn oval_mask() -> Vec<f32> {
        (0..MASK_WIDTH * MASK_HEIGHT)
            .map(|index| {
                let (x, y) = ((index % MASK_WIDTH) as f32 / MASK_WIDTH as f32 - 0.5, (index / MASK_WIDTH) as f32 / MASK_HEIGHT as f32 - 0.5);
                if (x * x) / 0.04 + (y * y) / 0.09 <= 1. { 1. } else { 0. }
            })
            .collect()
    }

    fn pixel(rgba: &[u8], x: usize, y: usize) -> u8 {
        rgba[(y * WIDTH + x) * CHANNELS]
    }

    fn roughness(rgba: &[u8], x: usize, y: usize) -> i32 {
        (0..8).map(|step| (i32::from(pixel(rgba, x + step + 1, y)) - i32::from(pixel(rgba, x + step, y))).abs()).sum()
    }

    #[test]
    fn the_person_stays_sharp_and_the_background_is_smoothed() {
        let original = checkerboard();
        let mut blurred = original.clone();
        compose(&mut blurred, WIDTH, HEIGHT, &oval_mask());
        let (center_x, center_y) = (WIDTH / 2, HEIGHT / 2);
        assert_eq!(pixel(&blurred, center_x, center_y), pixel(&original, center_x, center_y));
        assert_eq!(roughness(&blurred, center_x - 4, center_y), roughness(&original, center_x - 4, center_y));
        assert!(roughness(&blurred, 2, 2) * 4 < roughness(&original, 2, 2));
        assert!(roughness(&blurred, WIDTH - 12, HEIGHT - 3) * 4 < roughness(&original, WIDTH - 12, HEIGHT - 3));
    }

    #[test]
    fn the_edge_between_person_and_background_is_feathered() {
        let original = checkerboard();
        let mut blurred = original.clone();
        let mut mask = oval_mask();
        compose(&mut blurred, WIDTH, HEIGHT, &mask);
        let edge_row = HEIGHT / 2;
        let changed_steps = (0..WIDTH - 1)
            .filter(|&x| pixel(&blurred, x, edge_row) != pixel(&original, x, edge_row) && pixel(&blurred, x + 1, edge_row) == pixel(&original, x + 1, edge_row))
            .count();
        assert!(changed_steps <= 2, "a hard seam shows as one switch per side");
        mask.fill(1.);
        let mut untouched = original.clone();
        compose(&mut untouched, WIDTH, HEIGHT, &mask);
        assert_eq!(untouched, original);
    }

    #[test]
    fn an_empty_mask_blurs_the_whole_frame() {
        let original = checkerboard();
        let mut blurred = original.clone();
        compose(&mut blurred, WIDTH, HEIGHT, &vec![0.; MASK_WIDTH * MASK_HEIGHT]);
        assert!(roughness(&blurred, 70, 40) * 4 < roughness(&original, 70, 40));
        assert!(blurred.chunks(CHANNELS).all(|pixel| pixel[3] == 255));
    }

    fn cover_of(color: [u8; 3]) -> Vec<u8> {
        BackgroundPicture::from_rgb(color.repeat(4 * 4), 4, 4).unwrap().covering(WIDTH, HEIGHT)
    }

    #[test]
    fn the_person_stays_and_the_picture_replaces_everything_else() {
        let original = checkerboard();
        let mut replaced = original.clone();
        compose_image(&mut replaced, WIDTH, HEIGHT, &oval_mask(), &cover_of([10, 120, 200]));
        let (center_x, center_y) = (WIDTH / 2, HEIGHT / 2);
        assert_eq!(pixel(&replaced, center_x, center_y), pixel(&original, center_x, center_y));
        for (x, y) in [(2, 2), (WIDTH - 3, HEIGHT - 3), (2, HEIGHT - 3)] {
            assert_eq!(&replaced[(y * WIDTH + x) * CHANNELS..][..4], &[10, 120, 200, 255]);
        }
    }

    #[test]
    fn the_picture_edge_is_feathered_like_the_blur_edge() {
        let original = checkerboard();
        let mut replaced = original.clone();
        compose_image(&mut replaced, WIDTH, HEIGHT, &oval_mask(), &cover_of([0, 0, 0]));
        let edge_row = HEIGHT / 2;
        let switches = (0..WIDTH - 1)
            .filter(|&x| pixel(&replaced, x, edge_row) != pixel(&original, x, edge_row) && pixel(&replaced, x + 1, edge_row) == pixel(&original, x + 1, edge_row))
            .count();
        assert!(switches <= 2);
        let mut everyone = original.clone();
        compose_image(&mut everyone, WIDTH, HEIGHT, &vec![1.; MASK_WIDTH * MASK_HEIGHT], &cover_of([0, 0, 0]));
        assert_eq!(everyone, original);
        let mut nobody = original;
        compose_image(&mut nobody, WIDTH, HEIGHT, &vec![0.; MASK_WIDTH * MASK_HEIGHT], &cover_of([0, 0, 0]));
        assert!(nobody.chunks(CHANNELS).all(|pixel| pixel[..3] == [0, 0, 0] && pixel[3] == 255));
    }

    #[test]
    fn the_model_answers_one_probability_per_mask_pixel() {
        let segmenter = Segmenter::new().expect("model loads");
        let mask = segmenter.mask(&checkerboard(), WIDTH, HEIGHT).expect("inference runs");
        assert_eq!(mask.len(), MASK_WIDTH * MASK_HEIGHT);
        assert!(mask.iter().all(|value| (0. ..=1.).contains(value)));
    }

    #[test]
    fn the_blur_keeps_a_running_average_and_a_mask() {
        let mut blur = BackgroundBlur::new().expect("model loads");
        let mut frame = checkerboard();
        blur.apply(&mut frame, WIDTH, HEIGHT, None);
        blur.apply(&mut frame, WIDTH, HEIGHT, None);
        assert!(blur.average_ms() > 0.);
        assert_eq!(blur.mask.len(), MASK_WIDTH * MASK_HEIGHT);
    }
}
