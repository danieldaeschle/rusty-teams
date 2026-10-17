use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::blur::{BlurSettings, BlurStage, MaskSource};
use crate::standout::Cutout;
use crate::framing::{CropPlanner, Subject, crop_and_scale, find_subject, is_full_frame};
use crate::latest::Latest;
use crate::mask::Mask;
use crate::matte::{Planes, refine};
use crate::motion::{LumaGrid, propagate_for_display};
use crate::luma_effects::{apply_table, gamma_table, mean_luma, smooth_gamma, soften_luma, target_gamma};

pub const SOFT_FOCUS_DEFAULT: u8 = 50;
const AVERAGE_WEIGHT: f64 = 0.1;
const FIRST_FRAME_GAP: Duration = Duration::from_millis(33);
const GAMMA_NEUTRAL: f32 = 1.;
const GAMMA_VISIBLE: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraEffects {
    pub mirror: bool,
    pub auto_framing: bool,
    pub adjust_brightness: bool,
    pub soft_focus: bool,
    pub soft_focus_strength: u8,
}

impl Default for CameraEffects {
    fn default() -> Self {
        CameraEffects { mirror: true, auto_framing: false, adjust_brightness: false, soft_focus: false, soft_focus_strength: SOFT_FOCUS_DEFAULT }
    }
}

impl CameraEffects {
    pub fn needs_mask(&self) -> bool {
        self.auto_framing || self.soft_focus
    }

    pub fn alters_frame(&self) -> bool {
        self.auto_framing || self.adjust_brightness || self.soft_focus
    }

    pub fn to_meta(&self) -> String {
        format!("mirror={},framing={},brightness={},soft_focus={},strength={}", u8::from(self.mirror), u8::from(self.auto_framing), u8::from(self.adjust_brightness), u8::from(self.soft_focus), self.soft_focus_strength)
    }

    pub fn from_meta(value: &str) -> Self {
        let mut effects = CameraEffects::default();
        for (key, setting) in value.split(',').filter_map(|part| part.split_once('=')) {
            match key {
                "mirror" => effects.mirror = setting == "1",
                "framing" => effects.auto_framing = setting == "1",
                "brightness" => effects.adjust_brightness = setting == "1",
                "soft_focus" => effects.soft_focus = setting == "1",
                "strength" => effects.soft_focus_strength = setting.parse().map_or(SOFT_FOCUS_DEFAULT, |strength: u8| strength.min(100)),
                _ => {}
            }
        }
        effects
    }
}

/// Runs on the effects thread: brightness, soft focus, background and auto framing share one mask per frame.
pub struct EffectsStage {
    settings: Arc<BlurSettings>,
    blur: BlurStage,
    masks: Option<MaskSource>,
    shown: Mask,
    unavailable: bool,
    subject: Option<Subject>,
    planner: CropPlanner,
    gamma: f32,
    previous_frame: Option<Instant>,
    processed: u64,
    average_ms: f64,
}

impl EffectsStage {
    pub fn new(settings: Arc<BlurSettings>) -> Self {
        EffectsStage {
            blur: BlurStage::new(settings.clone()),
            settings,
            masks: None,
            shown: Mask::default(),
            unavailable: false,
            subject: None,
            planner: CropPlanner::default(),
            gamma: GAMMA_NEUTRAL,
            previous_frame: None,
            processed: 0,
            average_ms: 0.,
        }
    }

    pub fn apply_i420(&mut self, mut buffer: I420Buffer) -> I420Buffer {
        let effects = self.settings.effects();
        let blur = self.settings.enabled();
        let standout = self.settings.standout();
        if !blur && !standout && !effects.alters_frame() {
            self.previous_frame = None;
            return buffer;
        }
        let started = Instant::now();
        let elapsed = self.previous_frame.replace(started).map_or(FIRST_FRAME_GAP, |previous| started.duration_since(previous));
        let (width, height) = (buffer.width() as usize, buffer.height() as usize);
        if effects.adjust_brightness {
            self.adjust_brightness(&mut buffer, width, height, elapsed);
        }
        if (blur || standout || effects.needs_mask()) && self.load_masks() {
            let fresh = self.refresh_mask(&buffer, effects.auto_framing);
            if effects.soft_focus {
                self.soften(&mut buffer, width, height, effects.soft_focus_strength);
            }
            if standout && fresh {
                self.publish_cutout(&mut buffer);
            }
            if blur {
                buffer = self.compose_background(buffer);
            }
        }
        buffer = self.frame(buffer, effects.auto_framing, elapsed);
        self.record(started);
        buffer
    }

    fn adjust_brightness(&mut self, buffer: &mut I420Buffer, width: usize, height: usize, elapsed: Duration) {
        let stride = buffer.strides().0 as usize;
        let plane = buffer.data_mut().0;
        let target = target_gamma(mean_luma(plane, stride, width, height));
        self.gamma = smooth_gamma(self.gamma, target, elapsed);
        if (self.gamma - GAMMA_NEUTRAL).abs() > GAMMA_VISIBLE {
            apply_table(plane, stride, width, height, &gamma_table(self.gamma));
        }
    }

    fn load_masks(&mut self) -> bool {
        if self.masks.is_none() && !self.unavailable {
            match MaskSource::new() {
                Ok(masks) => self.masks = Some(masks),
                Err(_) => self.unavailable = true,
            }
        }
        !self.unavailable
    }

    fn refresh_mask(&mut self, buffer: &I420Buffer, track_subject: bool) -> bool {
        let Some(masks) = self.masks.as_mut() else { return false };
        let fresh = masks.refresh(|| copy_frame(buffer));
        let (shown, moved) = propagate_for_display(masks.mask(), masks.reference(), &LumaGrid::of(buffer));
        self.shown = Mask { matte: shown.matte && !moved, ..shown };
        if track_subject && fresh {
            self.subject = find_subject(&self.shown.values, self.shown.width, self.shown.height);
        }
        fresh
    }

    fn publish_cutout(&self, buffer: &mut I420Buffer) {
        self.settings.set_cutout(Cutout::from_camera(buffer, &self.shown).map(Arc::new));
    }

    fn soften(&self, buffer: &mut I420Buffer, width: usize, height: usize, strength: u8) {
        if self.shown.is_empty() {
            return;
        }
        let stride = buffer.strides().0 as usize;
        soften_luma(buffer.data_mut().0, stride, width, height, &self.shown, strength);
    }

    fn compose_background(&mut self, mut buffer: I420Buffer) -> I420Buffer {
        if self.shown.is_empty() {
            return buffer;
        }
        let matte = refine(&self.shown, Planes::of(&buffer));
        self.blur.compose(&mut buffer, &matte);
        buffer
    }

    fn frame(&mut self, buffer: I420Buffer, auto_framing: bool, elapsed: Duration) -> I420Buffer {
        if !auto_framing {
            self.planner.reset();
            self.subject = None;
            return buffer;
        }
        let crop = self.planner.advance(self.subject.as_ref(), elapsed);
        if is_full_frame(&crop) { buffer } else { crop_and_scale(&buffer, &crop) }
    }

    fn record(&mut self, started: Instant) {
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        self.average_ms = if self.processed == 0 { elapsed } else { self.average_ms * (1. - AVERAGE_WEIGHT) + elapsed * AVERAGE_WEIGHT };
        self.processed += 1;
        self.settings.set_average_ms(self.average_ms);
    }
}

/// Runs the effects on a thread of its own, so capture never waits on them; a frame that arrives while one is still in work replaces the queued one.
pub struct EffectsThread {
    frames: Arc<Latest<I420Buffer>>,
    worker: Option<JoinHandle<()>>,
}

impl EffectsThread {
    pub fn spawn(settings: Arc<BlurSettings>, mut deliver: impl FnMut(I420Buffer) + Send + 'static) -> std::io::Result<Self> {
        let frames = Arc::new(Latest::default());
        let worker_frames = frames.clone();
        let worker = std::thread::Builder::new().name("camera effects".to_owned()).spawn(move || {
            let mut stage = EffectsStage::new(settings);
            while let Some(frame) = worker_frames.take() {
                deliver(stage.apply_i420(frame));
            }
        })?;
        Ok(EffectsThread { frames, worker: Some(worker) })
    }

    pub fn submit(&self, frame: I420Buffer) {
        self.frames.put(frame);
    }

    pub fn dropped(&self) -> u64 {
        self.frames.dropped()
    }
}

impl Drop for EffectsThread {
    fn drop(&mut self) {
        self.frames.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn copy_frame(buffer: &I420Buffer) -> I420Buffer {
    let mut copy = I420Buffer::new(buffer.width(), buffer.height());
    let (source_y, source_u, source_v) = buffer.data();
    let (target_y, target_u, target_v) = copy.data_mut();
    target_y.copy_from_slice(source_y);
    target_u.copy_from_slice(source_u);
    target_v.copy_from_slice(source_v);
    copy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_mirror_the_self_view_and_leave_the_stream_untouched() {
        let effects = CameraEffects::default();
        assert!(effects.mirror && !effects.alters_frame() && !effects.needs_mask());
        assert_eq!(effects.soft_focus_strength, 50);
    }

    #[test]
    fn the_effects_survive_a_round_trip_through_the_meta_text() {
        let effects = CameraEffects { mirror: false, auto_framing: true, adjust_brightness: true, soft_focus: true, soft_focus_strength: 80 };
        assert_eq!(CameraEffects::from_meta(&effects.to_meta()), effects);
        assert_eq!(CameraEffects::from_meta(""), CameraEffects::default());
        assert_eq!(CameraEffects::from_meta("strength=900,junk").soft_focus_strength, SOFT_FOCUS_DEFAULT);
    }

    #[test]
    fn only_framing_and_soft_focus_need_the_segmenter() {
        let framing = CameraEffects { auto_framing: true, ..CameraEffects::default() };
        let brightness = CameraEffects { adjust_brightness: true, ..CameraEffects::default() };
        assert!(framing.needs_mask() && framing.alters_frame());
        assert!(!brightness.needs_mask() && brightness.alters_frame());
    }

    #[test]
    fn an_idle_stage_hands_the_frame_back_untouched() {
        let settings = Arc::new(BlurSettings::default());
        let mut stage = EffectsStage::new(settings.clone());
        let mut buffer = I420Buffer::new(64, 36);
        buffer.data_mut().0.fill(77);
        let out = stage.apply_i420(buffer);
        assert!(out.data().0.iter().all(|&value| value == 77));
        settings.set_effects(CameraEffects { adjust_brightness: true, ..CameraEffects::default() });
        let mut dark = I420Buffer::new(64, 36);
        dark.data_mut().0.fill(30);
        let mut brightened = stage.apply_i420(dark);
        for _ in 0..200 {
            let mut again = I420Buffer::new(64, 36);
            again.data_mut().0.fill(30);
            brightened = stage.apply_i420(again);
        }
        assert!(brightened.data().0[0] > 30);
    }

    #[test]
    fn the_effects_thread_never_holds_capture_back_and_delivers_the_newest_frame() {
        let delivered = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = delivered.clone();
        let effects = EffectsThread::spawn(Arc::new(BlurSettings::default()), move |frame: I420Buffer| {
            std::thread::sleep(Duration::from_millis(40));
            sink.lock().unwrap().push(frame.data().0[0]);
        })
        .expect("thread starts");
        let started = Instant::now();
        for marker in 1..=30u8 {
            let mut frame = I420Buffer::new(4, 4);
            frame.data_mut().0.fill(marker);
            effects.submit(frame);
        }
        assert!(started.elapsed() < Duration::from_millis(30));
        let deadline = Instant::now() + Duration::from_secs(5);
        while delivered.lock().unwrap().last() != Some(&30) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let seen = delivered.lock().unwrap().clone();
        assert_eq!(seen.last(), Some(&30));
        assert!(seen.len() < 30 && effects.dropped() > 0, "{seen:?}");
    }

    fn timed<T>(label: &str, mut work: impl FnMut() -> T) -> f64 {
        let runs = 20;
        let started = Instant::now();
        for _ in 0..runs {
            std::hint::black_box(work());
        }
        let millis = started.elapsed().as_secs_f64() * 1000. / f64::from(runs);
        eprintln!("{label}: {millis:.2} ms per 720p frame");
        millis
    }

    #[test]
    #[ignore = "timing, run with --release --ignored --nocapture"]
    fn every_effect_stays_inside_the_frame_budget_at_720p() {
        let mut buffer = I420Buffer::new(1280, 720);
        buffer.data_mut().0.iter_mut().enumerate().for_each(|(index, value)| *value = (index % 200) as u8 + 20);
        let mask = crate::mask::Mask::from_fn(256, 144, |x, y| if ((x - 0.5) * (x - 0.5)) / 0.04 + ((y - 0.5) * (y - 0.5)) / 0.09 <= 1. { 1. } else { 0. });
        let crop = crate::framing::Crop { center_x: 0.5, center_y: 0.5, zoom: 1.6 };
        let total = [
            timed("brightness", || {
                let mut copy = I420Buffer::new(1280, 720);
                apply_table(copy.data_mut().0, 1280, 1280, 720, &gamma_table(0.7));
            }),
            timed("soft focus", || {
                let mut copy = I420Buffer::new(1280, 720);
                soften_luma(copy.data_mut().0, 1280, 1280, 720, &mask, 100);
            }),
            timed("frame copy", || copy_frame(&buffer)),
            timed("subject", || find_subject(&mask.values, mask.width, mask.height)),
            timed("crop and scale", || crop_and_scale(&buffer, &crop)),
        ]
        .iter()
        .sum::<f64>();
        eprintln!("sum without segmentation: {total:.2} ms");
        assert!(total < 33.);
    }
}
