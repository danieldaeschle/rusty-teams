use std::cell::Cell;
use std::rc::Rc;

pub const ZOOM_MIN: f32 = 1.;
pub const ZOOM_MAX: f32 = 4.;
const ZOOM_STEP: f32 = 1.25;
const WHEEL_ZOOM_PER_PIXEL: f32 = 0.0015;
pub const STRIP_MIN_WIDTH: f32 = 160.;
pub const STRIP_MAX_WIDTH: f32 = 400.;
pub const STRIP_DEFAULT_WIDTH: f32 = 232.;
pub const STRIP_WIDTH_META_KEY: &str = "call_strip_width";
pub const SHARE_MOTION_META_KEY: &str = "call_share_motion";
pub const SHARE_STANDOUT_META_KEY: &str = "call_share_standout";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShareView {
    zoom: f32,
    offset: (f32, f32),
}

impl Default for ShareView {
    fn default() -> Self {
        ShareView { zoom: ZOOM_MIN, offset: (0., 0.) }
    }
}

fn clamp_axis(offset: f32, extent: f32, zoom: f32) -> f32 {
    offset.clamp(extent - extent * zoom, 0.)
}

impl ShareView {
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoom > ZOOM_MIN
    }

    pub fn percent(&self) -> u32 {
        (self.zoom * 100.).round() as u32
    }

    pub fn offset(&self, stage: (f32, f32)) -> (f32, f32) {
        (clamp_axis(self.offset.0, stage.0, self.zoom), clamp_axis(self.offset.1, stage.1, self.zoom))
    }

    pub fn reset(&mut self) {
        *self = ShareView::default();
    }

    pub fn zoom_around(&mut self, factor: f32, pointer: (f32, f32), stage: (f32, f32)) {
        let target = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        if target <= ZOOM_MIN {
            self.reset();
            return;
        }
        let (offset_x, offset_y) = self.offset(stage);
        let ratio = target / self.zoom;
        self.zoom = target;
        self.offset = (pointer.0 - (pointer.0 - offset_x) * ratio, pointer.1 - (pointer.1 - offset_y) * ratio);
        self.offset = self.offset(stage);
    }

    pub fn zoom_wheel(&mut self, scroll_pixels: f32, pointer: (f32, f32), stage: (f32, f32)) {
        self.zoom_around((scroll_pixels * WHEEL_ZOOM_PER_PIXEL).exp(), pointer, stage);
    }

    pub fn step_in(&mut self, stage: (f32, f32)) {
        self.zoom_around(ZOOM_STEP, (stage.0 / 2., stage.1 / 2.), stage);
    }

    pub fn step_out(&mut self, stage: (f32, f32)) {
        self.zoom_around(1. / ZOOM_STEP, (stage.0 / 2., stage.1 / 2.), stage);
    }

    pub fn pan_by(&mut self, delta: (f32, f32), stage: (f32, f32)) {
        if !self.is_zoomed() {
            return;
        }
        let (offset_x, offset_y) = self.offset(stage);
        self.offset = (offset_x + delta.0, offset_y + delta.1);
        self.offset = self.offset(stage);
    }
}

pub fn clamp_strip_width(width: f32) -> f32 {
    width.clamp(STRIP_MIN_WIDTH, STRIP_MAX_WIDTH)
}

pub fn load_strip_width(store: &store::Store) -> f32 {
    store
        .meta(STRIP_WIDTH_META_KEY)
        .ok()
        .flatten()
        .and_then(|text| text.parse().ok())
        .map_or(STRIP_DEFAULT_WIDTH, clamp_strip_width)
}

pub fn load_flag(store: &store::Store, key: &str) -> bool {
    store.meta(key).ok().flatten().as_deref() == Some("1")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripDrag {
    pub start_pointer: f32,
    pub start_width: f32,
}

impl StripDrag {
    pub fn width_at(&self, pointer: f32) -> f32 {
        clamp_strip_width(self.start_width + self.start_pointer - pointer)
    }
}

#[derive(Default)]
pub struct ShareStage {
    pub view: ShareView,
    pub pan_from: Option<(f32, f32)>,
    pub strip_drag: Option<StripDrag>,
    pub popped: bool,
    pub bounds: Rc<Cell<(f32, f32, f32, f32)>>,
}

impl ShareStage {
    pub fn size(&self) -> (f32, f32) {
        let (_, _, width, height) = self.bounds.get();
        (width, height)
    }

    pub fn pointer(&self, position: (f32, f32)) -> (f32, f32) {
        let (left, top, _, _) = self.bounds.get();
        (position.0 - left, position.1 - top)
    }

    pub fn end(&mut self) {
        self.view.reset();
        self.pan_from = None;
        self.popped = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: (f32, f32) = (800., 450.);

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let mut view = ShareView::default();
        let pointer = (600., 100.);
        view.zoom_around(2., pointer, STAGE);
        let (offset_x, offset_y) = view.offset(STAGE);
        let content = ((pointer.0 - offset_x) / view.zoom(), (pointer.1 - offset_y) / view.zoom());
        assert!((content.0 - 600.).abs() < 0.01 && (content.1 - 100.).abs() < 0.01);
    }

    #[test]
    fn zoom_stays_between_one_and_four_times() {
        let mut view = ShareView::default();
        view.zoom_around(100., (400., 225.), STAGE);
        assert_eq!(view.zoom(), ZOOM_MAX);
        view.zoom_around(0.0001, (400., 225.), STAGE);
        assert!(!view.is_zoomed());
        assert_eq!(view.offset(STAGE), (0., 0.));
    }

    #[test]
    fn panning_never_leaves_the_content() {
        let mut view = ShareView::default();
        view.pan_by((50., 50.), STAGE);
        assert_eq!(view.offset(STAGE), (0., 0.));
        view.zoom_around(2., (0., 0.), STAGE);
        view.pan_by((-5000., -5000.), STAGE);
        assert_eq!(view.offset(STAGE), (-800., -450.));
        view.pan_by((5000., 5000.), STAGE);
        assert_eq!(view.offset(STAGE), (0., 0.));
    }

    #[test]
    fn keyboard_steps_zoom_around_the_center_and_zero_resets() {
        let mut view = ShareView::default();
        view.step_in(STAGE);
        view.step_in(STAGE);
        assert_eq!(view.percent(), 156);
        view.step_out(STAGE);
        assert_eq!(view.percent(), 125);
        view.reset();
        assert_eq!(view.percent(), 100);
    }

    #[test]
    fn wheel_up_zooms_in_and_down_zooms_out() {
        let mut view = ShareView::default();
        view.zoom_wheel(120., (400., 225.), STAGE);
        assert!(view.zoom() > 1.);
        let zoomed = view.zoom();
        view.zoom_wheel(-60., (400., 225.), STAGE);
        assert!(view.zoom() < zoomed);
    }

    #[test]
    fn the_strip_width_stays_between_160_and_400_and_follows_the_pointer() {
        let drag = StripDrag { start_pointer: 900., start_width: 232. };
        assert_eq!(drag.width_at(880.), 252.);
        assert_eq!(drag.width_at(0.), STRIP_MAX_WIDTH);
        assert_eq!(drag.width_at(2000.), STRIP_MIN_WIDTH);
        assert_eq!(clamp_strip_width(10.), STRIP_MIN_WIDTH);
    }
}
