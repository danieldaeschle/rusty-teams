use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use calling::background::BackgroundPicture;
use calling::blur::BlurSettings;
use calling::camera::start_camera;
use calling::capture::Capture;
use calling::video_frame::picture_from_i420;
use calling::{BackgroundChoice, CallUpdate, DeviceChoice, VideoHub, VideoKey};
use gpui_kit::*;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::background::{BackgroundLibrary, BackgroundPick};
use super::pictures::CallPictures;

const PREVIEW_MAX_WIDTH: u32 = 640;
const FPS_WINDOW: Duration = Duration::from_millis(900);

pub struct CameraPreview {
    hub: Arc<VideoHub>,
    pictures: CallPictures,
    blur: Arc<BlurSettings>,
    background_request: Arc<Mutex<u64>>,
    capture: Option<Capture>,
    frames: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<String>>>,
    counted: (Instant, u64),
    fps: Option<f32>,
}

impl CameraPreview {
    pub fn new() -> (Self, UnboundedReceiver<CallUpdate>) {
        let (sender, receiver) = unbounded_channel();
        let preview = CameraPreview {
            hub: VideoHub::new(sender),
            pictures: CallPictures::default(),
            blur: Arc::new(BlurSettings::default()),
            background_request: Arc::new(Mutex::new(0)),
            capture: None,
            frames: Arc::new(AtomicU64::new(0)),
            failure: Arc::new(Mutex::new(None)),
            counted: (Instant::now(), 0),
            fps: None,
        };
        (preview, receiver)
    }

    pub fn start(&mut self, choice: &DeviceChoice) {
        self.release_capture();
        *self.failure.lock().expect("preview failure lock") = None;
        self.frames.store(0, Ordering::Relaxed);
        self.counted = (Instant::now(), 0);
        self.fps = None;
        let hub = self.hub.clone();
        let frames = self.frames.clone();
        let failure = self.failure.clone();
        let started = start_camera(
            choice,
            self.blur.clone(),
            move |buffer| {
                frames.fetch_add(1, Ordering::Relaxed);
                if let Some(picture) = picture_from_i420(buffer, PREVIEW_MAX_WIDTH) {
                    hub.publish(VideoKey::LocalCamera, picture);
                }
            },
            move |reason| *failure.lock().expect("preview failure lock") = Some(reason),
        );
        match started {
            Ok(capture) => self.capture = Some(capture),
            Err(error) => {
                *self.failure.lock().expect("preview failure lock") = Some(error.to_string())
            }
        }
    }

    pub fn set_background(&self, choice: BackgroundChoice) {
        let mut latest = self
            .background_request
            .lock()
            .expect("background request lock");
        *latest += 1;
        let request = *latest;
        match choice {
            BackgroundChoice::None => self.apply_background(false, None),
            BackgroundChoice::Blur => self.apply_background(true, None),
            BackgroundChoice::Image(path) => {
                let blur = self.blur.clone();
                let latest_request = self.background_request.clone();
                std::thread::spawn(move || {
                    let Ok(picture) = BackgroundPicture::load(&path) else {
                        return;
                    };
                    let latest = latest_request.lock().expect("background request lock");
                    if *latest == request {
                        blur.set_picture(Some(Arc::new(picture)));
                        blur.set_enabled(true);
                    }
                });
            }
        }
    }

    fn apply_background(&self, enabled: bool, picture: Option<Arc<BackgroundPicture>>) {
        self.blur.set_picture(picture);
        self.blur.set_enabled(enabled);
    }

    pub fn take_frames(&mut self, cx: &mut App) {
        self.pictures.apply(&self.hub, cx);
    }

    pub fn image(&self, now: Instant) -> Option<Arc<RenderImage>> {
        self.pictures.live(&VideoKey::LocalCamera, now)
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.lock().expect("preview failure lock").clone()
    }

    pub fn fps(&self) -> Option<f32> {
        self.fps
    }

    pub fn tick(&mut self, now: Instant) {
        let total = self.frames.load(Ordering::Relaxed);
        let (since, previous) = self.counted;
        let elapsed = now.saturating_duration_since(since);
        if elapsed < FPS_WINDOW {
            return;
        }
        self.fps = self
            .capture
            .is_some()
            .then(|| frames_per_second(total.saturating_sub(previous), elapsed));
        self.counted = (now, total);
    }

    pub fn stop(&mut self, cx: &mut App) {
        self.release_capture();
        self.pictures.clear(cx);
        self.fps = None;
    }

    fn release_capture(&mut self) {
        if let Some(capture) = self.capture.take() {
            std::thread::spawn(move || drop(capture));
        }
    }
}

pub fn frames_per_second(frames: u64, elapsed: Duration) -> f32 {
    if elapsed.is_zero() {
        return 0.;
    }
    frames as f32 / elapsed.as_secs_f32()
}

pub fn preview_backgrounds(library: &BackgroundLibrary) -> Vec<(BackgroundPick, String)> {
    let defaults = library
        .shown()
        .iter()
        .filter(|image| library.cached_image(&image.id).is_some())
        .map(|image| {
            (
                BackgroundPick::Default(image.id.clone()),
                image.name.clone(),
            )
        });
    let customs = library
        .customs
        .iter()
        .filter(|path| path.exists())
        .map(|path| {
            let name = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            (BackgroundPick::Custom(path.clone()), name)
        });
    [
        (BackgroundPick::None, "None".to_owned()),
        (BackgroundPick::Blur, "Blur".to_owned()),
    ]
    .into_iter()
    .chain(defaults)
    .chain(customs)
    .collect()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        BackgroundLibrary, BackgroundPick, CameraPreview, frames_per_second, preview_backgrounds,
    };

    #[test]
    fn frames_per_second_divides_by_elapsed_time() {
        assert_eq!(frames_per_second(30, Duration::from_secs(1)), 30.);
        assert_eq!(frames_per_second(15, Duration::from_millis(500)), 30.);
        assert_eq!(frames_per_second(10, Duration::ZERO), 0.);
    }

    #[test]
    fn the_background_choices_start_with_none_and_blur() {
        let library =
            BackgroundLibrary::new(std::env::temp_dir().join("teams-preview-test-no-cache"));
        let picks: Vec<BackgroundPick> = preview_backgrounds(&library)
            .into_iter()
            .map(|(pick, _)| pick)
            .collect();
        assert_eq!(picks, vec![BackgroundPick::None, BackgroundPick::Blur]);
    }

    #[test]
    fn a_preview_without_a_camera_reports_no_frames() {
        let (preview, _receiver) = CameraPreview::new();
        assert_eq!(preview.fps(), None);
        assert!(preview.image(Instant::now()).is_none());
    }
}
