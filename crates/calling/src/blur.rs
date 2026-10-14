use std::io::Cursor;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tract_onnx::prelude::*;

use libwebrtc::native::yuv_helper::i420_to_abgr;
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::background::BackgroundPicture;
use crate::effects::CameraEffects;
use crate::error::{Error, Result};
use crate::matte::{Matte, Planes, refine, refine_rgba};
use crate::standout::Cutout;

const MODEL: &[u8] = include_bytes!("../assets/selfie_segmentation_landscape.onnx");
pub const MASK_WIDTH: usize = 256;
pub const MASK_HEIGHT: usize = 144;
const CHANNELS: usize = 4;
const BACKGROUND_SCALE: usize = 4;
const BLUR_RADIUS: usize = 3;
const BLUR_PASSES: usize = 2;
const MASK_SMOOTHING: f32 = 0.35;
const MOTION_GAIN: f32 = 2.;
const SLOW_INFERENCE: Duration = Duration::from_millis(22);
const VERY_SLOW_INFERENCE: Duration = Duration::from_millis(45);
const MAX_BANDS: usize = 4;
const MASK_INPUT_WIDTH: u32 = 512;

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

pub(crate) struct Axis {
    pub(crate) first: Vec<usize>,
    pub(crate) second: Vec<usize>,
    pub(crate) weight: Vec<f32>,
}

pub(crate) fn axis(output: usize, input: usize) -> Axis {
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

fn blur_pass(input: &[u8], output: &mut [u8], lines: usize, length: usize, line_stride: usize, step: usize, channels: usize) {
    let radius = BLUR_RADIUS as isize;
    let window = (BLUR_RADIUS * 2 + 1) as u32;
    for line in 0..lines {
        for channel in 0..channels {
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

fn box_blur(image: &mut [u8], width: usize, height: usize, scratch: &mut [u8], channels: usize) {
    blur_pass(image, scratch, height, width, width * channels, channels, channels);
    blur_pass(scratch, image, width, height, channels, width * channels, channels);
}

fn blurred_background(rgba: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let (mut small, small_width, small_height) = downscaled(rgba, width, height);
    let mut scratch = vec![0u8; small.len()];
    for _ in 0..BLUR_PASSES {
        box_blur(&mut small, small_width, small_height, &mut scratch, 3);
    }
    (small, small_width, small_height)
}

struct Composer<'a> {
    background: &'a [u8],
    matte: &'a Matte,
    width: usize,
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
        let (small_top, small_bottom, small_fraction) = (self.small_y.first[row] * self.small_width, self.small_y.second[row] * self.small_width, self.small_y.weight[row]);
        let alphas = &self.matte.alpha[row * self.matte.width..][..self.matte.width];
        for (column, (pixel, &alpha)) in line.as_chunks_mut::<CHANNELS>().0.iter_mut().zip(alphas).enumerate() {
            if alpha == u8::MAX {
                continue;
            }
            let person = f32::from(alpha) / 255.;
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

fn compose_over(rgba: &mut [u8], width: usize, height: usize, matte: &Matte, background: &[u8], background_width: usize, background_height: usize) {
    let composer = Composer {
        background,
        matte,
        width,
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

fn downscaled_plane(plane: &[u8], stride: usize, width: usize, height: usize, factor: usize) -> (Vec<u8>, usize, usize) {
    let small_width = width.div_ceil(factor);
    let small_height = height.div_ceil(factor);
    let mut small = vec![0u8; small_width * small_height];
    for (row, line) in small.chunks_exact_mut(small_width).enumerate() {
        let rows = row * factor..((row + 1) * factor).min(height);
        let mut sums = vec![0u32; small_width];
        for source_row in rows.clone() {
            for (sum, group) in sums.iter_mut().zip(plane[source_row * stride..][..width].chunks(factor)) {
                *sum += group.iter().map(|&value| u32::from(value)).sum::<u32>();
            }
        }
        for (column, (slot, sum)) in line.iter_mut().zip(&sums).enumerate() {
            let columns = factor.min(width - column * factor);
            *slot = (sum / (rows.len() * columns) as u32) as u8;
        }
    }
    (small, small_width, small_height)
}

fn blurred_plane(plane: &[u8], stride: usize, width: usize, height: usize, factor: usize) -> (Vec<u8>, usize, usize) {
    let (mut small, small_width, small_height) = downscaled_plane(plane, stride, width, height, factor);
    let mut scratch = vec![0u8; small.len()];
    for _ in 0..BLUR_PASSES {
        box_blur(&mut small, small_width, small_height, &mut scratch, 1);
    }
    (small, small_width, small_height)
}

struct PlaneComposer<'a> {
    background: &'a [u8],
    matte: &'a Matte,
    width: usize,
    small_x: Axis,
    small_y: Axis,
    small_width: usize,
}

impl PlaneComposer<'_> {
    fn new<'a>(background: &'a [u8], small_width: usize, small_height: usize, matte: &'a Matte) -> PlaneComposer<'a> {
        let (width, height) = (matte.width, matte.height);
        PlaneComposer {
            background,
            matte,
            width,
            small_x: axis(width, small_width),
            small_y: axis(height, small_height),
            small_width,
        }
    }

    fn rows(&self, plane: &mut [u8], stride: usize, first_row: usize) {
        for (offset, line) in plane.chunks_mut(stride).enumerate() {
            let length = self.width.min(line.len());
            self.row(&mut line[..length], first_row + offset);
        }
    }

    fn row(&self, line: &mut [u8], row: usize) {
        let (small_top, small_bottom, small_fraction) = (self.small_y.first[row] * self.small_width, self.small_y.second[row] * self.small_width, self.small_y.weight[row]);
        let alphas = &self.matte.alpha[row * self.width..][..self.width];
        for (column, (value, &alpha)) in line.iter_mut().zip(alphas).enumerate() {
            if alpha == u8::MAX {
                continue;
            }
            let person = f32::from(alpha) / 255.;
            let (left, right, across) = (self.small_x.first[column], self.small_x.second[column], self.small_x.weight[column]);
            let sample = |line: usize, index: usize| f32::from(self.background[line + index]);
            let upper = sample(small_top, left) * (1. - across) + sample(small_top, right) * across;
            let lower = sample(small_bottom, left) * (1. - across) + sample(small_bottom, right) * across;
            let blurred = upper * (1. - small_fraction) + lower * small_fraction;
            *value = (f32::from(*value) * person + blurred * (1. - person) + 0.5) as u8;
        }
    }
}

/// `compose` on I420 planes: luma blurs at `BACKGROUND_SCALE`, chroma at the same grid, with no RGBA round trip.
pub fn compose_i420(buffer: &mut I420Buffer, mask: &[f32]) {
    let (width, height) = (buffer.width() as usize, buffer.height() as usize);
    let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (stride_y, stride_u, stride_v) = (stride_y as usize, stride_u as usize, stride_v as usize);
    let luma_matte = refine(mask, Planes::of(buffer));
    let chroma_matte = luma_matte.halved();
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    let (background_y, small_width, small_height) = blurred_plane(plane_y, stride_y, width, height, BACKGROUND_SCALE);
    let (background_u, ..) = blurred_plane(plane_u, stride_u, chroma_width, chroma_height, BACKGROUND_SCALE / 2);
    let (background_v, ..) = blurred_plane(plane_v, stride_v, chroma_width, chroma_height, BACKGROUND_SCALE / 2);
    let luma = PlaneComposer::new(&background_y, small_width, small_height, &luma_matte);
    let blue = PlaneComposer::new(&background_u, small_width, small_height, &chroma_matte);
    let red = PlaneComposer::new(&background_v, small_width, small_height, &chroma_matte);
    let bands = std::thread::available_parallelism().map_or(1, usize::from).min(MAX_BANDS);
    std::thread::scope(|scope| {
        for (composer, plane, stride, rows) in [(&luma, plane_y, stride_y, height), (&blue, plane_u, stride_u, chroma_height), (&red, plane_v, stride_v, chroma_height)] {
            let rows_per_band = rows.div_ceil(bands);
            for (index, band) in plane.chunks_mut(rows_per_band * stride).enumerate() {
                scope.spawn(move || composer.rows(band, stride, index * rows_per_band));
            }
        }
    });
}

/// Keeps the person (mask near 1) sharp and replaces the rest with a blurred copy; `mask` is `MASK_WIDTH` x `MASK_HEIGHT`.
pub fn compose(rgba: &mut [u8], width: usize, height: usize, mask: &[f32]) {
    let matte = refine_rgba(mask, rgba, width, height);
    compose_blurred(rgba, width, height, &matte);
}

fn compose_blurred(rgba: &mut [u8], width: usize, height: usize, matte: &Matte) {
    let (background, small_width, small_height) = blurred_background(rgba, width, height);
    compose_over(rgba, width, height, matte, &background, small_width, small_height);
}

pub fn compose_image(rgba: &mut [u8], width: usize, height: usize, matte: &Matte, covered: &[u8]) {
    compose_over(rgba, width, height, matte, covered, width, height);
}

/// Shared between the UI command and the camera thread.
#[derive(Default)]
pub struct BlurSettings {
    enabled: AtomicBool,
    standout: AtomicBool,
    average_micros: AtomicU32,
    picture: Mutex<Option<Arc<BackgroundPicture>>>,
    effects: Mutex<CameraEffects>,
    cutout: Mutex<Option<Arc<Cutout>>>,
}

impl BlurSettings {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_standout(&self, standout: bool) {
        self.standout.store(standout, Ordering::Relaxed);
        if !standout {
            self.set_cutout(None);
        }
    }

    pub fn standout(&self) -> bool {
        self.standout.load(Ordering::Relaxed)
    }

    pub fn set_cutout(&self, cutout: Option<Arc<Cutout>>) {
        *self.cutout.lock().expect("cutout lock") = cutout;
    }

    pub fn cutout(&self) -> Option<Arc<Cutout>> {
        self.cutout.lock().expect("cutout lock").clone()
    }

    pub fn set_picture(&self, picture: Option<Arc<BackgroundPicture>>) {
        *self.picture.lock().expect("picture lock") = picture;
    }

    fn picture(&self) -> Option<Arc<BackgroundPicture>> {
        self.picture.lock().expect("picture lock").clone()
    }

    pub fn set_effects(&self, effects: CameraEffects) {
        *self.effects.lock().expect("effects lock") = effects;
    }

    pub fn effects(&self) -> CameraEffects {
        *self.effects.lock().expect("effects lock")
    }

    pub fn active(&self) -> bool {
        self.enabled() || self.standout() || self.effects().alters_frame()
    }

    pub fn average_ms(&self) -> f32 {
        self.average_micros.load(Ordering::Relaxed) as f32 / 1000.
    }

    pub fn set_average_ms(&self, average_ms: f64) {
        self.average_micros.store((average_ms * 1000.) as u32, Ordering::Relaxed);
    }
}

/// Replaces the background of a frame; the mask comes from the shared `MaskSource`.
pub struct BlurStage {
    settings: Arc<BlurSettings>,
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
        BlurStage { settings, covered: None }
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

    pub fn covers(&mut self, width: usize, height: usize) -> bool {
        self.refresh_covered(width, height);
        self.covered.is_some()
    }

    pub fn compose(&mut self, rgba: &mut [u8], width: usize, height: usize, matte: &Matte) {
        self.refresh_covered(width, height);
        match &self.covered {
            Some(covered) => compose_image(rgba, width, height, matte, &covered.rgb),
            None => compose_blurred(rgba, width, height, matte),
        }
    }
}

pub trait MaskInference: Send + 'static {
    fn infer(&self, frame: I420Buffer) -> Result<Vec<f32>>;
}

impl MaskInference for Segmenter {
    fn infer(&self, frame: I420Buffer) -> Result<Vec<f32>> {
        let (rgba, width, height) = mask_input(frame);
        self.mask(&rgba, width, height)
    }
}

fn mask_input(mut buffer: I420Buffer) -> (Vec<u8>, usize, usize) {
    let (width, height) = (buffer.width(), buffer.height());
    let small_width = width.min(MASK_INPUT_WIDTH) & !1;
    let small_height = (u64::from(height) * u64::from(small_width) / u64::from(width)) as u32 & !1;
    let small = if (small_width, small_height) == (width, height) { None } else { Some(buffer.scale(small_width as i32, small_height as i32)) };
    let source = small.as_ref().unwrap_or(&buffer);
    let (stride_y, stride_u, stride_v) = source.strides();
    let (plane_y, plane_u, plane_v) = source.data();
    let mut rgba = vec![0u8; small_width as usize * small_height as usize * CHANNELS];
    i420_to_abgr(plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, &mut rgba, small_width * CHANNELS as u32, small_width as i32, small_height as i32);
    (rgba, small_width as usize, small_height as usize)
}

type Frame = I420Buffer;

#[derive(Default)]
struct Inbox {
    newest: Option<Frame>,
    closed: bool,
}

#[derive(Default)]
struct Outbox {
    mask: Vec<f32>,
    version: u64,
}

struct WorkerShared {
    inbox: Mutex<Inbox>,
    wake: Condvar,
    outbox: Mutex<Outbox>,
    stride: AtomicU64,
}

fn stride_for(inference: Duration) -> u64 {
    if inference > VERY_SLOW_INFERENCE {
        3
    } else if inference > SLOW_INFERENCE {
        2
    } else {
        1
    }
}

fn smooth_into(current: &mut Vec<f32>, fresh: Vec<f32>) {
    if current.is_empty() {
        *current = fresh;
        return;
    }
    for (value, new) in current.iter_mut().zip(fresh) {
        let change = new - *value;
        let smoothing = MASK_SMOOTHING * (1. - change.abs() * MOTION_GAIN).max(0.);
        *value += change * (1. - smoothing);
    }
}

fn run_worker(shared: &WorkerShared, inference: &dyn MaskInference) {
    let mut smoothed = Vec::new();
    loop {
        let frame = {
            let mut inbox = shared.inbox.lock().expect("inbox lock");
            loop {
                if inbox.closed {
                    return;
                }
                if let Some(frame) = inbox.newest.take() {
                    break frame;
                }
                inbox = shared.wake.wait(inbox).expect("inbox lock");
            }
        };
        let started = Instant::now();
        let Ok(fresh) = inference.infer(frame) else { continue };
        shared.stride.store(stride_for(started.elapsed()), Ordering::Relaxed);
        smooth_into(&mut smoothed, fresh);
        let mut outbox = shared.outbox.lock().expect("outbox lock");
        outbox.mask.clone_from(&smoothed);
        outbox.version += 1;
    }
}

/// One segmentation mask for every effect: a worker thread segments the newest frame, the caller never waits on it.
pub struct MaskSource {
    shared: Arc<WorkerShared>,
    worker: Option<JoinHandle<()>>,
    mask: Vec<f32>,
    seen_version: u64,
    frames: u64,
}

impl MaskSource {
    pub fn new() -> Result<Self> {
        Ok(Self::with_inference(Segmenter::new()?))
    }

    pub fn with_inference(inference: impl MaskInference) -> Self {
        let shared = Arc::new(WorkerShared { inbox: Mutex::default(), wake: Condvar::new(), outbox: Mutex::default(), stride: AtomicU64::new(1) });
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("segmentation".to_owned())
            .spawn(move || run_worker(&worker_shared, &inference))
            .expect("segmentation thread");
        MaskSource { shared, worker: Some(worker), mask: Vec::new(), seen_version: 0, frames: 0 }
    }

    pub fn due(&self) -> bool {
        self.frames.is_multiple_of(self.shared.stride.load(Ordering::Relaxed)) || self.mask.is_empty()
    }

    pub fn mask(&self) -> &[f32] {
        &self.mask
    }

    /// Call once per frame; `frame` is only read when `due`. Answers whether a newer mask arrived.
    pub fn refresh(&mut self, frame: impl FnOnce() -> Frame) -> bool {
        let due = self.due();
        self.frames += 1;
        if due {
            self.shared.inbox.lock().expect("inbox lock").newest = Some(frame());
            self.shared.wake.notify_one();
        }
        let outbox = self.shared.outbox.lock().expect("outbox lock");
        if outbox.version == self.seen_version {
            return false;
        }
        self.seen_version = outbox.version;
        self.mask.clone_from(&outbox.mask);
        true
    }
}

impl Drop for MaskSource {
    fn drop(&mut self) {
        self.shared.inbox.lock().expect("inbox lock").closed = true;
        self.shared.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::i420_from_rgba;

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

    fn checkerboard_i420() -> I420Buffer {
        let mut buffer = I420Buffer::new(WIDTH as u32, HEIGHT as u32);
        let (stride_y, stride_u, _) = buffer.strides();
        let (plane_y, plane_u, plane_v) = buffer.data_mut();
        for row in 0..HEIGHT {
            for column in 0..WIDTH {
                plane_y[row * stride_y as usize + column] = if (row / 3 + column / 3) % 2 == 0 { 20 } else { 235 };
            }
        }
        for row in 0..HEIGHT / 2 {
            for column in 0..WIDTH / 2 {
                plane_u[row * stride_u as usize + column] = if (row / 3 + column / 3) % 2 == 0 { 60 } else { 200 };
            }
        }
        plane_v.fill(128);
        buffer
    }

    fn luma_roughness(buffer: &mut I420Buffer, x: usize, y: usize) -> i32 {
        let stride = buffer.strides().0 as usize;
        let plane = buffer.data_mut().0;
        (0..8).map(|step| (i32::from(plane[y * stride + x + step + 1]) - i32::from(plane[y * stride + x + step])).abs()).sum()
    }

    #[test]
    fn the_planar_blur_keeps_the_person_and_smooths_luma_and_chroma_outside() {
        let mut original = checkerboard_i420();
        let mut blurred = checkerboard_i420();
        compose_i420(&mut blurred, &oval_mask());
        let (center_x, center_y) = (WIDTH / 2, HEIGHT / 2);
        assert_eq!(luma_roughness(&mut blurred, center_x - 4, center_y), luma_roughness(&mut original, center_x - 4, center_y));
        assert!(luma_roughness(&mut blurred, 2, 2) * 4 < luma_roughness(&mut original, 2, 2));
        let chroma = |buffer: &mut I420Buffer, x: usize, y: usize| {
            let stride = buffer.strides().1 as usize;
            i32::from(buffer.data_mut().1[y * stride + x])
        };
        assert_eq!(chroma(&mut blurred, center_x / 2, center_y / 2), chroma(&mut original, center_x / 2, center_y / 2));
        let spread = |buffer: &mut I420Buffer| (2..10).map(|x| (chroma(buffer, x + 1, 2) - chroma(buffer, x, 2)).abs()).sum::<i32>();
        assert!(spread(&mut blurred) * 4 < spread(&mut original));
    }

    #[test]
    fn the_planar_blur_matches_the_rgba_blur_on_gray_frames() {
        let mut planar = checkerboard_i420();
        planar.data_mut().1.fill(128);
        compose_i420(&mut planar, &oval_mask());
        let mut rgba = checkerboard();
        compose(&mut rgba, WIDTH, HEIGHT, &oval_mask());
        let stride = planar.strides().0 as usize;
        let plane = planar.data().0;
        let worst = (0..HEIGHT).flat_map(|y| (0..WIDTH).map(move |x| (x, y))).map(|(x, y)| (i32::from(plane[y * stride + x]) - i32::from(pixel(&rgba, x, y))).abs()).max().unwrap();
        assert!(worst <= 24, "worst luma difference {worst}");
    }

    #[test]
    fn a_full_person_mask_leaves_the_planes_untouched() {
        let mut untouched = checkerboard_i420();
        compose_i420(&mut untouched, &vec![1.; MASK_WIDTH * MASK_HEIGHT]);
        let reference = checkerboard_i420();
        assert_eq!(untouched.data(), reference.data());
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
        compose_image(&mut replaced, WIDTH, HEIGHT, &refine_rgba(&oval_mask(), &original, WIDTH, HEIGHT), &cover_of([10, 120, 200]));
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
        compose_image(&mut replaced, WIDTH, HEIGHT, &refine_rgba(&oval_mask(), &original, WIDTH, HEIGHT), &cover_of([0, 0, 0]));
        let edge_row = HEIGHT / 2;
        let switches = (0..WIDTH - 1)
            .filter(|&x| pixel(&replaced, x, edge_row) != pixel(&original, x, edge_row) && pixel(&replaced, x + 1, edge_row) == pixel(&original, x + 1, edge_row))
            .count();
        assert!(switches <= 2);
        let mut everyone = original.clone();
        compose_image(&mut everyone, WIDTH, HEIGHT, &refine_rgba(&vec![1.; MASK_WIDTH * MASK_HEIGHT], &original, WIDTH, HEIGHT), &cover_of([0, 0, 0]));
        assert_eq!(everyone, original);
        let empty = refine_rgba(&vec![0.; MASK_WIDTH * MASK_HEIGHT], &original, WIDTH, HEIGHT);
        let mut nobody = original;
        compose_image(&mut nobody, WIDTH, HEIGHT, &empty, &cover_of([0, 0, 0]));
        assert!(nobody.chunks(CHANNELS).all(|pixel| pixel[..3] == [0, 0, 0] && pixel[3] == 255));
    }

    #[test]
    fn the_model_answers_one_probability_per_mask_pixel() {
        let segmenter = Segmenter::new().expect("model loads");
        let mask = segmenter.mask(&checkerboard(), WIDTH, HEIGHT).expect("inference runs");
        assert_eq!(mask.len(), MASK_WIDTH * MASK_HEIGHT);
        assert!(mask.iter().all(|value| (0. ..=1.).contains(value)));
    }

    fn wait_for_mask(source: &mut MaskSource, frame: impl Fn() -> Frame) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if source.refresh(&frame) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn the_mask_source_segments_when_due_and_keeps_one_mask() {
        let mut source = MaskSource::new().expect("model loads");
        assert!(wait_for_mask(&mut source, || i420_from_rgba(&checkerboard(), WIDTH as u32, HEIGHT as u32)));
        assert_eq!(source.mask().len(), MASK_WIDTH * MASK_HEIGHT);
        source.shared.stride.store(3, Ordering::Relaxed);
        source.frames = 1;
        assert!(!source.due());
        assert!(!source.refresh(|| unreachable!("a skipped frame is not read")));
    }

    struct SlowSegmenter {
        delay: Duration,
        seen: Arc<Mutex<Vec<u8>>>,
    }

    impl MaskInference for SlowSegmenter {
        fn infer(&self, frame: I420Buffer) -> Result<Vec<f32>> {
            let marker = frame.data().0[0];
            self.seen.lock().unwrap().push(marker);
            std::thread::sleep(self.delay);
            Ok(vec![f32::from(marker) / 255.; MASK_WIDTH * MASK_HEIGHT])
        }
    }

    fn marked_frame(marker: u8) -> Frame {
        let mut buffer = I420Buffer::new(2, 2);
        buffer.data_mut().0.fill(marker);
        buffer
    }

    #[test]
    fn a_slow_segmenter_never_blocks_the_sender() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut source = MaskSource::with_inference(SlowSegmenter { delay: Duration::from_millis(300), seen });
        let started = Instant::now();
        for _ in 0..30 {
            source.refresh(|| marked_frame(1));
        }
        assert!(started.elapsed() < Duration::from_millis(150));
    }

    #[test]
    fn the_worker_hands_over_the_newest_frame_and_drops_stale_ones() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut source = MaskSource::with_inference(SlowSegmenter { delay: Duration::from_millis(100), seen: seen.clone() });
        for marker in [10, 20, 30, 40] {
            source.refresh(|| marked_frame(marker));
            std::thread::sleep(Duration::from_millis(5));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while seen.lock().unwrap().last() != Some(&40) && Instant::now() < deadline {
            source.refresh(|| marked_frame(40));
            std::thread::sleep(Duration::from_millis(10));
        }
        let processed = seen.lock().unwrap().clone();
        assert_eq!(processed.last(), Some(&40));
        assert!(!processed.contains(&20) || !processed.contains(&30), "stale frames pile up: {processed:?}");
    }
}
