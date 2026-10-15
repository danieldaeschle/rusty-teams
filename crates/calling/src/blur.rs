use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use libwebrtc::native::yuv_helper::i420_to_abgr;
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::background::BackgroundPicture;
use crate::effects::CameraEffects;
use crate::error::Result;
use crate::camera::i420_from_rgba;
use crate::composite::{Backdrop, PicturePlanes, composite};
use crate::mask::Mask;
use crate::segmentation::LadderSegmenter;
use crate::matte::{Matte, Planes, refine};
use crate::standout::Cutout;

const CHANNELS: usize = 4;
const MASK_SMOOTHING: f32 = 0.35;
const MOTION_GAIN: f32 = 2.;
const SLOW_INFERENCE: Duration = Duration::from_millis(22);
const VERY_SLOW_INFERENCE: Duration = Duration::from_millis(45);

pub fn compose_i420(buffer: &mut I420Buffer, mask: &Mask) {
    let matte = refine(mask, Planes::of(buffer));
    composite(buffer, &matte, Backdrop::Blur);
}

fn through_i420(rgba: &mut [u8], width: usize, height: usize, apply: impl FnOnce(&mut I420Buffer)) {
    let mut buffer = i420_from_rgba(rgba, width as u32, height as u32);
    apply(&mut buffer);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data();
    i420_to_abgr(plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, rgba, (width * CHANNELS) as u32, width as i32, height as i32);
}

/// Keeps the person (mask near 1) sharp and replaces the rest with a blurred copy.
pub fn compose(rgba: &mut [u8], width: usize, height: usize, mask: &Mask) {
    through_i420(rgba, width, height, |buffer| compose_i420(buffer, mask));
}

pub fn compose_image(rgba: &mut [u8], width: usize, height: usize, matte: &Matte, picture: &PicturePlanes) {
    through_i420(rgba, width, height, |buffer| composite(buffer, matte, Backdrop::Picture(picture)));
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
    planes: PicturePlanes,
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
        let fresh = self.covered.as_ref().is_some_and(|covered| Arc::ptr_eq(&covered.picture, &picture) && covered.planes.fits(width, height));
        if !fresh {
            let planes = PicturePlanes::new(&picture.covering(width, height), width, height);
            self.covered = Some(CoveredPicture { picture, planes });
        }
    }

    pub fn compose(&mut self, buffer: &mut I420Buffer, matte: &Matte) {
        self.refresh_covered(buffer.width() as usize, buffer.height() as usize);
        let backdrop = self.covered.as_ref().map_or(Backdrop::Blur, |covered| Backdrop::Picture(&covered.planes));
        composite(buffer, matte, backdrop);
    }
}

pub trait MaskInference: Send + 'static {
    fn infer(&mut self, frame: I420Buffer) -> Result<Mask>;
}

type Frame = I420Buffer;

#[derive(Default)]
struct Inbox {
    newest: Option<Frame>,
    closed: bool,
}

#[derive(Default)]
struct Outbox {
    mask: Mask,
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

fn smooth_into(current: &mut Mask, fresh: Mask) {
    if (current.width, current.height, current.matte) != (fresh.width, fresh.height, fresh.matte) {
        *current = fresh;
        return;
    }
    for (value, new) in current.values.iter_mut().zip(fresh.values) {
        let change = new - *value;
        let smoothing = MASK_SMOOTHING * (1. - change.abs() * MOTION_GAIN).max(0.);
        *value += change * (1. - smoothing);
    }
}

fn run_worker(shared: &WorkerShared, inference: &mut dyn MaskInference) {
    let mut smoothed = Mask::default();
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
    mask: Mask,
    seen_version: u64,
    frames: u64,
}

impl MaskSource {
    /// The models load on the worker, so the first frames pass unchanged instead of the camera thread waiting on them.
    pub fn new() -> Result<Self> {
        Ok(Self::spawn(|| LadderSegmenter::new().ok()))
    }

    pub fn with_inference(inference: impl MaskInference) -> Self {
        Self::spawn(move || Some(inference))
    }

    fn spawn<Inference: MaskInference>(create: impl FnOnce() -> Option<Inference> + Send + 'static) -> Self {
        let shared = Arc::new(WorkerShared { inbox: Mutex::default(), wake: Condvar::new(), outbox: Mutex::default(), stride: AtomicU64::new(1) });
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("segmentation".to_owned())
            .spawn(move || {
                if let Some(mut inference) = create() {
                    run_worker(&worker_shared, &mut inference);
                }
            })
            .expect("segmentation thread");
        MaskSource { shared, worker: Some(worker), mask: Mask::default(), seen_version: 0, frames: 0 }
    }

    pub fn due(&self) -> bool {
        self.frames.is_multiple_of(self.shared.stride.load(Ordering::Relaxed)) || self.mask.is_empty()
    }

    pub fn mask(&self) -> &Mask {
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

    const MASK_WIDTH: usize = 256;
    const MASK_HEIGHT: usize = 144;
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

    fn oval_mask() -> Mask {
        Mask::from_fn(MASK_WIDTH, MASK_HEIGHT, |x, y| if ((x - 0.5) * (x - 0.5)) / 0.04 + ((y - 0.5) * (y - 0.5)) / 0.09 <= 1. { 1. } else { 0. })
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
        compose_i420(&mut untouched, &Mask::filled(MASK_WIDTH, MASK_HEIGHT, 1.));
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
        mask.values.fill(1.);
        let mut untouched = original.clone();
        compose(&mut untouched, WIDTH, HEIGHT, &mask);
        assert_eq!(untouched, original);
    }

    #[test]
    fn an_empty_mask_blurs_the_whole_frame() {
        let original = checkerboard();
        let mut blurred = original.clone();
        compose(&mut blurred, WIDTH, HEIGHT, &Mask::filled(MASK_WIDTH, MASK_HEIGHT, 0.));
        assert!(roughness(&blurred, 70, 40) * 4 < roughness(&original, 70, 40));
        assert!(blurred.chunks(CHANNELS).all(|pixel| pixel[3] == 255));
    }

    fn cover_of(color: [u8; 3]) -> PicturePlanes {
        PicturePlanes::new(&BackgroundPicture::from_rgb(color.repeat(4 * 4), 4, 4).unwrap().covering(WIDTH, HEIGHT), WIDTH, HEIGHT)
    }

    fn refine_rgba(mask: &Mask, rgba: &[u8], width: usize, height: usize) -> Matte {
        crate::matte::refine_rgba(mask, rgba, width, height)
    }

    #[test]
    fn the_person_stays_and_the_picture_replaces_everything_else() {
        let original = checkerboard();
        let mut replaced = original.clone();
        compose_image(&mut replaced, WIDTH, HEIGHT, &refine_rgba(&oval_mask(), &original, WIDTH, HEIGHT), &cover_of([10, 120, 200]));
        let (center_x, center_y) = (WIDTH / 2, HEIGHT / 2);
        assert_eq!(pixel(&replaced, center_x, center_y), pixel(&original, center_x, center_y));
        for (x, y) in [(2, 2), (WIDTH - 3, HEIGHT - 3), (2, HEIGHT - 3)] {
            let shown = &replaced[(y * WIDTH + x) * CHANNELS..][..4];
            assert!(shown.iter().zip([10, 120, 200, 255]).all(|(&shown, wanted)| shown.abs_diff(wanted) <= 3), "{shown:?}");
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
        compose_image(&mut everyone, WIDTH, HEIGHT, &refine_rgba(&Mask::filled(MASK_WIDTH, MASK_HEIGHT, 1.), &original, WIDTH, HEIGHT), &cover_of([0, 0, 0]));
        assert_eq!(everyone, original);
        let empty = refine_rgba(&Mask::filled(MASK_WIDTH, MASK_HEIGHT, 0.), &original, WIDTH, HEIGHT);
        let mut nobody = original;
        compose_image(&mut nobody, WIDTH, HEIGHT, &empty, &cover_of([0, 0, 0]));
        assert!(nobody.chunks(CHANNELS).all(|pixel| pixel[..3] == [0, 0, 0] && pixel[3] == 255));
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
        assert_eq!(source.mask().values.len(), MASK_WIDTH * MASK_HEIGHT);
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
        fn infer(&mut self, frame: I420Buffer) -> Result<Mask> {
            let marker = frame.data().0[0];
            self.seen.lock().unwrap().push(marker);
            std::thread::sleep(self.delay);
            Ok(Mask::filled(MASK_WIDTH, MASK_HEIGHT, f32::from(marker) / 255.))
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
